//! Reference evaluation for the sequential kernel.
use crate::ast::{BinaryOp, Expr, ParamMode, Pattern, Program};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::Rc;

type Env = HashMap<String, Rc<RefCell<Value>>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Int(i32),
    Bool(bool),
    Unit,
    Str(String),
    Bytes(Vec<u8>),
    U8x16([u8; 16]),
    Record(String, Vec<(String, Value)>),
    Adt(String, Vec<Value>),
}
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(n) => write!(f, "{n}"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Unit => write!(f, "()"),
            Self::Str(value) => write!(f, "{value}"),
            Self::Bytes(value) => {
                write!(f, "bytes[")?;
                for (i, byte) in value.iter().enumerate() {
                    if i != 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{byte}")?;
                }
                write!(f, "]")
            }
            Self::U8x16(lanes) => {
                write!(f, "u8x16[")?;
                for (i, lane) in lanes.iter().enumerate() {
                    if i != 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{lane}")?;
                }
                write!(f, "]")
            }
            Self::Record(name, fields) => {
                write!(f, "{} {{", crate::display::constructor_name(name))?;
                for (index, (field, value)) in fields.iter().enumerate() {
                    if index == 0 {
                        write!(f, " ")?;
                    } else {
                        write!(f, "; ")?;
                    }
                    write!(f, "{field} = {value}")?;
                }
                if !fields.is_empty() {
                    write!(f, " ")?;
                }
                write!(f, "}}")
            }
            Self::Adt(name, fields) => {
                write!(f, "{}", crate::display::constructor_name(name))?;
                if !fields.is_empty() {
                    write!(f, "(")?;
                    for (i, field) in fields.iter().enumerate() {
                        if i != 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{field}")?;
                    }
                    write!(f, ")")?;
                }
                Ok(())
            }
        }
    }
}

pub fn run(program: &Program) -> Result<Value, String> {
    let checked = crate::check::analyze(program)?;
    match crate::typed::lower_main(&checked) {
        Ok(typed) => crate::typed::evaluate(&typed),
        Err(error) if error.starts_with("E-TYPED-UNSUPPORTED:") && !checked.has_owned() => {
            run_checked_reference(program)
        }
        Err(error) => Err(error),
    }
}

/// Independently evaluate the checked Copy subset from the source AST.
/// This is a differential oracle during migration to the shared typed path;
/// owner programs must never execute through a copying reference evaluator.
pub fn run_reference(program: &Program) -> Result<Value, String> {
    let checked = crate::check::analyze(program)?;
    if checked.has_owned() {
        return Err("E-REFERENCE: owner programs require the checked typed path".into());
    }
    run_checked_reference(program)
}

fn run_checked_reference(program: &Program) -> Result<Value, String> {
    let functions: HashMap<_, _> = program
        .functions
        .iter()
        .map(|f| (f.name.as_str(), f))
        .collect();
    let main = functions.get("main").ok_or("missing main function")?;
    if !main.params.is_empty() {
        return Err("main must have no parameters".into());
    }
    let constructors: HashMap<_, _> = program
        .types
        .iter()
        .flat_map(|ty| &ty.variants)
        .map(|variant| (variant.name.as_str(), variant.fields.len()))
        .collect();
    let externs: HashSet<_> = program
        .externs
        .iter()
        .map(|f| f.name.as_str())
        .chain(program.constants.iter().map(|c| c.name.as_str()))
        .collect();
    let records: HashMap<_, Vec<_>> = program
        .types
        .iter()
        .filter_map(|ty| {
            ty.record_fields.as_ref().map(|fields| {
                (
                    ty.name.as_str(),
                    fields.iter().map(String::as_str).collect(),
                )
            })
        })
        .collect();
    eval(
        &main.body,
        &mut HashMap::new(),
        &functions,
        &constructors,
        &externs,
        &records,
        1,
    )
}

