//! Source-AST callable execution, independent of closure conversion and layouts.
use super::{eval, Env, Value};
use crate::ast::{Expr, Function, FunctionOp, Param, ParamMode, Pattern};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

#[derive(Clone, Debug, PartialEq)]
pub struct Closure {
    kind: ClosureKind,
    bound: Vec<Value>,
}

// Source expressions contain only reflexive scalar/string data. Their public
// AST currently derives PartialEq, so the closure supplies the matching marker.
impl Eq for Closure {}

#[derive(Clone, Debug, PartialEq)]
enum ClosureKind {
    Named(String),
    Lambda {
        params: Vec<Param>,
        body: Expr,
        captures: HashMap<String, Value>,
    },
    Compose(Box<Closure>, Box<Closure>),
}

impl Closure {
    pub(super) fn named(name: String) -> Value {
        Value::Callable(Box::new(Self {
            kind: ClosureKind::Named(name),
            bound: vec![],
        }))
    }
}

pub(super) struct Context<'a, 'p> {
    pub functions: &'a HashMap<&'p str, &'p Function>,
    pub constructors: &'a HashMap<&'p str, usize>,
    pub externs: &'a HashSet<&'p str>,
    pub records: &'a HashMap<&'p str, Vec<&'p str>>,
}

impl Context<'_, '_> {
    fn evaluate(&self, expr: &Expr, env: &mut Env, depth: usize) -> Result<Value, String> {
        eval(
            expr,
            env,
            self.functions,
            self.constructors,
            self.externs,
            self.records,
            depth,
        )
    }

    pub fn expression(&self, expr: &Expr, env: &mut Env, depth: usize) -> Result<Value, String> {
        match expr {
            Expr::Lambda(params, body) => {
                let names = free_names(
                    body,
                    params.iter().map(|p| p.name.clone()).collect(),
                    self.constructors,
                );
                let captures = names
                    .into_iter()
                    .filter_map(|name| env.get(&name).map(|value| (name, value.borrow().clone())))
                    .collect();
                Ok(Value::Callable(Box::new(Closure {
                    kind: ClosureKind::Lambda {
                        params: params.clone(),
                        body: *body.clone(),
                        captures,
                    },
                    bound: vec![],
                })))
            }
            Expr::Apply(callee, args) => {
                let value = self.evaluate(callee, env, depth)?;
                self.apply_expressions(value, args, env, depth)
            }
            Expr::FunctionOp(op, left, right) => {
                let left = self.evaluate(left, env, depth)?;
                let right = self.evaluate(right, env, depth)?;
                match op {
                    FunctionOp::PipeForward => self.apply_value(right, left, depth),
                    FunctionOp::PipeBackward => self.apply_value(left, right, depth),
                    FunctionOp::ComposeForward | FunctionOp::ComposeBackward => {
                        let (left, right) = (callable(left)?, callable(right)?);
                        let (first, second) = if *op == FunctionOp::ComposeForward {
                            (left, right)
                        } else {
                            (right, left)
                        };
                        Ok(Value::Callable(Box::new(Closure {
                            kind: ClosureKind::Compose(first, second),
                            bound: vec![],
                        })))
                    }
                }
            }
            _ => Err("E-REFERENCE: expected callable expression".into()),
        }
    }

    pub fn apply_expressions(
        &self,
        value: Value,
        args: &[Expr],
        env: &mut Env,
        depth: usize,
    ) -> Result<Value, String> {
        let mut closure = callable(value)?;
        if args.is_empty() {
            if self.arity(&closure)? != closure.bound.len() {
                return Err("E-ARITY: nonzero-arity function needs an argument".into());
            }
            return self.invoke(*closure, depth);
        }
        for (index, arg) in args.iter().enumerate() {
            if self.arity(&closure)? <= closure.bound.len() {
                return Err("E-ARITY: callable argument count differs".into());
            }
            closure.bound.push(self.evaluate(arg, env, depth)?);
            if closure.bound.len() == self.arity(&closure)? {
                let result = self.invoke(*closure, depth)?;
                if index + 1 == args.len() {
                    return Ok(result);
                }
                // Invoke the saturated function before touching the next
                // source operand, which may observe the body's writes/trap.
                closure = callable(result)?;
            }
        }
        Ok(Value::Callable(closure))
    }

    fn apply_value(&self, function: Value, argument: Value, depth: usize) -> Result<Value, String> {
        let mut closure = callable(function)?;
        if self.arity(&closure)? <= closure.bound.len() {
            return Err("E-ARITY: callable argument count differs".into());
        }
        closure.bound.push(argument);
        if closure.bound.len() == self.arity(&closure)? {
            self.invoke(*closure, depth)
        } else {
            Ok(Value::Callable(closure))
        }
    }

    fn arity(&self, closure: &Closure) -> Result<usize, String> {
        match &closure.kind {
            ClosureKind::Named(name) => self
                .functions
                .get(name.as_str())
                .map(|f| f.params.len())
                .ok_or_else(|| format!("unknown function: {name}")),
            ClosureKind::Lambda { params, .. } => Ok(params.len()),
            ClosureKind::Compose(..) => Ok(1),
        }
    }

