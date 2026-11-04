//! Source callable inference. Runtime representation stays separate from types.
use super::{Checker, ClosureInfo, Scheme, Type};
use crate::ast::{Expr, FunctionOp, Param, ParamMode, Pattern, Program};
use std::collections::{HashMap, HashSet};

/// A natural-arity function has a curried logical type and one concrete base
/// environment. Curry tails capture that environment and the supplied prefix.
pub(crate) fn callable_type(
    environment: Type,
    params: Vec<Type>,
    modes: Vec<ParamMode>,
    result: Type,
) -> Type {
    if params.is_empty() {
        return Type::Callable {
            environment: Box::new(environment),
            params,
            modes,
            result: Box::new(result),
        };
    }
    let mut result = result;
    for index in (0..params.len()).rev() {
        let env = if index == 0 {
            environment.clone()
        } else {
            let mut captures = vec![environment.clone()];
            captures.extend_from_slice(&params[..index]);
            Type::Nominal(format!("$curry:{index}"), captures)
        };
        result = Type::Callable {
            environment: Box::new(env),
            params: vec![params[index].clone()],
            modes: vec![modes[index]],
            result: Box::new(result),
        };
    }
    result
}

pub(crate) fn children(expression: &Expr) -> Vec<&Expr> {
    match expression {
        Expr::Binary(_, a, b)
        | Expr::FunctionOp(_, a, b)
        | Expr::Let(_, a, b)
        | Expr::LetMut(_, a, b)
        | Expr::While(a, b) => vec![a, b],
        Expr::If(c, a, b) => vec![c, a, b],
        Expr::Lambda(_, body) | Expr::Assign(_, body) | Expr::FieldAssign(_, _, body) => vec![body],
        Expr::Apply(callee, args) => std::iter::once(callee.as_ref()).chain(args).collect(),
        Expr::Call(_, args) => args.iter().collect(),
        Expr::Record(_, fields) => fields.iter().map(|(_, value)| value).collect(),
        Expr::Match(value, arms) => std::iter::once(value.as_ref())
            .chain(arms.iter().map(|arm| &arm.body))
            .collect(),
        _ => vec![],
    }
}

pub(super) fn sites(program: &Program) -> HashMap<*const Expr, usize> {
    let mut sites = HashMap::new();
    let mut work: Vec<_> = program.functions.iter().rev().map(|f| &f.body).collect();
    while let Some(expr) = work.pop() {
        sites.insert(expr as *const Expr, sites.len());
        work.extend(children(expr).into_iter().rev());
    }
    sites
}

fn pattern_names(pattern: &Pattern, names: &mut HashSet<String>) {
    match pattern {
        Pattern::Binding(name) => {
            names.insert(name.clone());
        }
        Pattern::Constructor(_, fields) => {
            for field in fields {
                pattern_names(field, names);
            }
        }
        _ => {}
    }
}

/// Ordered lexical free names, including local call/field/assignment roots.
/// Module names and global declarations are filtered by the caller's scope.
pub(super) fn free_names(
    expression: &Expr,
    initial: HashSet<String>,
    constructors: &HashMap<String, super::Constructor>,
) -> Vec<String> {
    fn walk(
        expr: &Expr,
        bound: &HashSet<String>,
        seen: &mut HashSet<String>,
        out: &mut Vec<String>,
        constructors: &HashMap<String, super::Constructor>,
    ) {
        let mut name = |name: &str| {
            if !bound.contains(name) && seen.insert(name.into()) {
                out.push(name.into());
            }
        };
        match expr {
            Expr::Var(n)
            | Expr::Move(n)
            | Expr::Inout(n)
            | Expr::Field(n, _)
            | Expr::InoutField(n, _) => name(n),
            Expr::Call(n, args) => {
                if !constructors.contains_key(n) {
                    name(n);
                }
                for arg in args {
                    walk(arg, bound, seen, out, constructors);
                }
            }
            Expr::Assign(n, value) | Expr::FieldAssign(n, _, value) => {
                name(n);
                walk(value, bound, seen, out, constructors);
            }
            Expr::Let(n, value, body) | Expr::LetMut(n, value, body) => {
                walk(value, bound, seen, out, constructors);
                let mut inner = bound.clone();
                inner.insert(n.clone());
                walk(body, &inner, seen, out, constructors);
            }
            Expr::Lambda(params, body) => {
                let mut inner = bound.clone();
                inner.extend(params.iter().map(|p| p.name.clone()));
                walk(body, &inner, seen, out, constructors);
            }
            Expr::Match(value, arms) => {
                walk(value, bound, seen, out, constructors);
                for arm in arms {
                    let mut inner = bound.clone();
                    pattern_names(&arm.pattern, &mut inner);
                    walk(&arm.body, &inner, seen, out, constructors);
                }
            }
            _ => {
                for child in children(expr) {
                    walk(child, bound, seen, out, constructors);
                }
            }
        }
    }
    let mut out = vec![];
    walk(
        expression,
        &initial,
        &mut HashSet::new(),
        &mut out,
        constructors,
    );
    out
}