fn eval(
    expr: &Expr,
    env: &mut Env,
    functions: &HashMap<&str, &crate::ast::Function>,
    constructors: &HashMap<&str, usize>,
    externs: &HashSet<&str>,
    records: &HashMap<&str, Vec<&str>>,
    call_depth: usize,
) -> Result<Value, String> {
    match expr {
        Expr::Int(n) => Ok(Value::Int(*n)),
        Expr::Bool(b) => Ok(Value::Bool(*b)),
        Expr::Unit => Ok(Value::Unit),
        Expr::Str(value) => Ok(Value::Str(value.clone())),
        Expr::CStr(_) => {
            Err("E-INTERP-FFI: cstr literals are only valid as direct C arguments".into())
        }
        Expr::Move(_) => Err("E-OWNER: move requires checked ownership lowering".into()),
        Expr::Inout(_) | Expr::InoutField(_, _) => {
            Err("E-LOAN: inout argument is only valid at a call".into())
        }
        Expr::Record(..) | Expr::Field(..) | Expr::FieldAssign(..) => eval_record(
            expr,
            env,
            functions,
            constructors,
            externs,
            records,
            call_depth,
        ),
        Expr::Var(name) => {
            if let Some(value) = env.get(name) {
                Ok(value.borrow().clone())
            } else if externs.contains(name.as_str()) {
                Err("E-FFI: imported C constants require native C emission".into())
            } else if constructors.get(name.as_str()) == Some(&0) {
                Ok(Value::Adt(name.clone(), Vec::new()))
            } else {
                Err(format!("unknown variable: {name}"))
            }
        }
        Expr::Let(name, value, body) | Expr::LetMut(name, value, body) => {
            let v = eval(
                value,
                env,
                functions,
                constructors,
                externs,
                records,
                call_depth,
            )?;
            let old = env.insert(name.clone(), Rc::new(RefCell::new(v)));
            let result = eval(
                body,
                env,
                functions,
                constructors,
                externs,
                records,
                call_depth,
            );
            if let Some(old) = old {
                env.insert(name.clone(), old);
            } else {
                env.remove(name);
            }
            result
        }
        Expr::If(condition, yes, no) => {
            match eval(
                condition,
                env,
                functions,
                constructors,
                externs,
                records,
                call_depth,
            )? {
                Value::Bool(true) => eval(
                    yes,
                    env,
                    functions,
                    constructors,
                    externs,
                    records,
                    call_depth,
                ),
                Value::Bool(false) => eval(
                    no,
                    env,
                    functions,
                    constructors,
                    externs,
                    records,
                    call_depth,
                ),
                _ => Err("runtime trap".into()),
            }
        }
        Expr::Assign(name, value) => {
            let new_value = eval(
                value,
                env,
                functions,
                constructors,
                externs,
                records,
                call_depth,
            )?;
            let target = env
                .get_mut(name)
                .ok_or_else(|| format!("unknown variable: {name}"))?;
            *target.borrow_mut() = new_value;
            Ok(Value::Unit)
        }
        Expr::While(condition, body) => {
            loop {
                match eval(
                    condition,
                    env,
                    functions,
                    constructors,
                    externs,
                    records,
                    call_depth,
                )? {
                    Value::Bool(false) => break,
                    Value::Bool(true) => {
                        if eval(
                            body,
                            env,
                            functions,
                            constructors,
                            externs,
                            records,
                            call_depth,
                        )? != Value::Unit
                        {
                            return Err("runtime trap".into());
                        }
                    }
                    _ => return Err("runtime trap".into()),
                }
            }
            Ok(Value::Unit)
        }
        Expr::Call(..) => eval_call(
            expr,
            env,
            functions,
            constructors,
            externs,
            records,
            call_depth,
        ),
        Expr::Match(scrutinee, arms) => {
            let value = eval(
                scrutinee,
                env,
                functions,
                constructors,
                externs,
                records,
                call_depth,
            )?;
            for arm in arms {
                let mut bindings = HashMap::new();
                if matches_pattern(&arm.pattern, &value, &mut bindings) {
                    let mut previous = Vec::new();
                    for (name, value) in bindings {
                        let old = env.insert(name.clone(), Rc::new(RefCell::new(value)));
                        previous.push((name, old));
                    }
                    let result = eval(
                        &arm.body,
                        env,
                        functions,
                        constructors,
                        externs,
                        records,
                        call_depth,
                    );
                    for (name, old) in previous {
                        if let Some(value) = old {
                            env.insert(name, value);
                        } else {
                            env.remove(&name);
                        }
                    }
                    return result;
                }
            }
            Err("runtime trap".into())
        }
        Expr::Binary(op, left, right) => {
            let l = eval(
                left,
                env,
                functions,
                constructors,
                externs,
                records,
                call_depth,
            )?;
            if matches!(op, BinaryOp::And) && l == Value::Bool(false) {
                return Ok(l);
            }
            if matches!(op, BinaryOp::Or) && l == Value::Bool(true) {
                return Ok(l);
            }
            let r = eval(
                right,
                env,
                functions,
                constructors,
                externs,
                records,
                call_depth,
            )?;
            binary(op, l, r)
        }
    }
}

