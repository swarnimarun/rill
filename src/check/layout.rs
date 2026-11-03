//! Reachable monomorphic instances used to bound the C backend's inline storage.
use super::{CheckedProgram, FunctionType, Type};
use crate::ast::{Expr, Program, TypeExpr};
use std::collections::{HashMap, HashSet};

const MAX_NODES: usize = 4096;
const MAX_INSTANCES: usize = 1024;
const MAX_DEPTH: usize = 128;
type Substitution = HashMap<usize, Type>;

fn substitute(ty: &Type, context: &Substitution) -> Result<Type, String> {
    match ty {
        Type::Var(id) => context.get(id).cloned().ok_or_else(|| {
            "E-C-ADT: unresolved polymorphic value requires local specialization".into()
        }),
        Type::Nominal(name, args) => Ok(Type::Nominal(
            name.clone(),
            args.iter()
                .map(|t| substitute(t, context))
                .collect::<Result<_, _>>()?,
        )),
        t => Ok(t.clone()),
    }
}

fn match_type(
    pattern: &Type,
    actual: &Type,
    quantified: &HashSet<usize>,
    mapping: &mut Substitution,
) -> Result<(), String> {
    match (pattern, actual) {
        (Type::Var(id), actual) if quantified.contains(id) => {
            if let Some(previous) = mapping.get(id) {
                if previous != actual {
                    return Err("E-C-ADT: inconsistent function specialization".into());
                }
            } else {
                mapping.insert(*id, actual.clone());
            }
            Ok(())
        }
        (Type::Nominal(a, aa), Type::Nominal(b, bb)) if a == b && aa.len() == bb.len() => {
            for (a, b) in aa.iter().zip(bb) {
                match_type(a, b, quantified, mapping)?;
            }
            Ok(())
        }
        (a, b) if a == b => Ok(()),
        _ => Err("E-C-ADT: inconsistent function specialization".into()),
    }
}

fn lower(t: &TypeExpr, vars: &HashMap<String, Type>) -> Type {
    if let Some(ty) = vars.get(&t.name) {
        return ty.clone();
    }
    match t.name.as_str() {
        "u8" => Type::CU8,
        "u32" => Type::CU32,
        "u64" => Type::CU64,
        "f32" => Type::CF32,
        "ptr" | "const_ptr" => Type::CPointer {
            pointee: t.args[0].name.clone(),
            is_const: t.name == "const_ptr",
        },
        "i32" => Type::Int,
        "bool" => Type::Bool,
        "unit" => Type::Unit,
        "str" => Type::StaticStr,
        "u8x16" => Type::U8x16,
        _ => Type::Nominal(
            t.name.clone(),
            t.args.iter().map(|a| lower(a, vars)).collect(),
        ),
    }
}

