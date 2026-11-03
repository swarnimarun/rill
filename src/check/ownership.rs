//! Affine validation for hosted owners and finite nominal enums.
//! Structured branch joins require equal ownership states; loops remain invariant.
use super::{CheckedProgram, Type};
use crate::ast::{Expr, ParamMode, Pattern};
use std::collections::{HashMap, HashSet};

#[derive(Clone, PartialEq, Eq)]
enum Binding {
    Other,
    Owner {
        live: bool,
        mutable: bool,
        can_move: bool,
    },
}
type Scope = HashMap<String, Binding>;

pub(super) fn verify(checked: &CheckedProgram<'_>) -> Result<(), String> {
    let program = checked.program;
    if !program.externs.is_empty() {
        return Err("E-OWNED: owner programs cannot declare foreign functions".into());
    }
    if !program.constants.is_empty()
        || !program.native.includes.is_empty()
        || checked.has_c_values()
    {
        return Err(
            "E-OWNED: owner programs cannot use foreign constants, headers or raw C values".into(),
        );
    }
    reject_recursive_calls(checked)?;
    for function in &program.functions {
        let sig = &checked.functions[&function.name];
        if !sig.quantified.is_empty() {
            return Err("E-OWNED: generic functions are unsupported in owner programs".into());
        }
        for (param, ty) in function.params.iter().zip(&sig.params) {
            if param.mode != ParamMode::Value
                && checked.type_is_owner(ty)?
                && matches!(ty, Type::Nominal(..))
            {
                return Err(
                    "E-OWNED: borrowed owner-bearing enum parameters are unsupported".into(),
                );
            }
            if checked.is_record_type(ty) && checked.type_is_owner(ty)? {
                return Err("E-OWNED: owner-bearing record parameters are unsupported".into());
            }
        }
        if function.name == "main" && checked.type_is_owner(&sig.result)? {
            return Err("E-OWNED: main cannot return an owner".into());
        }
        let mut scope = Scope::new();
        for (param, ty) in function.params.iter().zip(&sig.params) {
            let binding = if checked.type_is_owner(ty)? {
                Binding::Owner {
                    live: true,
                    mutable: param.mode == ParamMode::Inout,
                    can_move: param.mode == ParamMode::Value,
                }
            } else {
                Binding::Other
            };
            scope.insert(param.name.clone(), binding);
        }
        visit(checked, &function.body, &mut scope)?;
    }
    Ok(())
}