pub(crate) fn binary(op: &BinaryOp, l: Value, r: Value) -> Result<Value, String> {
    use BinaryOp::*;
    let trap = || "runtime trap".to_string();
    match (op, l, r) {
        (Add, Value::Int(a), Value::Int(b)) => a.checked_add(b).map(Value::Int).ok_or_else(trap),
        (Sub, Value::Int(a), Value::Int(b)) => a.checked_sub(b).map(Value::Int).ok_or_else(trap),
        (Mul, Value::Int(a), Value::Int(b)) => a.checked_mul(b).map(Value::Int).ok_or_else(trap),
        (Div, Value::Int(a), Value::Int(b)) => a.checked_div(b).map(Value::Int).ok_or_else(trap),
        (Rem, Value::Int(a), Value::Int(b)) => a.checked_rem(b).map(Value::Int).ok_or_else(trap),
        (Eq, a, b) if std::mem::discriminant(&a) == std::mem::discriminant(&b) => {
            Ok(Value::Bool(a == b))
        }
        (Ne, a, b) if std::mem::discriminant(&a) == std::mem::discriminant(&b) => {
            Ok(Value::Bool(a != b))
        }
        (Lt, Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a < b)),
        (Le, Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a <= b)),
        (Gt, Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a > b)),
        (Ge, Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a >= b)),
        (And, Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(a && b)),
        (Or, Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(a || b)),
        _ => Err(trap()),
    }
}

fn matches_pattern(
    pattern: &Pattern,
    value: &Value,
    bindings: &mut HashMap<String, Value>,
) -> bool {
    match (pattern, value) {
        (Pattern::Wildcard, _) => true,
        (Pattern::Binding(name), value) => {
            bindings.insert(name.clone(), value.clone());
            true
        }
        (Pattern::Constructor(name, patterns), Value::Adt(actual, fields)) => {
            name == actual
                && patterns.len() == fields.len()
                && patterns
                    .iter()
                    .zip(fields)
                    .all(|(pattern, field)| matches_pattern(pattern, field, bindings))
        }
        (Pattern::Bool(a), Value::Bool(b)) => a == b,
        (Pattern::Int(a), Value::Int(b)) => a == b,
        (Pattern::Unit, Value::Unit) => true,
        _ => false,
    }
}

pub(crate) fn bytes_builtin(name: &str, values: &[Value]) -> Result<Value, String> {
    let trap = || "runtime trap".to_string();
    match (name, values) {
        ("bytes_new", [Value::Int(n)]) if (0..=256).contains(n) => {
            Ok(Value::Bytes(vec![0; *n as usize]))
        }
        ("bytes_len", [Value::Bytes(buf)]) => Ok(Value::Int(buf.len() as i32)),
        ("bytes_get", [Value::Bytes(buf), Value::Int(i)]) => usize::try_from(*i)
            .ok()
            .and_then(|i| buf.get(i))
            .map(|b| Value::Int(i32::from(*b)))
            .ok_or_else(trap),
        ("bytes_set", [Value::Bytes(buf), Value::Int(i), Value::Int(byte)]) => {
            let index = usize::try_from(*i).map_err(|_| trap())?;
            let byte = u8::try_from(*byte).map_err(|_| trap())?;
            let mut result = buf.clone();
            *result.get_mut(index).ok_or_else(trap)? = byte;
            Ok(Value::Bytes(result))
        }
        _ => Err(trap()),
    }
}

