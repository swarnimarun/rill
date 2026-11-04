//! Checked closure conversion. Concrete environments are ordinary inline records.
use crate::ast::{
    Expr, Function, FunctionOp, MatchArm, Param, ParamMode, Pattern, Program, TypeDecl, TypeExpr,
    VariantDecl,
};
use crate::check::{CheckedProgram, Type};
use std::collections::HashMap;
use std::rc::Rc;

const LIMIT: usize = 1024;
type Substitution = HashMap<usize, Type>;
type Scope<'a> = HashMap<String, Binding<'a>>;
#[derive(Clone)]
struct Binding<'a> {
    emitted: String,
    template: Option<(&'a Expr, Substitution, Rc<Scope<'a>>)>,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Instance {
    source: String,
    params: Vec<Type>,
    result: Type,
}
enum Job {
    Source(Instance, String),
    Invoke(Type, String),
}
struct Converter<'a> {
    checked: CheckedProgram<'a>,
    output: Program,
    sites: HashMap<usize, &'a Expr>,
    instances: HashMap<Instance, String>,
    environments: HashMap<Type, String>,
    invocations: HashMap<Type, String>,
    jobs: Vec<Job>,
    serial: usize,
    work: usize,
}

pub fn needs_lowering(program: &Program) -> bool {
    let names: std::collections::HashSet<_> =
        program.functions.iter().map(|f| f.name.as_str()).collect();
    let mut pending: Vec<_> = program.functions.iter().map(|f| &f.body).collect();
    while let Some(expr) = pending.pop() {
        if matches!(
            expr,
            Expr::Lambda(..) | Expr::Apply(..) | Expr::FunctionOp(..)
        ) || matches!(expr, Expr::Var(name) if names.contains(name.as_str()))
            || matches!(expr, Expr::Call(name,args) if program.functions.iter().any(|f| f.name==*name && f.params.len()!=args.len()))
        {
            return true;
        }
        children(expr, &mut pending);
    }
    false
}

pub fn lower(program: &Program) -> Result<Program, String> {
    let checked = crate::check::analyze(program)?;
    let main = checked
        .functions
        .get("main")
        .ok_or("E-FUNCTION: missing main")?;
    if matches!(main.result, Type::Callable { .. }) {
        return Err("E-FUNCTION-RESULT: main cannot return a callable value".into());
    }
    let entry = Instance {
        source: "main".into(),
        params: vec![],
        result: main.result.clone(),
    };
    let mut output = program.clone();
    output.functions.clear();
    let mut sites = HashMap::new();
    let mut pending: Vec<_> = program.functions.iter().map(|f| &f.body).collect();
    while let Some(expr) = pending.pop() {
        if let Some(site) = checked.expression_site(expr) {
            sites.insert(site, expr);
        }
        children(expr, &mut pending);
    }
    let mut converter = Converter {
        checked,
        output,
        sites,
        instances: HashMap::new(),
        environments: HashMap::new(),
        invocations: HashMap::new(),
        jobs: vec![],
        serial: 0,
        work: 0,
    };
    converter.instances.insert(entry.clone(), "main".into());
    converter.jobs.push(Job::Source(entry, "main".into()));
    while let Some(job) = converter.jobs.pop() {
        let function = match job {
            Job::Source(key, name) => converter.source_function(&key, name)?,
            Job::Invoke(ty, name) => converter.invoke_function(&ty, name)?,
        };
        converter.output.functions.push(function);
    }
    crate::check::analyze(&converter.output)?;
    Ok(converter.output)
}