fn user_calls(expr: &Expr, out: &mut HashSet<String>) {
    match expr {
        Expr::Call(name, args) => {
            out.insert(name.clone());
            for arg in args {
                user_calls(arg, out)
            }
        }
        Expr::Binary(_, a, b) | Expr::Let(_, a, b) | Expr::LetMut(_, a, b) | Expr::While(a, b) => {
            user_calls(a, out);
            user_calls(b, out)
        }
        Expr::Assign(_, value) | Expr::FieldAssign(_, _, value) => user_calls(value, out),
        Expr::If(a, b, c) => {
            user_calls(a, out);
            user_calls(b, out);
            user_calls(c, out)
        }
        Expr::Match(value, arms) => {
            user_calls(value, out);
            for arm in arms {
                user_calls(&arm.body, out)
            }
        }
        Expr::Record(_, fields) => {
            for (_, value) in fields {
                user_calls(value, out)
            }
        }
        _ => {}
    }
}
fn reject_recursive_calls(checked: &CheckedProgram<'_>) -> Result<(), String> {
    let functions = &checked.program.functions;
    let names: HashMap<_, _> = functions
        .iter()
        .enumerate()
        .map(|(i, f)| (f.name.as_str(), i))
        .collect();
    let mut incoming = vec![0usize; functions.len()];
    let mut edges = vec![Vec::new(); functions.len()];
    for (i, f) in functions.iter().enumerate() {
        let mut calls = HashSet::new();
        user_calls(&f.body, &mut calls);
        for call in calls {
            if let Some(&j) = names.get(call.as_str()) {
                edges[i].push(j);
                incoming[j] += 1;
            }
        }
    }
    let mut ready: Vec<_> = incoming
        .iter()
        .enumerate()
        .filter_map(|(i, n)| (*n == 0).then_some(i))
        .collect();
    let mut count = 0;
    while let Some(i) = ready.pop() {
        count += 1;
        for &j in &edges[i] {
            incoming[j] -= 1;
            if incoming[j] == 0 {
                ready.push(j)
            }
        }
    }
    if count != functions.len() {
        return Err("E-OWNED: recursive user calls are unsupported in owner programs".into());
    }
    Ok(())
}
fn owner(scope: &Scope, name: &str) -> Result<(bool, bool, bool), String> {
    match scope.get(name) {
        Some(Binding::Owner {
            live,
            mutable,
            can_move,
            ..
        }) => Ok((*live, *mutable, *can_move)),
        _ => Err(format!("E-OWNED: '{name}' is not an owner local")),
    }
}
fn ensure_live(scope: &Scope, name: &str) -> Result<(bool, bool), String> {
    let (live, mutable, can_move) = owner(scope, name)?;
    if !live {
        return Err(format!("E-OWNED: owner '{name}' was already moved"));
    }
    Ok((mutable, can_move))
}
fn scoped<F: FnOnce(&mut Scope) -> Result<(), String>>(
    scope: &mut Scope,
    name: &str,
    binding: Binding,
    body: F,
) -> Result<(), String> {
    let previous = scope.insert(name.into(), binding);
    let result = body(scope);
    if let Some(old) = previous {
        scope.insert(name.into(), old);
    } else {
        scope.remove(name);
    }
    result
}
fn visit(checked: &CheckedProgram<'_>, expr: &Expr, scope: &mut Scope) -> Result<(), String> {
    if let Some(ty) = checked.expression_type(expr) {
        if checked.is_record_type(ty) && checked.type_is_owner(ty)? {
            return Err("E-OWNED: owner-bearing record values are unsupported".into());
        }
    }
    match expr {
        Expr::Let(name, value, body) | Expr::LetMut(name, value, body) => {
            let mutable = matches!(expr, Expr::LetMut(..));
            let is_owner_value = checked
                .expression_type(value)
                .map(|ty| checked.type_is_owner(ty))
                .transpose()?
                .unwrap_or(false);
            let binding = if is_owner_value {
                match value.as_ref() {
                    Expr::Call(n, args)
                        if matches!(
                            n.as_str(),
                            "system_box_i32"
                                | "system_bytes_zeroed"
                                | "system_box_i32_abort"
                                | "system_bytes_zeroed_abort"
                        ) && args.len() == 1 =>
                    {
                        visit(checked, &args[0], scope)?
                    }
                    Expr::Move(source) => {
                        if !ensure_live(scope, source)?.1 {
                            return Err(format!(
                                "E-OWNED: borrowed owner '{source}' cannot be moved"
                            ));
                        }
                        let Some(Binding::Owner { live, .. }) = scope.get_mut(source) else {
                            unreachable!()
                        };
                        *live = false;
                    }
                    _ => visit(checked, value, scope)?,
                }
                Binding::Owner {
                    live: true,
                    mutable,
                    can_move: true,
                }
            } else {
                visit(checked, value, scope)?;
                Binding::Other
            };
            scoped(scope, name, binding, |scope| visit(checked, body, scope))
        }
        Expr::Var(name) | Expr::Inout(name)
            if matches!(scope.get(name), Some(Binding::Owner { .. })) =>
        {
            Err(format!(
                "E-OWNED: owner '{name}' cannot be copied or passed to user functions"
            ))
        }
        Expr::Move(root) => {
            if !ensure_live(scope, root)?.1 {
                return Err(format!("E-OWNED: borrowed owner '{root}' cannot be moved"));
            }
            let Some(Binding::Owner { live, .. }) = scope.get_mut(root) else {
                unreachable!()
            };
            *live = false;
            Ok(())
        }
        Expr::Call(name, _)
            if matches!(
                name.as_str(),
                "system_box_i32"
                    | "system_bytes_zeroed"
                    | "system_box_i32_abort"
                    | "system_bytes_zeroed_abort"
            ) =>
        {
            Err("E-OWNED: allocation requires a direct owner let initializer".into())
        }
        Expr::Call(name, args)
            if matches!(
                name.as_str(),
                "box_read" | "owned_bytes_len" | "owned_bytes_get"
            ) =>
        {
            let Some(Expr::Var(root)) = args.first() else {
                return Err(format!("E-OWNED: {name} requires an owner local"));
            };
            ensure_live(scope, root)?;
            for arg in &args[1..] {
                visit(checked, arg, scope)?;
            }
            Ok(())
        }
        Expr::Call(name, args) if matches!(name.as_str(), "box_write" | "owned_bytes_set") => {
            let Some(Expr::Inout(root)) = args.first() else {
                return Err(format!("E-OWNED: {name} requires an inout owner local"));
            };
            if !ensure_live(scope, root)?.0 {
                return Err(format!("E-OWNED: owner '{root}' is immutable"));
            }
            for arg in &args[1..] {
                visit(checked, arg, scope)?;
            }
            Ok(())
        }
        Expr::Call(name, args) => {
            if let Some(ty @ Type::Nominal(..)) = checked.expression_type(expr) {
                if let Some(variant) = checked
                    .adt_variants(ty)?
                    .into_iter()
                    .find(|variant| variant.name == *name)
                {
                    if variant.fields.len() != args.len() {
                        return Err("E-OWNED: invalid enum constructor arity".into());
                    }
                    for arg in args {
                        visit(checked, arg, scope)?;
                    }
                    return Ok(());
                }
            }
            if matches!(
                name.as_str(),
                "bytes_new"
                    | "bytes_len"
                    | "bytes_get"
                    | "bytes_set"
                    | "byte_len"
                    | "byte_at"
                    | "u8x16_load"
                    | "u8x16_lane"
                    | "u8x16_and"
                    | "u8x16_or"
                    | "u8x16_xor"
                    | "u8x16_eq"
                    | "u8x16_any"
                    | "u8x16_all"
            ) {
                for arg in args {
                    visit(checked, arg, scope)?;
                }
                return Ok(());
            }
            let signature = checked
                .functions
                .get(name)
                .ok_or_else(|| format!("E-OWNED: unknown function '{name}'"))?;
            if !signature.quantified.is_empty() || signature.params.len() != args.len() {
                return Err("E-OWNED: generic or invalid owner call".into());
            }
            let function = checked
                .program
                .functions
                .iter()
                .find(|f| f.name == *name)
                .ok_or_else(|| format!("E-OWNED: unknown user function '{name}'"))?;
            for ((arg, ty), param) in args.iter().zip(&signature.params).zip(&function.params) {
                if checked.type_is_owner(ty)? {
                    match (param.mode, arg) {
                        (ParamMode::Value, Expr::Move(root)) => {
                            if !ensure_live(scope, root)?.1 { return Err(format!("E-OWNED: borrowed owner '{root}' cannot be moved")); }
                            let Some(Binding::Owner { live, .. }) = scope.get_mut(root) else { unreachable!() };
                            *live = false;
                        }
                        (ParamMode::Read, Expr::Var(root)) => { ensure_live(scope, root)?; }
                        (ParamMode::Inout, Expr::Inout(root)) => {
                            if !ensure_live(scope, root)?.0 { return Err(format!("E-OWNED: owner '{root}' is immutable")); }
                        }
                        _ => return Err("E-OWNED: owner argument must match its parameter mode and name a live root".into()),
                    }
                } else {
                    visit(checked, arg, scope)?;
                }
            }
            Ok(())
        }
        Expr::Assign(name, value) => {
            if matches!(scope.get(name), Some(Binding::Owner { .. })) {
                return Err("E-OWNED: owner replacement or reinitialization is unsupported".into());
            }
            visit(checked, value, scope)
        }
        Expr::If(condition, yes, no) => {
            visit(checked, condition, scope)?;
            let mut yes_scope = scope.clone();
            visit(checked, yes, &mut yes_scope)?;
            let mut no_scope = scope.clone();
            visit(checked, no, &mut no_scope)?;
            if yes_scope != no_scope {
                return Err("E-OWNED: branches disagree about enclosing ownership state".into());
            }
            *scope = yes_scope;
            Ok(())
        }
        Expr::While(condition, body) => {
            let before = scope.clone();
            visit(checked, condition, scope)?;
            visit(checked, body, scope)?;
            if *scope != before {
                return Err("E-OWNED: a loop must preserve enclosing ownership state".into());
            }
            Ok(())
        }
        Expr::Binary(op, a, b) => {
            visit(checked, a, scope)?;
            let before = scope.clone();
            visit(checked, b, scope)?;
            if matches!(op, crate::ast::BinaryOp::And | crate::ast::BinaryOp::Or)
                && *scope != before
            {
                return Err(
                    "E-OWNED: a short-circuit operand must preserve enclosing ownership state"
                        .into(),
                );
            }
            Ok(())
        }
        Expr::Match(scrutinee, arms) => {
            let ty = checked
                .expression_type(scrutinee)
                .ok_or("E-OWNED: missing match type")?;
            let owned = checked.type_is_owner(ty)?;
            let variants = if owned {
                checked.adt_variants(ty)?
            } else {
                Vec::new()
            };
            if owned && !matches!(scrutinee.as_ref(), Expr::Move(_)) {
                return Err("E-OWNED: an owner enum match must consume a direct moved root".into());
            }
            visit(checked, scrutinee, scope)?;
            let before = scope.clone();
            let mut seen = HashSet::new();
            let mut joined = None;
            for arm in arms {
                let mut arm_scope = before.clone();
                let mut shadows = Vec::new();
                let bindings: Vec<(&String, bool)> = if !owned {
                    // Type checking has established that every subpattern is
                    // Copy and the complete matrix is exhaustive. These arms
                    // borrow no owner and introduce only ordinary bindings.
                    let mut bindings = Vec::new();
                    let mut pending = vec![&arm.pattern];
                    while let Some(pattern) = pending.pop() {
                        match pattern {
                            Pattern::Binding(name) => bindings.push((name, false)),
                            Pattern::Constructor(_, fields) => pending.extend(fields.iter().rev()),
                            _ => {}
                        }
                    }
                    bindings
                } else {
                    match &arm.pattern {
                        Pattern::Constructor(name, patterns) => {
                            let variant = variants
                                .iter()
                                .find(|variant| variant.name == *name)
                                .ok_or("E-OWNED: pattern constructor belongs to another enum")?;
                            if !seen.insert(name.clone()) || patterns.len() != variant.fields.len()
                            {
                                return Err(
                                "E-OWNED: enum match constructor is duplicate or has wrong arity"
                                    .into(),
                            );
                            }
                            let mut bindings = Vec::new();
                            for (pattern, field_ty) in patterns.iter().zip(&variant.fields) {
                                match pattern {
                                Pattern::Wildcard => {},
                                Pattern::Binding(name) => bindings.push((name, checked.type_is_owner(field_ty)?)),
                                _ => return Err("E-OWNED: nested or literal enum field patterns are unsupported".into()),
                            }
                            }
                            bindings
                        }
                        Pattern::Binding(name) => {
                            if seen.len() == variants.len() {
                                return Err("E-OWNED: unreachable whole-value pattern".into());
                            }
                            seen.extend(variants.iter().map(|variant| variant.name.clone()));
                            vec![(name, checked.type_is_owner(ty)?)]
                        }
                        Pattern::Wildcard => {
                            if seen.len() == variants.len() {
                                return Err("E-OWNED: unreachable wildcard pattern".into());
                            }
                            seen.extend(variants.iter().map(|variant| variant.name.clone()));
                            Vec::new()
                        }
                        _ => return Err(
                            "E-OWNED: enum match requires constructor, binder or wildcard patterns"
                                .into(),
                        ),
                    }
                };
                for (name, owned) in bindings {
                    let binding = if owned {
                        Binding::Owner {
                            live: true,
                            mutable: false,
                            can_move: true,
                        }
                    } else {
                        Binding::Other
                    };
                    shadows.push((name, arm_scope.insert(name.clone(), binding)));
                }
                visit(checked, &arm.body, &mut arm_scope)?;
                for (name, previous) in shadows.into_iter().rev() {
                    if let Some(previous) = previous {
                        arm_scope.insert(name.clone(), previous);
                    } else {
                        arm_scope.remove(name);
                    }
                }
                if joined
                    .as_ref()
                    .is_some_and(|previous| *previous != arm_scope)
                {
                    return Err(
                        "E-OWNED: match arms disagree about enclosing ownership state".into(),
                    );
                }
                joined = Some(arm_scope);
            }
            if owned && seen.len() != variants.len() {
                return Err("E-OWNED: enum match must name every constructor exactly once".into());
            }
            *scope = joined.ok_or("E-OWNED: empty enum match")?;
            Ok(())
        }
        Expr::Record(_, fields) => {
            for (_, value) in fields {
                visit(checked, value, scope)?;
            }
            Ok(())
        }
        Expr::FieldAssign(_, _, value) => visit(checked, value, scope),
        Expr::Int(_)
        | Expr::Bool(_)
        | Expr::Unit
        | Expr::Str(_)
        | Expr::CStr(_)
        | Expr::Var(_)
        | Expr::Inout(_)
        | Expr::Field(_, _)
        | Expr::InoutField(_, _) => Ok(()),
    }
}