#[inline(never)]
fn eval_call(
    expr: &Expr,
    env: &mut Env,
    functions: &HashMap<&str, &crate::ast::Function>,
    constructors: &HashMap<&str, usize>,
    externs: &HashSet<&str>,
    records: &HashMap<&str, Vec<&str>>,
    call_depth: usize,
) -> Result<Value, String> {
    let Expr::Call(name, args) = expr else {
        unreachable!()
    };
    if matches!(
        name.as_str(),
        "u8" | "u32" | "u64" | "f32" | "c_null" | "is_null"
    ) {
        return Err("E-FFI: raw C values require native C emission".into());
    }
    if is_vector_builtin(name) {
        let expected = match name.as_str() {
            "u8x16_any" | "u8x16_all" => 1,
            _ => 2,
        };
        if args.len() != expected {
            return Err(format!("wrong number of arguments to {name}"));
        }
        let mut values = Vec::new();
        for arg in args {
            values.push(eval(
                arg,
                env,
                functions,
                constructors,
                externs,
                records,
                call_depth,
            )?);
        }
        return vector_builtin(name, &values);
    }
    if ["bytes_new", "bytes_len", "bytes_get", "bytes_set"].contains(&name.as_str()) {
        let expected = match name.as_str() {
            "bytes_new" | "bytes_len" => 1,
            "bytes_get" => 2,
            _ => 3,
        };
        if args.len() != expected {
            return Err(format!("wrong number of arguments to {name}"));
        }
        let mut values = Vec::new();
        for arg in args {
            values.push(eval(
                arg,
                env,
                functions,
                constructors,
                externs,
                records,
                call_depth,
            )?);
        }
        return bytes_builtin(name, &values);
    }
    if name == "byte_len" || name == "byte_at" {
        let expected = if name == "byte_len" { 1 } else { 2 };
        if args.len() != expected {
            return Err(format!("wrong number of arguments to {name}"));
        }
        let value = eval(
            &args[0],
            env,
            functions,
            constructors,
            externs,
            records,
            call_depth,
        )?;
        let Value::Str(value) = value else {
            return Err("runtime trap".into());
        };
        if name == "byte_len" {
            return i32::try_from(value.len())
                .map(Value::Int)
                .map_err(|_| "runtime trap".into());
        }
        let index = eval(
            &args[1],
            env,
            functions,
            constructors,
            externs,
            records,
            call_depth,
        )?;
        let Value::Int(index) = index else {
            return Err("runtime trap".into());
        };
        return usize::try_from(index)
            .ok()
            .and_then(|index| value.as_bytes().get(index))
            .map(|byte| Value::Int(i32::from(*byte)))
            .ok_or_else(|| "runtime trap".into());
    }
    if externs.contains(name.as_str()) {
        for arg in args {
            if !matches!(arg, Expr::CStr(_)) {
                eval(
                    arg,
                    env,
                    functions,
                    constructors,
                    externs,
                    records,
                    call_depth,
                )?;
            }
        }
        return Err(format!(
            "E-INTERP-FFI: cannot call C function {name} in the interpreter"
        ));
    }
    if let Some(&arity) = constructors.get(name.as_str()) {
        if args.len() != arity {
            return Err(format!("wrong number of arguments to {name}"));
        }
        let mut fields = Vec::with_capacity(arity);
        for arg in args {
            fields.push(eval(
                arg,
                env,
                functions,
                constructors,
                externs,
                records,
                call_depth,
            )?);
        }
        return Ok(Value::Adt(name.clone(), fields));
    }
    let fun = functions
        .get(name.as_str())
        .ok_or_else(|| format!("unknown function: {name}"))?;
    if fun.params.len() != args.len() {
        return Err(format!("wrong number of arguments to {name}"));
    }
    let mut call_env = HashMap::new();
    let mut writebacks = Vec::new();
    for (param, arg) in fun.params.iter().zip(args) {
        let value = match (param.mode, arg) {
            (ParamMode::Read, Expr::Var(name)) if env.contains_key(name) => env[name].clone(),
            (ParamMode::Read, _) => Rc::new(RefCell::new(eval(
                arg,
                env,
                functions,
                constructors,
                externs,
                records,
                call_depth,
            )?)),
            (ParamMode::Inout, Expr::InoutField(root, field)) => {
                let root = env.get(root).cloned().ok_or("E-RECORD: missing root")?;
                let value = {
                    let borrowed = root.borrow();
                    record_get(&borrowed, field)?.clone()
                };
                let temporary = Rc::new(RefCell::new(value));
                writebacks.push((root, field.clone(), temporary.clone()));
                temporary
            }
            (ParamMode::Inout, Expr::Inout(name)) => env
                .get(name)
                .cloned()
                .ok_or_else(|| format!("unknown variable: {name}"))?,
            (ParamMode::Inout, _) => return Err("E-LOAN: expected inout argument".into()),
            (ParamMode::Value, _) => Rc::new(RefCell::new(eval(
                arg,
                env,
                functions,
                constructors,
                externs,
                records,
                call_depth,
            )?)),
        };
        call_env.insert(param.name.clone(), value);
    }
    if call_depth >= 32 {
        return Err("runtime trap".into());
    }
    let result = eval(
        &fun.body,
        &mut call_env,
        functions,
        constructors,
        externs,
        records,
        call_depth + 1,
    )?;
    for (root, field, temporary) in writebacks {
        record_set(&mut root.borrow_mut(), &field, temporary.borrow().clone())?;
    }
    Ok(result)
}