impl Checker {
    pub(super) fn named_callable(&mut self, name: &str) -> Result<Type, String> {
        if !self.source_functions.contains(name) {
            return Err(
                "E-FUNCTION-FFI: builtin and foreign functions currently require direct calls"
                    .into(),
            );
        }
        let signature = self
            .functions
            .get(name)
            .map(|s| self.copy_function(s))
            .transpose()?
            .ok_or_else(|| format!("E-NAME: unknown function '{name}'"))?;
        let modes = self
            .parameter_modes
            .get(name)
            .cloned()
            .unwrap_or_else(|| vec![ParamMode::Value; signature.params.len()]);
        if modes.iter().any(|mode| *mode != ParamMode::Value) {
            return Err("E-FUNCTION-MODE: first-class functions currently require value parameters; use a direct call for read/inout".into());
        }
        let (params, result) = self.instantiate_function(&signature)?;
        if params.contains(&Type::CStr) || result == Type::CStr {
            return Err("E-FUNCTION-FFI: cstr functions require direct foreign calls".into());
        }
        Ok(callable_type(
            Type::Nominal(format!("$fn:{name}"), vec![]),
            params,
            modes,
            result,
        ))
    }

    pub(super) fn infer_lambda(
        &mut self,
        expr: &Expr,
        params: &[Param],
        body: &Expr,
        locals: &mut HashMap<String, Scheme>,
    ) -> Result<Type, String> {
        let mut names = HashSet::new();
        for param in params {
            if !names.insert(param.name.clone()) {
                return Err(format!(
                    "E-NAME: duplicate lambda parameter '{}'",
                    param.name
                ));
            }
            if param.mode != ParamMode::Value {
                return Err("E-FUNCTION-MODE: lambdas currently require value parameters".into());
            }
        }
        let captures: Vec<_> = free_names(body, names, &self.constructors)
            .into_iter()
            .filter(|name| locals.contains_key(name))
            .collect();
        let mut captured_types = vec![];
        for name in &captures {
            let binding = &locals[name];
            if binding.mutable
                || binding.borrowed
                || self
                    .active_loans
                    .iter()
                    .any(|loan| loan.root == binding.local_id)
            {
                return Err(format!("E-FUNCTION-CAPTURE: '{name}' is mutable or borrowed; closures require immutable Copy captures"));
            }
            if !binding.quantified.is_empty() {
                return Err(format!("E-FUNCTION-CAPTURE: capturing polymorphic local '{name}' requires an explicit monomorphic binding"));
            }
            if self.has_owner_dependency(&binding.ty)? {
                return Err(format!(
                    "E-FUNCTION-CAPTURE: cannot implicitly copy owner '{name}' into a closure"
                ));
            }
            captured_types.push(self.copy_type(&binding.ty)?);
        }
        let mut scope = locals
            .iter()
            .map(|(n, s)| Ok((n.clone(), self.copy_scheme(s)?)))
            .collect::<Result<HashMap<_, _>, String>>()?;
        let mut types = vec![];
        for param in params {
            let ty = self.fresh();
            if let Some(annotation) = &param.annotation {
                let declared = self.declared_type(annotation, &HashMap::new())?;
                self.unify(self.copy_type(&ty)?, declared)?;
            }
            let local_id = self.local_id();
            scope.insert(
                param.name.clone(),
                Scheme {
                    quantified: vec![],
                    ty: self.copy_type(&ty)?,
                    mutable: false,
                    borrowed: false,
                    local_id,
                },
            );
            types.push(ty);
        }
        let result = self.infer(body, &mut scope)?;
        let site = self.expression_sites[&(expr as *const Expr)];
        self.closures
            .insert(expr as *const Expr, ClosureInfo { site, captures });
        Ok(callable_type(
            Type::Nominal(format!("$lambda:{site}"), captured_types),
            types,
            vec![ParamMode::Value; params.len()],
            result,
        ))
    }