fn size(ty: &Type, program: &Program, depth: usize) -> Result<usize, String> {
    if depth > MAX_DEPTH {
        return Err("E-C-ADT: inline layout exceeds nesting limit".into());
    }
    match ty {
        Type::Var(_) => {
            Err("E-C-ADT: unresolved polymorphic value requires local specialization".into())
        }
        Type::Int
        | Type::Bool
        | Type::Unit
        | Type::CStr
        | Type::StaticStr
        | Type::CU8
        | Type::CU32
        | Type::CU64
        | Type::CF32
        | Type::CPointer { .. } => Ok(1),
        // The initial C representation reserves 256 inline bytes in 64 nodes.
        Type::Bytes(_) => Ok(65),
        Type::U8x16 => Ok(5),
        Type::SystemBoxI32 | Type::SystemBytes => {
            Err("E-C-OWNED: owner requires ownership-aware lowering".into())
        }
        Type::Nominal(name, args) => {
            let decl = program
                .types
                .iter()
                .find(|d| d.name == *name)
                .ok_or_else(|| format!("E-C-ADT: unknown type '{name}'"))?;
            // Resolve even phantom arguments. Layout must not depend on guessing a type.
            for arg in args {
                ensure_concrete(arg)?;
            }
            let params: HashMap<_, _> = decl
                .params
                .iter()
                .cloned()
                .zip(args.iter().cloned())
                .collect();
            let mut max = 1;
            for variant in &decl.variants {
                let mut nodes = 1usize;
                for field in &variant.fields {
                    nodes = nodes
                        .checked_add(size(&lower(field, &params), program, depth + 1)?)
                        .ok_or("E-C-ADT: inline layout exceeds compiler limit")?;
                    if nodes > MAX_NODES {
                        return Err("E-C-ADT: inline layout exceeds compiler limit".into());
                    }
                }
                max = max.max(nodes);
            }
            Ok(max)
        }
    }
}
fn ensure_concrete(ty: &Type) -> Result<(), String> {
    match ty {
        Type::Var(_) => {
            Err("E-C-ADT: unresolved polymorphic value requires local specialization".into())
        }
        Type::Nominal(_, args) => {
            for a in args {
                ensure_concrete(a)?
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

struct Analyzer<'a> {
    checked: &'a CheckedProgram<'a>,
    visited: HashSet<(String, Vec<Type>, Type)>,
    max: usize,
}
impl Analyzer<'_> {
    fn expr_type(&self, expr: &Expr, context: &Substitution) -> Result<Type, String> {
        let ty = self
            .checked
            .expression_type(expr)
            .ok_or("E-C-ADT: missing inferred expression type")?;
        substitute(ty, context)
    }
    fn instance(&mut self, name: &str, args: Vec<Type>, result: Type) -> Result<(), String> {
        for ty in args.iter().chain(std::iter::once(&result)) {
            self.max = self.max.max(size(ty, self.checked.program, 0)?);
        }
        let key = (name.to_string(), args.clone(), result.clone());
        if self.visited.contains(&key) {
            return Ok(());
        }
        if self.visited.len() >= MAX_INSTANCES {
            return Err("E-C-ADT: specialization limit exceeded".into());
        }
        self.visited.insert(key);
        let Some(function) = self
            .checked
            .program
            .functions
            .iter()
            .find(|f| f.name == name)
        else {
            return Ok(());
        }; // foreign functions have no body
        let FunctionType {
            quantified,
            params,
            result: result_type,
        } = &self.checked.functions[name];
        let vars: HashSet<_> = quantified.iter().copied().collect();
        let mut mapping = Substitution::new();
        for (p, a) in params.iter().zip(args.iter()) {
            match_type(p, a, &vars, &mut mapping)?;
        }
        match_type(result_type, &result, &vars, &mut mapping)?;
        self.expr(&function.body, &mapping)
    }
    fn expr(&mut self, expr: &Expr, context: &Substitution) -> Result<(), String> {
        let ty = self.expr_type(expr, context)?;
        self.max = self.max.max(size(&ty, self.checked.program, 0)?);
        match expr {
            Expr::Binary(_, a, b)
            | Expr::Let(_, a, b)
            | Expr::LetMut(_, a, b)
            | Expr::While(a, b) => {
                self.expr(a, context)?;
                self.expr(b, context)?
            }
            Expr::If(a, b, c) => {
                self.expr(a, context)?;
                self.expr(b, context)?;
                self.expr(c, context)?
            }
            Expr::Call(name, args) => {
                for a in args {
                    self.expr(a, context)?;
                }
                if self.checked.functions.contains_key(name) {
                    let actual = args
                        .iter()
                        .map(|a| self.expr_type(a, context))
                        .collect::<Result<Vec<_>, _>>()?;
                    self.instance(name, actual, ty)?;
                }
            }
            Expr::Match(e, arms) => {
                self.expr(e, context)?;
                for arm in arms {
                    self.expr(&arm.body, context)?;
                }
            }
            Expr::Record(_, fields) => {
                for (_, value) in fields {
                    self.expr(value, context)?;
                }
            }
            Expr::FieldAssign(_, _, value) => self.expr(value, context)?,
            Expr::Assign(_, value) => self.expr(value, context)?,
            Expr::Int(_)
            | Expr::Bool(_)
            | Expr::Unit
            | Expr::CStr(_)
            | Expr::Str(_)
            | Expr::Inout(_)
            | Expr::Move(_)
            | Expr::InoutField(_, _)
            | Expr::Field(_, _)
            | Expr::Var(_) => {}
        }
        Ok(())
    }
}

pub(super) fn max_inline_nodes(checked: &CheckedProgram<'_>) -> Result<usize, String> {
    let main = checked
        .functions
        .get("main")
        .ok_or("E-C-ADT: missing main")?;
    let result = substitute(&main.result, &Substitution::new())?;
    let mut analyzer = Analyzer {
        checked,
        visited: HashSet::new(),
        max: 1,
    };
    analyzer.instance("main", vec![], result)?;
    Ok(analyzer.max)
}

/// Test-only oracle retained while the typed worklist replaces uniform layout
/// specialization. This traverses the old analyzer, not the new implementation.
#[cfg(test)]
pub(super) fn reference_instance_keys(
    checked: &CheckedProgram<'_>,
) -> Result<HashSet<super::ConcreteInstanceKey>, String> {
    let result = substitute(&checked.functions["main"].result, &Substitution::new())?;
    let mut analyzer = Analyzer {
        checked,
        visited: HashSet::new(),
        max: 1,
    };
    analyzer.instance("main", vec![], result)?;
    // The old analyzer also records builtin calls; only source bodies become
    // typed function instances.
    analyzer
        .visited
        .retain(|(name, _, _)| checked.program.functions.iter().any(|f| f.name == *name));
    Ok(analyzer.visited)
}