#[inline(never)]
fn eval_record(
    expr: &Expr,
    env: &mut Env,
    functions: &HashMap<&str, &crate::ast::Function>,
    constructors: &HashMap<&str, usize>,
    externs: &HashSet<&str>,
    records: &HashMap<&str, Vec<&str>>,
    call_depth: usize,
) -> Result<Value, String> {
    match expr {
        Expr::Record(name, values) => {
            let order = records
                .get(name.as_str())
                .ok_or_else(|| format!("unknown record: {name}"))?;
            let mut fields = HashMap::new();
            for (field, value) in values {
                fields.insert(
                    field.as_str(),
                    eval(
                        value,
                        env,
                        functions,
                        constructors,
                        externs,
                        records,
                        call_depth,
                    )?,
                );
            }
            let ordered = order
                .iter()
                .map(|field| {
                    fields
                        .remove(field)
                        .map(|value| ((*field).to_string(), value))
                        .ok_or_else(|| format!("missing field: {field}"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Value::Record(name.clone(), ordered))
        }
        Expr::Field(root, field) => {
            let root = env.get(root).ok_or("E-RECORD: missing local")?.borrow();
            Ok(record_get(&root, field)?.clone())
        }
        Expr::FieldAssign(root, field, value) => {
            let value = eval(
                value,
                env,
                functions,
                constructors,
                externs,
                records,
                call_depth,
            )?;
            let mut root = env.get(root).ok_or("E-RECORD: missing local")?.borrow_mut();
            record_set(&mut root, field, value)?;
            Ok(Value::Unit)
        }
        _ => unreachable!(),
    }
}

fn record_get<'a>(mut value: &'a Value, path: &[String]) -> Result<&'a Value, String> {
    if path.is_empty() {
        return Err("E-RECORD: empty field path".into());
    }
    for field in path {
        let Value::Record(_, fields) = value else {
            return Err("E-RECORD: expected record".into());
        };
        value = &fields
            .iter()
            .find(|(name, _)| name == field)
            .ok_or("E-RECORD: missing field")?
            .1;
    }
    Ok(value)
}

fn record_set(value: &mut Value, path: &[String], replacement: Value) -> Result<(), String> {
    let (last, parents) = path.split_last().ok_or("E-RECORD: empty field path")?;
    let mut current = value;
    for field in parents {
        let Value::Record(_, fields) = current else {
            return Err("E-RECORD: expected record".into());
        };
        current = &mut fields
            .iter_mut()
            .find(|(name, _)| name == field)
            .ok_or("E-RECORD: missing field")?
            .1;
    }
    let Value::Record(_, fields) = current else {
        return Err("E-RECORD: expected record".into());
    };
    fields
        .iter_mut()
        .find(|(name, _)| name == last)
        .ok_or("E-RECORD: missing field")?
        .1 = replacement;
    Ok(())
}

fn is_vector_builtin(name: &str) -> bool {
    matches!(
        name,
        "u8x16_load"
            | "u8x16_lane"
            | "u8x16_and"
            | "u8x16_or"
            | "u8x16_xor"
            | "u8x16_eq"
            | "u8x16_any"
            | "u8x16_all"
    )
}

fn vector_builtin(name: &str, values: &[Value]) -> Result<Value, String> {
    let trap = || "runtime trap".to_string();
    match (name, values) {
        ("u8x16_load", [Value::Bytes(bytes), Value::Int(offset)]) => {
            let start = usize::try_from(*offset).map_err(|_| trap())?;
            let end = start.checked_add(16).ok_or_else(trap)?;
            let source = bytes.get(start..end).ok_or_else(trap)?;
            let mut lanes = [0; 16];
            lanes.copy_from_slice(source);
            Ok(Value::U8x16(lanes))
        }
        ("u8x16_lane", [Value::U8x16(lanes), Value::Int(index)]) => usize::try_from(*index)
            .ok()
            .and_then(|i| lanes.get(i))
            .map(|lane| Value::Int(i32::from(*lane)))
            .ok_or_else(trap),
        ("u8x16_any", [Value::U8x16(lanes)]) => Ok(Value::Bool(lanes.iter().any(|x| *x != 0))),
        ("u8x16_all", [Value::U8x16(lanes)]) => Ok(Value::Bool(lanes.iter().all(|x| *x != 0))),
        (
            op @ ("u8x16_and" | "u8x16_or" | "u8x16_xor" | "u8x16_eq"),
            [Value::U8x16(a), Value::U8x16(b)],
        ) => {
            let mut lanes = [0; 16];
            for i in 0..16 {
                lanes[i] = match op {
                    "u8x16_and" => a[i] & b[i],
                    "u8x16_or" => a[i] | b[i],
                    "u8x16_xor" => a[i] ^ b[i],
                    "u8x16_eq" => {
                        if a[i] == b[i] {
                            255
                        } else {
                            0
                        }
                    }
                    _ => unreachable!(),
                };
            }
            Ok(Value::U8x16(lanes))
        }
        _ => Err(trap()),
    }
}
