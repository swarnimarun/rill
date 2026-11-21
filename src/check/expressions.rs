//! Inference for recursive operator and local-binding expressions.
use super::{Checker, Scheme, Type};
use crate::ast::{BinaryOp, Expr};
use std::collections::{HashMap, HashSet};

impl Checker {
    pub(super) fn infer_binary(
        &mut self,
        op: &BinaryOp,
        a: &Expr,
        b: &Expr,
        locals: &mut HashMap<String, Scheme>,
    ) -> Result<Type, String> {
        let a = self.infer(a, locals)?;
        let b = self.infer(b, locals)?;
        use BinaryOp::*;
        let (operator, operand, result) = match op {
            Add => ("+", Type::Int, Type::Int),
            Sub => ("-", Type::Int, Type::Int),
            Mul => ("*", Type::Int, Type::Int),
            Div => ("/", Type::Int, Type::Int),
            Rem => ("%", Type::Int, Type::Int),
            Lt => ("<", Type::Int, Type::Bool),
            Le => ("<=", Type::Int, Type::Bool),
            Gt => (">", Type::Int, Type::Bool),
            Ge => (">=", Type::Int, Type::Bool),
            And => ("&&", Type::Bool, Type::Bool),
            Or => ("||", Type::Bool, Type::Bool),
            Eq | Ne => {
                self.unify(self.copy_type(&a)?, b)?;
                self.equality_types.push(a);
                return Ok(Type::Bool);
            }
        };
        self.projections.dirty = true;
        for (side, actual) in [("left ", a), ("right ", b)] {
            // The parser also uses subtraction for unary minus. The AST alone
            // cannot distinguish that form from written "0 - expression".
            let side = if *op == Sub { "" } else { side };
            self.solver
                .unify_with(actual, operand.clone(), |found, expected| {
                    format!(
                        "E-TYPE: {side}operand of '{operator}' expects {expected}, found {found}"
                    )
                })?;
        }
        Ok(result)
    }
    pub(super) fn infer_let(
        &mut self,
        expr: &Expr,
        n: &str,
        value: &Expr,
        body: &Expr,
        locals: &mut HashMap<String, Scheme>,
    ) -> Result<Type, String> {
        self.level += 1;
        let inferred = self.infer(value, locals);
        self.level -= 1;
        let ty = inferred?;
        // Projection solving can introduce free nominal arguments. Discharge
        // before collecting the variables eligible for generalization.
        self.resolve_projections(false)?;
        let mut free = HashSet::new();
        self.free_type(&ty, &mut free);
        let mut env = HashSet::new();
        for s in locals.values() {
            self.free_scheme(s, &mut env)
        }
        self.free_functions(&mut env);
        // Keep the level/environment cross-check independent of the
        // value restriction. Rejected candidates become weak variables
        // at this boundary, so immutable aliases cannot generalize them.
        let mut quantified = self.generalize(free.clone(), &env)?;
        if !self.nonexpansive(value, locals) || self.has_owner_dependency(&ty)? {
            for id in free {
                self.solver.lower_variable(id, self.level);
            }
            quantified.clear();
        }
        let local_id = self.local_id();
        if !quantified.is_empty() {
            self.polymorphic_locals
                .insert(expr as *const Expr, local_id);
        }
        let old = locals.insert(
            n.to_owned(),
            Scheme {
                quantified,
                ty,
                mutable: false,
                borrowed: false,
                local_id,
            },
        );
        let result = self.infer(body, locals);
        match old {
            Some(v) => {
                locals.insert(n.to_owned(), v);
            }
            None => {
                locals.remove(n);
            }
        }
        result
    }
    pub(super) fn infer_let_mut(
        &mut self,
        n: &str,
        value: &Expr,
        body: &Expr,
        locals: &mut HashMap<String, Scheme>,
    ) -> Result<Type, String> {
        let ty = self.infer(value, locals)?;
        let local_id = self.local_id();
        let old = locals.insert(
            n.to_owned(),
            Scheme {
                quantified: Vec::new(),
                ty,
                mutable: true,
                borrowed: false,
                local_id,
            },
        );
        let result = self.infer(body, locals);
        match old {
            Some(v) => {
                locals.insert(n.to_owned(), v);
            }
            None => {
                locals.remove(n);
            }
        }
        result
    }
}