    pub(super) fn infer_application(
        &mut self,
        expr: &Expr,
        callee: Type,
        args: &[Expr],
        locals: &mut HashMap<String, Scheme>,
    ) -> Result<Type, String> {
        self.application_types
            .insert(expr as *const Expr, self.copy_type(&callee)?);
        let mut current = callee;
        if args.is_empty() {
            let resolved = self.resolve(&current)?;
            return match resolved {
                Type::Var(_) => {
                    let result = self.fresh();
                    let env = self.fresh();
                    self.unify(
                        current,
                        callable_type(env, vec![], vec![], self.copy_type(&result)?),
                    )?;
                    Ok(result)
                }
                Type::Callable { params, result, .. } if params.is_empty() => Ok(*result),
                Type::Callable { .. } => {
                    Err("E-ARITY: nonzero-arity function needs an argument".into())
                }
                _ => Err("E-TYPE: application requires a function".into()),
            };
        }
        for (index, arg) in args.iter().enumerate() {
            let resolved = self.resolve(&current)?;
            let (parameter, result) = match resolved {
                Type::Var(_) => {
                    let parameter = self.fresh();
                    let result = self.fresh();
                    let env = self.fresh();
                    self.unify(
                        current,
                        callable_type(
                            env,
                            vec![self.copy_type(&parameter)?],
                            vec![ParamMode::Value],
                            self.copy_type(&result)?,
                        ),
                    )?;
                    (parameter, result)
                }
                Type::Callable {
                    params,
                    modes,
                    result,
                    ..
                } if params.len() == 1 && modes == [ParamMode::Value] => {
                    (params.into_iter().next().unwrap(), *result)
                }
                Type::Callable { .. } => {
                    return Err(
                        "E-ARITY: callable argument count or parameter convention differs".into(),
                    )
                }
                _ if index > 0 => {
                    return Err(
                        "E-ARITY: supplied arguments exceed the callable result's arity".into(),
                    )
                }
                _ => return Err("E-TYPE: application requires a function".into()),
            };
            let actual = self.infer(arg, locals)?;
            self.unify(parameter, actual)?;
            current = result;
        }
        Ok(current)
    }

    pub(super) fn infer_function_op(
        &mut self,
        expr: &Expr,
        op: FunctionOp,
        left: &Expr,
        right: &Expr,
        locals: &mut HashMap<String, Scheme>,
    ) -> Result<Type, String> {
        // Infer both operands in source order even when application is reversed.
        let left_ty = self.infer(left, locals)?;
        let right_ty = self.infer(right, locals)?;
        let unary = |checker: &mut Self, input: Type, output: Type| {
            let env = checker.fresh();
            callable_type(env, vec![input], vec![ParamMode::Value], output)
        };
        match op {
            FunctionOp::PipeForward => {
                let result = self.fresh();
                let expected = unary(self, left_ty, self.copy_type(&result)?);
                self.unify(right_ty, expected)?;
                Ok(result)
            }
            FunctionOp::PipeBackward => {
                let result = self.fresh();
                let expected = unary(self, right_ty, self.copy_type(&result)?);
                self.unify(left_ty, expected)?;
                Ok(result)
            }
            FunctionOp::ComposeForward | FunctionOp::ComposeBackward => {
                let input = self.fresh();
                let middle = self.fresh();
                let output = self.fresh();
                let first = unary(self, self.copy_type(&input)?, self.copy_type(&middle)?);
                let second = unary(self, middle, self.copy_type(&output)?);
                let (a, b) = if op == FunctionOp::ComposeForward {
                    (self.copy_type(&left_ty)?, self.copy_type(&right_ty)?)
                } else {
                    (self.copy_type(&right_ty)?, self.copy_type(&left_ty)?)
                };
                self.unify(a, first)?;
                self.unify(b, second)?;
                let site = self.expression_sites[&(expr as *const Expr)];
                let direction = if op == FunctionOp::ComposeForward {
                    "forward"
                } else {
                    "backward"
                };
                Ok(callable_type(
                    Type::Nominal(
                        format!("$compose:{site}:{direction}"),
                        vec![left_ty, right_ty],
                    ),
                    vec![input],
                    vec![ParamMode::Value],
                    output,
                ))
            }
        }
    }
}