    fn invoke(&self, closure: Closure, depth: usize) -> Result<Value, String> {
        if depth >= 32 {
            return Err("runtime trap".into());
        }
        match closure.kind {
            ClosureKind::Named(name) => {
                let function = self
                    .functions
                    .get(name.as_str())
                    .ok_or_else(|| format!("unknown function: {name}"))?;
                let mut env = bind(&function.params, closure.bound, HashMap::new())?;
                self.evaluate(&function.body, &mut env, depth + 1)
            }
            ClosureKind::Lambda {
                params,
                body,
                captures,
            } => {
                let mut env = bind(&params, closure.bound, captures)?;
                self.evaluate(&body, &mut env, depth + 1)
            }
            ClosureKind::Compose(first, second) => {
                let [argument]: [Value; 1] = closure
                    .bound
                    .try_into()
                    .map_err(|_| "E-ARITY: composition needs one argument")?;
                let value = self.apply_value(Value::Callable(first), argument, depth + 1)?;
                self.apply_value(Value::Callable(second), value, depth + 1)
            }
        }
    }
}

fn callable(value: Value) -> Result<Box<Closure>, String> {
    match value {
        Value::Callable(closure) => Ok(closure),
        _ => Err("E-TYPE: application requires a function".into()),
    }
}

fn bind(
    params: &[Param],
    values: Vec<Value>,
    captures: HashMap<String, Value>,
) -> Result<Env, String> {
    if params.len() != values.len() || params.iter().any(|p| p.mode != ParamMode::Value) {
        return Err("E-FUNCTION-MODE: first-class functions require value parameters".into());
    }
    let mut env: Env = captures
        .into_iter()
        .map(|(name, value)| (name, Rc::new(RefCell::new(value))))
        .collect();
    for (param, value) in params.iter().zip(values) {
        env.insert(param.name.clone(), Rc::new(RefCell::new(value)));
    }
    Ok(env)
}

fn free_names(
    expr: &Expr,
    bound: HashSet<String>,
    constructors: &HashMap<&str, usize>,
) -> HashSet<String> {
    fn pattern_names(pattern: &Pattern, bound: &mut HashSet<String>) {
        match pattern {
            Pattern::Binding(name) => {
                bound.insert(name.clone());
            }
            Pattern::Constructor(_, fields) => {
                for field in fields {
                    pattern_names(field, bound);
                }
            }
            _ => {}
        }
    }
    fn walk(
        expr: &Expr,
        bound: &HashSet<String>,
        free: &mut HashSet<String>,
        constructors: &HashMap<&str, usize>,
    ) {
        let mut name = |name: &str| {
            if !bound.contains(name) {
                free.insert(name.into());
            }
        };
        match expr {
            Expr::Var(n)
            | Expr::Move(n)
            | Expr::Inout(n)
            | Expr::Field(n, _)
            | Expr::InoutField(n, _) => name(n),
            Expr::Call(n, args) => {
                if !constructors.contains_key(n.as_str()) {
                    name(n);
                }
                for arg in args {
                    walk(arg, bound, free, constructors);
                }
            }
            Expr::Assign(n, value) | Expr::FieldAssign(n, _, value) => {
                name(n);
                walk(value, bound, free, constructors);
            }
            Expr::Let(n, value, body) | Expr::LetMut(n, value, body) => {
                walk(value, bound, free, constructors);
                let mut inner = bound.clone();
                inner.insert(n.clone());
                walk(body, &inner, free, constructors);
            }
            Expr::Lambda(params, body) => {
                let mut inner = bound.clone();
                inner.extend(params.iter().map(|p| p.name.clone()));
                walk(body, &inner, free, constructors);
            }
            Expr::Match(value, arms) => {
                walk(value, bound, free, constructors);
                for arm in arms {
                    let mut inner = bound.clone();
                    pattern_names(&arm.pattern, &mut inner);
                    walk(&arm.body, &inner, free, constructors);
                }
            }
            Expr::Binary(_, a, b) | Expr::FunctionOp(_, a, b) | Expr::While(a, b) => {
                walk(a, bound, free, constructors);
                walk(b, bound, free, constructors);
            }
            Expr::If(c, a, b) => {
                walk(c, bound, free, constructors);
                walk(a, bound, free, constructors);
                walk(b, bound, free, constructors);
            }
            Expr::Apply(callee, args) => {
                walk(callee, bound, free, constructors);
                for arg in args {
                    walk(arg, bound, free, constructors);
                }
            }
            Expr::Record(_, fields) => {
                for (_, value) in fields {
                    walk(value, bound, free, constructors);
                }
            }
            Expr::Int(_) | Expr::Bool(_) | Expr::Unit | Expr::Str(_) | Expr::CStr(_) => {}
        }
    }
    let mut free = HashSet::new();
    walk(expr, &bound, &mut free, constructors);
    free
}