impl<'a> Converter<'a> {
    fn fresh(&mut self, category: &str) -> String {
        let id = self.serial;
        self.serial += 1;
        format!("M$999999${category}{id}")
    }
    fn charge(&mut self) -> Result<(), String> {
        self.work += 1;
        if self.work > 100_000 {
            return Err("E-FUNCTION-LIMIT: closure conversion work limit exceeded".into());
        }
        Ok(())
    }
    fn ty(&self, expr: &Expr, subst: &Substitution) -> Result<Type, String> {
        substitute(
            self.checked
                .expression_type(expr)
                .ok_or("E-FUNCTION: missing expression type")?,
            subst,
            0,
        )
    }
    fn source_instance(&mut self, key: Instance) -> Result<String, String> {
        if let Some(name) = self.instances.get(&key) {
            return Ok(name.clone());
        }
        if self.instances.len() + self.invocations.len() >= LIMIT {
            return Err("E-FUNCTION-LIMIT: specialization limit exceeded".into());
        }
        ensure_layout(&key.result)?;
        for ty in &key.params {
            ensure_layout(ty)?;
        }
        let name = self.fresh("rill_function");
        self.instances.insert(key.clone(), name.clone());
        self.jobs.push(Job::Source(key, name.clone()));
        Ok(name)
    }
    fn environment(&mut self, ty: &Type) -> Result<String, String> {
        let key = physical_type(ty, 0)?;
        if let Some(name) = self.environments.get(&key) {
            return Ok(name.clone());
        }
        ensure_layout(ty)?;
        if self.environments.len() >= LIMIT {
            return Err("E-FUNCTION-LIMIT: environment layout limit exceeded".into());
        }
        let Type::Callable { environment, .. } = ty else {
            return Err("E-FUNCTION: expected callable environment".into());
        };
        let Type::Nominal(_, _) = environment.as_ref() else {
            return Err("E-FUNCTION: unresolved callable environment".into());
        };
        let captures = physical_captures(environment)?;
        let name = self.fresh("RillClosure");
        self.environments.insert(key, name.clone());
        let fields = captures
            .iter()
            .map(|ty| self.annotation(ty))
            .collect::<Result<Vec<_>, _>>()?;
        self.output.types.push(TypeDecl {
            name: name.clone(),
            params: vec![],
            variants: vec![VariantDecl {
                name: name.clone(),
                fields,
            }],
            record_fields: Some((0..captures.len()).map(|i| format!("capture{i}")).collect()),
        });
        Ok(name)
    }
    fn annotation(&mut self, ty: &Type) -> Result<TypeExpr, String> {
        let (name, args) = match ty {
            Type::Callable { .. } => (self.environment(ty)?, vec![]),
            Type::Nominal(name, args) => (
                name.clone(),
                args.iter()
                    .map(|a| self.annotation(a))
                    .collect::<Result<_, _>>()?,
            ),
            Type::Int => ("i32".into(), vec![]),
            Type::Bool => ("bool".into(), vec![]),
            Type::Unit => ("unit".into(), vec![]),
            Type::StaticStr => ("str".into(), vec![]),
            Type::CStr => ("$cstr".into(), vec![]),
            Type::Bytes(n) => (format!("$bytes:{n}"), vec![]),
            Type::U8x16 => ("u8x16".into(), vec![]),
            Type::SystemBoxI32 => ("SystemBoxI32".into(), vec![]),
            Type::SystemBytes => ("SystemBytes".into(), vec![]),
            Type::CU8 => ("u8".into(), vec![]),
            Type::CU32 => ("u32".into(), vec![]),
            Type::CU64 => ("u64".into(), vec![]),
            Type::CF32 => ("f32".into(), vec![]),
            Type::CPointer { pointee, is_const } => (
                if *is_const { "const_ptr" } else { "ptr" }.into(),
                vec![TypeExpr {
                    name: pointee.clone(),
                    args: vec![],
                }],
            ),
            Type::Var(_) => {
                return Err(
                    "E-FUNCTION-LAYOUT: unresolved callable capture or function instance".into(),
                )
            }
        };
        Ok(TypeExpr { name, args })
    }
    fn source_function(&mut self, key: &Instance, name: String) -> Result<Function, String> {
        let source = self
            .checked
            .program
            .functions
            .iter()
            .find(|f| f.name == key.source)
            .ok_or("E-FUNCTION: missing source function")?;
        let signature = &self.checked.functions[&key.source];
        let mut subst = HashMap::new();
        for (a, b) in signature.params.iter().zip(&key.params) {
            constrain(a, b, &mut subst, 0)?;
        }
        constrain(&signature.result, &key.result, &mut subst, 0)?;
        let mut scope = Scope::new();
        let mut params = vec![];
        for (param, ty) in source.params.iter().zip(&key.params) {
            let emitted = self.fresh("rill_parameter");
            scope.insert(
                param.name.clone(),
                Binding {
                    emitted: emitted.clone(),
                    template: None,
                },
            );
            params.push(Param {
                name: emitted,
                mode: param.mode,
                annotation: Some(self.annotation(ty)?),
            });
        }
        let body = self.expr(&source.body, &subst, &scope, None)?;
        Ok(Function {
            name,
            params,
            result_annotation: Some(self.annotation(&key.result)?),
            body,
        })
    }
    fn invocation(&mut self, ty: &Type) -> Result<String, String> {
        if let Some(name) = self.invocations.get(ty) {
            return Ok(name.clone());
        }
        self.environment(ty)?;
        if self.instances.len() + self.invocations.len() >= LIMIT {
            return Err("E-FUNCTION-LIMIT: specialization limit exceeded".into());
        }
        let name = self.fresh("rill_invoke");
        self.invocations.insert(ty.clone(), name.clone());
        self.jobs.push(Job::Invoke(ty.clone(), name.clone()));
        Ok(name)
    }
    fn signature(&self, ty: &Type) -> Result<Signature, String> {
        let Type::Callable { environment, .. } = ty else {
            return Err("E-FUNCTION: expected callable".into());
        };
        let Type::Nominal(tag, fields) = environment.as_ref() else {
            return Err("E-FUNCTION: missing environment".into());
        };
        let (base, supplied) = if tag.starts_with("$curry:") {
            (
                fields
                    .first()
                    .ok_or("E-FUNCTION: invalid curry environment")?
                    .clone(),
                fields[1..].to_vec(),
            )
        } else {
            (environment.as_ref().clone(), vec![])
        };
        let Type::Nominal(tag, captures) = &base else {
            return Err("E-FUNCTION: invalid base environment".into());
        };
        let arity = if let Some(name) = tag.strip_prefix("$fn:") {
            self.checked
                .program
                .functions
                .iter()
                .find(|f| f.name == name)
                .ok_or("E-FUNCTION: missing callable declaration")?
                .params
                .len()
        } else if let Some(site) = tag.strip_prefix("$lambda:") {
            let site = site
                .parse::<usize>()
                .map_err(|_| "E-FUNCTION: invalid lambda site")?;
            let Expr::Lambda(params, _) = self
                .sites
                .get(&site)
                .ok_or("E-FUNCTION: missing lambda site")?
            else {
                return Err("E-FUNCTION: invalid lambda source".into());
            };
            params.len()
        } else if tag.starts_with("$compose:") {
            1
        } else {
            return Err(format!("E-FUNCTION: unsupported environment {tag}"));
        };
        let remaining = arity
            .checked_sub(supplied.len())
            .ok_or("E-FUNCTION: invalid supplied argument count")?;
        let mut params = supplied.clone();
        let mut current = ty;
        if remaining == 0 {
            let Type::Callable {
                params: empty,
                result,
                ..
            } = current
            else {
                return Err("E-FUNCTION: invalid nullary callable".into());
            };
            if !empty.is_empty() {
                return Err("E-FUNCTION: invalid nullary signature".into());
            }
            current = result;
        } else {
            for _ in 0..remaining {
                let Type::Callable {
                    params: next,
                    result,
                    ..
                } = current
                else {
                    return Err("E-FUNCTION: truncated curried signature".into());
                };
                if next.len() != 1 {
                    return Err("E-FUNCTION: invalid curried arity".into());
                }
                params.push(next[0].clone());
                current = result;
            }
        }
        Ok(Signature {
            tag: tag.clone(),
            base: base.clone(),
            captures: captures.clone(),
            supplied,
            params,
            result: current.clone(),
        })
    }
    fn invoke_function(&mut self, ty: &Type, name: String) -> Result<Function, String> {
        let signature = self.signature(ty)?;
        let env = self.fresh("rill_environment");
        let mut params = vec![Param {
            name: env.clone(),
            mode: ParamMode::Value,
            annotation: Some(self.annotation(ty)?),
        }];
        for ty in &signature.params[signature.supplied.len()..] {
            params.push(Param {
                name: self.fresh("rill_argument"),
                mode: ParamMode::Value,
                annotation: Some(self.annotation(ty)?),
            });
        }
        let capture = |i| Expr::Field(env.clone(), vec![format!("capture{i}")]);
        let mut args: Vec<_> = (0..signature.supplied.len())
            .map(|i| capture(signature.captures.len() + i))
            .collect();
        args.extend(params[1..].iter().map(|p| Expr::Var(p.name.clone())));
        let body = if let Some(source) = signature.tag.strip_prefix("$fn:") {
            let target = self.source_instance(Instance {
                source: source.into(),
                params: signature.params.clone(),
                result: signature.result.clone(),
            })?;
            Expr::Call(target, args)
        } else if let Some(site) = signature.tag.strip_prefix("$lambda:") {
            let site = site
                .parse::<usize>()
                .map_err(|_| "E-FUNCTION: malformed lambda identity")?;
            let expression = *self
                .sites
                .get(&site)
                .ok_or("E-FUNCTION: missing lambda source")?;
            let Expr::Lambda(original, body) = expression else {
                return Err("E-FUNCTION: invalid lambda source".into());
            };
            let names = self
                .checked
                .closure_info(expression)
                .ok_or("E-FUNCTION: missing captures")?
                .captures
                .clone();
            let mut subst = HashMap::new();
            let mut current = self
                .checked
                .expression_type(expression)
                .ok_or("E-FUNCTION: missing lambda type")?;
            let Type::Callable { environment, .. } = current else {
                return Err("E-FUNCTION: invalid lambda type".into());
            };
            constrain(environment, &signature.base, &mut subst, 0)?;
            if original.is_empty() {
                let Type::Callable { result, .. } = current else {
                    unreachable!()
                };
                current = result;
            }
            for actual in &signature.params {
                let Type::Callable { params, result, .. } = current else {
                    return Err("E-FUNCTION: invalid lambda parameters".into());
                };
                constrain(&params[0], actual, &mut subst, 0)?;
                current = result;
            }
            constrain(current, &signature.result, &mut subst, 0)?;
            let mut scope = Scope::new();
            let mut bindings = vec![];
            for (i, n) in names.iter().enumerate() {
                let emitted = self.fresh("rill_capture");
                scope.insert(
                    n.clone(),
                    Binding {
                        emitted: emitted.clone(),
                        template: None,
                    },
                );
                bindings.push((emitted, capture(i)));
            }
            for (p, argument) in original.iter().zip(&args) {
                let emitted = self.fresh("rill_parameter");
                scope.insert(
                    p.name.clone(),
                    Binding {
                        emitted: emitted.clone(),
                        template: None,
                    },
                );
                bindings.push((emitted, argument.clone()));
            }
            let mut converted = self.expr(body, &subst, &scope, None)?;
            for (emitted, value) in bindings.into_iter().rev() {
                converted = Expr::Let(emitted, Box::new(value), Box::new(converted));
            }
            converted
        } else if signature.tag.starts_with("$compose:") {
            let captures = &signature.captures;
            if captures.len() != 2 {
                return Err("E-FUNCTION: composition missing operands".into());
            }
            let backward = signature.tag.ends_with(":backward")
                || signature.tag.ends_with(":1")
                || signature.tag.ends_with(":ComposeBackward");
            let (first, second) = if backward { (1, 0) } else { (0, 1) };
            let Type::Callable { result: middle, .. } = &captures[first] else {
                return Err("E-FUNCTION: invalid composition".into());
            };
            let value = self.apply_values(capture(first), &captures[first], args, middle)?;
            self.apply_values(
                capture(second),
                &captures[second],
                vec![value],
                &signature.result,
            )?
        } else {
            return Err(format!(
                "E-FUNCTION: unsupported callable environment {}",
                signature.tag
            ));
        };
        Ok(Function {
            name,
            params,
            result_annotation: Some(self.annotation(&signature.result)?),
            body,
        })
    }
    fn record(&mut self, ty: &Type, values: Vec<Expr>) -> Result<Expr, String> {
        let name = self.environment(ty)?;
        Ok(Expr::Record(
            name,
            values
                .into_iter()
                .enumerate()
                .map(|(i, e)| (format!("capture{i}"), e))
                .collect(),
        ))
    }
    fn apply_values(
        &mut self,
        callee: Expr,
        callable: &Type,
        args: Vec<Expr>,
        result: &Type,
    ) -> Result<Expr, String> {
        let signature = self.signature(callable)?;
        let remaining = signature.params.len() - signature.supplied.len();
        if args.len() < remaining {
            let local = self.fresh("rill_partial");
            let mut values = (0..signature.captures.len() + signature.supplied.len())
                .map(|i| Expr::Field(local.clone(), vec![format!("capture{i}")]))
                .collect::<Vec<_>>();
            values.extend(args);
            let record = self.record(result, values)?;
            return Ok(Expr::Let(local, Box::new(callee), Box::new(record)));
        }
        let call = if let Some(source) = signature.tag.strip_prefix("$fn:") {
            // Named values retain a direct source call, without an extra
            // trampoline frame in the language's bounded call stack.
            let target = self.source_instance(Instance {
                source: source.into(),
                params: signature.params.clone(),
                result: signature.result.clone(),
            })?;
            let local = self.fresh("rill_callee");
            let mut arguments = (0..signature.supplied.len())
                .map(|i| {
                    Expr::Field(
                        local.clone(),
                        vec![format!("capture{}", signature.captures.len() + i)],
                    )
                })
                .collect::<Vec<_>>();
            arguments.extend(args[..remaining].iter().cloned());
            Expr::Let(
                local,
                Box::new(callee),
                Box::new(Expr::Call(target, arguments)),
            )
        } else {
            let target = self.invocation(callable)?;
            let mut arguments = vec![callee];
            arguments.extend(args[..remaining].iter().cloned());
            Expr::Call(target, arguments)
        };
        if args.len() == remaining {
            return Ok(call);
        }
        self.apply_values(call, &signature.result, args[remaining..].to_vec(), result)
    }
    fn binding_value(&mut self, name: &str, ty: &Type, scope: &Scope<'a>) -> Result<Expr, String> {
        if let Some(binding) = scope.get(name) {
            if let Some((expr, subst, captured)) = &binding.template {
                return self.expr(expr, subst, captured, Some(ty));
            }
            return Ok(Expr::Var(binding.emitted.clone()));
        }
        if matches!(ty, Type::Callable { .. }) {
            return self.record(ty, vec![]);
        }
        Ok(Expr::Var(name.into()))
    }
    fn pattern(&mut self, pattern: &Pattern, scope: &mut Scope<'a>) -> Pattern {
        match pattern {
            Pattern::Binding(name) => {
                let emitted = self.fresh("rill_pattern");
                scope.insert(
                    name.clone(),
                    Binding {
                        emitted: emitted.clone(),
                        template: None,
                    },
                );
                Pattern::Binding(emitted)
            }
            Pattern::Constructor(name, fields) => Pattern::Constructor(
                name.clone(),
                fields
                    .iter()
                    .map(|field| self.pattern(field, scope))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
    fn expr(
        &mut self,
        e: &'a Expr,
        subst: &Substitution,
        scope: &Scope<'a>,
        expected: Option<&Type>,
    ) -> Result<Expr, String> {
        self.charge()?;
        let mut context = subst.clone();
        if let Some(expected) = expected {
            constrain(
                self.checked
                    .expression_type(e)
                    .ok_or("E-FUNCTION: missing expression type")?,
                expected,
                &mut context,
                0,
            )?;
        }
        let ty = self.ty(e, &context)?;
        let convert = |this: &mut Self, e: &'a Expr| this.expr(e, &context, scope, None);
        Ok(match e {
            Expr::Var(name) => self.binding_value(name, &ty, scope)?,
            Expr::Move(n) => Expr::Move(emitted_name(scope, n)),
            Expr::Inout(n) => Expr::Inout(emitted_name(scope, n)),
            Expr::InoutField(n, path) => Expr::InoutField(emitted_name(scope, n), path.clone()),
            Expr::Field(n, path) => Expr::Field(emitted_name(scope, n), path.clone()),
            Expr::Lambda(..) => {
                let info = self
                    .checked
                    .closure_info(e)
                    .ok_or("E-FUNCTION: missing lambda captures")?;
                let names = info.captures.clone();
                let Type::Callable { environment, .. } = &ty else {
                    return Err("E-FUNCTION: missing lambda type".into());
                };
                let Type::Nominal(_, types) = environment.as_ref() else {
                    return Err("E-FUNCTION: missing lambda environment".into());
                };
                let values = names
                    .iter()
                    .zip(types)
                    .map(|(n, t)| self.binding_value(n, t, scope))
                    .collect::<Result<_, _>>()?;
                self.record(&ty, values)?
            }
            Expr::Apply(callee, args) => {
                let callable = self.ty(callee, &context)?;
                let callee = convert(self, callee)?;
                let args = args
                    .iter()
                    .map(|a| convert(self, a))
                    .collect::<Result<_, _>>()?;
                self.apply_values(callee, &callable, args, &ty)?
            }
            Expr::Call(name, args)
                if scope.contains_key(name) && self.checked.application_type(e).is_some() =>
            {
                let callable = substitute(
                    self.checked
                        .application_type(e)
                        .ok_or("E-FUNCTION: missing checked application type")?,
                    &context,
                    0,
                )?;
                let callee = self.binding_value(name, &callable, scope)?;
                let args = args
                    .iter()
                    .map(|a| convert(self, a))
                    .collect::<Result<_, _>>()?;
                self.apply_values(callee, &callable, args, &ty)?
            }
            Expr::Call(name, args)
                if self
                    .checked
                    .program
                    .functions
                    .iter()
                    .any(|f| f.name == *name) =>
            {
                let natural = self
                    .checked
                    .program
                    .functions
                    .iter()
                    .find(|f| f.name == *name)
                    .ok_or("E-FUNCTION: missing direct declaration")?
                    .params
                    .len();
                let converted = args
                    .iter()
                    .map(|a| convert(self, a))
                    .collect::<Result<Vec<_>, _>>()?;
                if args.len() == natural {
                    let params = args
                        .iter()
                        .map(|a| self.ty(a, &context))
                        .collect::<Result<Vec<_>, _>>()?;
                    let target = self.source_instance(Instance {
                        source: name.clone(),
                        params,
                        result: ty.clone(),
                    })?;
                    Expr::Call(target, converted)
                } else {
                    let callable = substitute(
                        self.checked
                            .application_type(e)
                            .ok_or("E-FUNCTION: missing checked application type")?,
                        &context,
                        0,
                    )?;
                    let value = self.record(&callable, vec![])?;
                    self.apply_values(value, &callable, converted, &ty)?
                }
            }

            Expr::FunctionOp(op, left, right) => {
                let lty = self.ty(left, &context)?;
                let rty = self.ty(right, &context)?;
                let left = convert(self, left)?;
                let right = convert(self, right)?;
                match op {
                    FunctionOp::ComposeForward | FunctionOp::ComposeBackward => {
                        self.record(&ty, vec![left, right])?
                    }
                    FunctionOp::PipeBackward => self.apply_values(left, &lty, vec![right], &ty)?,
                    FunctionOp::PipeForward => {
                        let local = self.fresh("rill_pipe");
                        let call =
                            self.apply_values(right, &rty, vec![Expr::Var(local.clone())], &ty)?;
                        Expr::Let(local, Box::new(left), Box::new(call))
                    }
                }
            }
            Expr::Let(name, value, body) | Expr::LetMut(name, value, body) => {
                let value_ty = self.ty(value, &context)?;
                let mut inner = scope.clone();
                let template = self.checked.polymorphic_local(e).is_some()
                    && matches!(value_ty, Type::Callable { .. })
                    && ensure_concrete(&value_ty).is_err();
                let emitted = self.fresh("rill_local");
                inner.insert(
                    name.clone(),
                    Binding {
                        emitted: emitted.clone(),
                        template: template
                            .then(|| (value.as_ref(), context.clone(), Rc::new(scope.clone()))),
                    },
                );
                if template {
                    self.expr(body, &context, &inner, None)?
                } else {
                    let value = convert(self, value)?;
                    let body = self.expr(body, &context, &inner, None)?;
                    if matches!(e, Expr::Let(..)) {
                        Expr::Let(emitted, Box::new(value), Box::new(body))
                    } else {
                        Expr::LetMut(emitted, Box::new(value), Box::new(body))
                    }
                }
            }
            Expr::Binary(op, a, b) => Expr::Binary(
                *op,
                Box::new(convert(self, a)?),
                Box::new(convert(self, b)?),
            ),
            Expr::If(a, b, c) => Expr::If(
                Box::new(convert(self, a)?),
                Box::new(convert(self, b)?),
                Box::new(convert(self, c)?),
            ),
            Expr::While(a, b) => {
                Expr::While(Box::new(convert(self, a)?), Box::new(convert(self, b)?))
            }
            Expr::Assign(n, v) => Expr::Assign(emitted_name(scope, n), Box::new(convert(self, v)?)),
            Expr::FieldAssign(n, p, v) => Expr::FieldAssign(
                emitted_name(scope, n),
                p.clone(),
                Box::new(convert(self, v)?),
            ),
            Expr::Call(n, args) => Expr::Call(
                n.clone(),
                args.iter()
                    .map(|a| convert(self, a))
                    .collect::<Result<_, _>>()?,
            ),
            Expr::Record(n, fields) => Expr::Record(
                n.clone(),
                fields
                    .iter()
                    .map(|(n, v)| Ok((n.clone(), convert(self, v)?)))
                    .collect::<Result<_, String>>()?,
            ),
            Expr::Match(value, arms) => {
                let converted = convert(self, value)?;
                let mut converted_arms = vec![];
                for arm in arms {
                    let mut inner = scope.clone();
                    // Pattern locals shadow top-level function names; their exact
                    // application types are supplied by the checker at each call.
                    let pattern = self.pattern(&arm.pattern, &mut inner);
                    converted_arms.push(MatchArm {
                        pattern,
                        body: self.expr(&arm.body, &context, &inner, None)?,
                    });
                }
                Expr::Match(Box::new(converted), converted_arms)
            }
            _ => e.clone(),
        })
    }
}

struct Signature {
    tag: String,
    base: Type,
    captures: Vec<Type>,
    supplied: Vec<Type>,
    params: Vec<Type>,
    result: Type,
}
// Invocation signatures are logical type information, not environment storage.
// In particular, a generalized aggregate may instantiate the same captured
// closure at several signatures while retaining one physical aggregate value.
fn physical_type(ty: &Type, depth: usize) -> Result<Type, String> {
    if depth > 128 {
        return Err("E-FUNCTION-LIMIT: type nesting exceeds 128".into());
    }
    Ok(match ty {
        Type::Callable { environment, .. } => physical_type(environment, depth + 1)?,
        Type::Nominal(name, arguments) => Type::Nominal(
            name.clone(),
            arguments
                .iter()
                .map(|argument| physical_type(argument, depth + 1))
                .collect::<Result<_, _>>()?,
        ),
        other => other.clone(),
    })
}

fn physical_captures(environment: &Type) -> Result<Vec<Type>, String> {
    let Type::Nominal(tag, fields) = environment else {
        return Err("E-FUNCTION: unresolved environment".into());
    };
    if tag.starts_with("$curry:") {
        let mut captures =
            physical_captures(fields.first().ok_or("E-FUNCTION: missing curry base")?)?;
        captures.extend_from_slice(&fields[1..]);
        Ok(captures)
    } else {
        Ok(fields.clone())
    }
}

fn emitted_name(scope: &Scope<'_>, name: &str) -> String {
    scope
        .get(name)
        .map_or_else(|| name.to_string(), |binding| binding.emitted.clone())
}
fn children<'a>(e: &'a Expr, out: &mut Vec<&'a Expr>) {
    match e {
        Expr::Lambda(_, body) => out.push(body),
        Expr::Apply(callee, args) => {
            out.push(callee);
            out.extend(args);
        }
        Expr::Binary(_, a, b)
        | Expr::FunctionOp(_, a, b)
        | Expr::Let(_, a, b)
        | Expr::LetMut(_, a, b)
        | Expr::While(a, b) => {
            out.push(a);
            out.push(b);
        }
        Expr::If(a, b, c) => {
            out.push(a);
            out.push(b);
            out.push(c);
        }
        Expr::Assign(_, a) | Expr::FieldAssign(_, _, a) => out.push(a),
        Expr::Call(_, args) => out.extend(args),
        Expr::Record(_, fields) => out.extend(fields.iter().map(|(_, e)| e)),
        Expr::Match(e, arms) => {
            out.push(e);
            out.extend(arms.iter().map(|a| &a.body));
        }
        _ => {}
    }
}
fn substitute(ty: &Type, map: &Substitution, depth: usize) -> Result<Type, String> {
    if depth > 128 {
        return Err("E-FUNCTION-LIMIT: type nesting exceeds 128".into());
    }
    Ok(match ty {
        Type::Var(id) => map.get(id).cloned().unwrap_or_else(|| ty.clone()),
        Type::Nominal(n, args) => Type::Nominal(
            n.clone(),
            args.iter()
                .map(|a| substitute(a, map, depth + 1))
                .collect::<Result<_, _>>()?,
        ),
        Type::Callable {
            environment,
            params,
            modes,
            result,
        } => Type::Callable {
            environment: Box::new(substitute(environment, map, depth + 1)?),
            params: params
                .iter()
                .map(|a| substitute(a, map, depth + 1))
                .collect::<Result<_, _>>()?,
            modes: modes.clone(),
            result: Box::new(substitute(result, map, depth + 1)?),
        },
        _ => ty.clone(),
    })
}
// A callable signature may contain unused phantom variables. Only its capture
// fields occupy storage; executable parameters are checked when instantiated.
fn ensure_layout(ty: &Type) -> Result<(), String> {
    let mut pending = vec![(ty.clone(), 0)];
    while let Some((ty, depth)) = pending.pop() {
        if depth > 128 {
            return Err("E-FUNCTION-LIMIT: type nesting exceeds 128".into());
        }
        match ty {
            Type::Var(_) => {
                return Err("E-FUNCTION-LAYOUT: unresolved concrete storage type".into())
            }
            Type::Nominal(_, args) => pending.extend(args.into_iter().map(|t| (t, depth + 1))),
            Type::Callable { environment, .. } => pending.extend(
                physical_captures(&environment)?
                    .into_iter()
                    .map(|t| (t, depth + 1)),
            ),
            _ => {}
        }
    }
    Ok(())
}

fn ensure_concrete(ty: &Type) -> Result<(), String> {
    let mut pending = vec![(ty, 0)];
    while let Some((ty, depth)) = pending.pop() {
        if depth > 128 {
            return Err("E-FUNCTION-LIMIT: type nesting exceeds 128".into());
        }
        match ty {
            Type::Var(_) => {
                return Err("E-FUNCTION-LAYOUT: unresolved callable specialization".into())
            }
            Type::Nominal(_, args) => pending.extend(args.iter().map(|t| (t, depth + 1))),
            Type::Callable {
                environment,
                params,
                result,
                ..
            } => {
                pending.push((environment, depth + 1));
                pending.push((result, depth + 1));
                pending.extend(params.iter().map(|t| (t, depth + 1)));
            }
            _ => {}
        }
    }
    Ok(())
}
fn constrain(
    pattern: &Type,
    actual: &Type,
    map: &mut Substitution,
    depth: usize,
) -> Result<(), String> {
    if depth > 128 {
        return Err("E-FUNCTION-LIMIT: type nesting exceeds 128".into());
    }
    match (pattern, actual) {
        (Type::Var(id), actual) => {
            if let Some(previous) = map.get(id) {
                if previous != actual {
                    return Err("E-FUNCTION: inconsistent specialization".into());
                }
            } else {
                map.insert(*id, actual.clone());
            }
        }
        (Type::Nominal(a, aa), Type::Nominal(b, bb)) if a == b && aa.len() == bb.len() => {
            for (a, b) in aa.iter().zip(bb) {
                constrain(a, b, map, depth + 1)?;
            }
        }
        (
            Type::Callable {
                environment: a,
                params: aa,
                modes: am,
                result: ar,
            },
            Type::Callable {
                environment: b,
                params: bb,
                modes: bm,
                result: br,
            },
        ) if aa.len() == bb.len() && am == bm => {
            constrain(a, b, map, depth + 1)?;
            for (a, b) in aa.iter().zip(bb) {
                constrain(a, b, map, depth + 1)?;
            }
            constrain(ar, br, map, depth + 1)?;
        }
        (a, b) if a == b => {}
        _ => return Err("E-FUNCTION: inconsistent specialization".into()),
    }
    Ok(())
}
