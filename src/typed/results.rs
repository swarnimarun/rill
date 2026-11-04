//! Computation results are distinct from types that inhabit runtime storage.
//! Value expressions retain the existing `ExprNode` contract. Bare logical
//! results have a separate syntax tree, so they cannot accidentally become a
//! local, eager operand, pattern input, field or call argument.
use super::{CallArg, CopyPattern, ExprNode, LocalId, OpaqueId, Type};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ResultRepr {
    Value(Type),
    NoNormal(OpaqueId),
}
impl ResultRepr {
    pub(super) fn value(&self) -> Result<Type, String> {
        match self {
            Self::Value(ty) => Ok(*ty),
            Self::NoNormal(_) => Err("E-TYPED-CONTROL: a value result is required".into()),
        }
    }
    pub(super) fn value_ref(&self) -> Result<&Type, String> {
        match self {
            Self::Value(ty) => Ok(ty),
            Self::NoNormal(_) => Err("E-TYPED-CONTROL: a value result is required".into()),
        }
    }
}
impl From<Type> for ResultRepr {
    fn from(value: Type) -> Self {
        Self::Value(value)
    }
}
impl PartialEq<Type> for ResultRepr {
    fn eq(&self, other: &Type) -> bool {
        matches!(self, Self::Value(ty) if ty == other)
    }
}
impl PartialEq<ResultRepr> for Type {
    fn eq(&self, other: &ResultRepr) -> bool {
        other == self
    }
}

#[derive(Clone, Debug)]
pub enum Computation {
    Value(ExprNode),
    NoNormal(NoNormalExpr),
}
impl Computation {
    pub(super) fn value_expr(&self) -> Result<&ExprNode, String> {
        match self {
            Self::Value(value) => Ok(value),
            Self::NoNormal(_) => Err("E-TYPED-CONTROL: a value expression is required".into()),
        }
    }
    #[cfg(test)]
    pub(super) fn value_expr_mut(&mut self) -> Result<&mut ExprNode, String> {
        match self {
            Self::Value(value) => Ok(value),
            Self::NoNormal(_) => Err("E-TYPED-CONTROL: a value expression is required".into()),
        }
    }
}
impl From<ExprNode> for Computation {
    fn from(value: ExprNode) -> Self {
        Self::Value(value)
    }
}

/// A source binding with no runtime `LocalId` or storage. The canonical source
/// relation, not its spelling or an absent use-map entry, must prove it unused.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NoStorageBinding {
    pub(super) domain: super::logical::Domain,
    pub(super) occurrence: usize,
}

#[derive(Clone, Debug)]
pub struct NoNormalExpr {
    pub result: OpaqueId,
    pub(super) occurrence: usize,
    pub kind: NoNormalKind,
}
#[derive(Clone, Debug)]
pub struct NoNormalArm {
    pub pattern: CopyPattern,
    pub body: NoNormalExpr,
}
#[derive(Clone, Debug)]
pub enum NoNormalKind {
    Call(usize, Vec<CallArg>),
    Let {
        local: LocalId,
        mutable: bool,
        value: Box<ExprNode>,
        body: Box<NoNormalExpr>,
    },
    NoStorageLet {
        binding: NoStorageBinding,
        value: Box<NoNormalExpr>,
        body: Box<NoNormalExpr>,
    },
    If {
        condition: Box<ExprNode>,
        yes: Box<NoNormalExpr>,
        no: Box<NoNormalExpr>,
    },
    Match {
        value: Box<ExprNode>,
        arms: Vec<NoNormalArm>,
    },
}

impl NoNormalExpr {
    /// Visit every concrete subtree root, including retained unreachable tails.
    /// Callers run only after the bounded verifier has admitted this tree.
    pub(super) fn any_value_root(&self, predicate: &mut impl FnMut(&ExprNode) -> bool) -> bool {
        let mut pending = vec![self];
        while let Some(node) = pending.pop() {
            match &node.kind {
                NoNormalKind::Call(_, args) => {
                    for arg in args {
                        if let CallArg::Value(value) | CallArg::ReadTemporary(value) = arg {
                            if predicate(value) {
                                return true;
                            }
                        }
                    }
                }
                NoNormalKind::Let { value, body, .. } => {
                    if predicate(value) {
                        return true;
                    }
                    pending.push(body);
                }
                NoNormalKind::NoStorageLet { value, body, .. } => {
                    pending.push(body);
                    pending.push(value);
                }
                NoNormalKind::If { condition, yes, no } => {
                    if predicate(condition) {
                        return true;
                    }
                    pending.push(no);
                    pending.push(yes);
                }
                NoNormalKind::Match { value, arms } => {
                    if predicate(value) {
                        return true;
                    }
                    pending.extend(arms.iter().rev().map(|arm| &arm.body));
                }
            }
        }
        false
    }
    pub(super) fn visit_calls(&self, output: &mut Vec<usize>) {
        let mut pending = vec![self];
        while let Some(node) = pending.pop() {
            match &node.kind {
                NoNormalKind::Call(callee, args) => {
                    output.push(*callee);
                    for arg in args {
                        if let CallArg::Value(value) | CallArg::ReadTemporary(value) = arg {
                            super::collect_calls(value, output);
                        }
                    }
                }
                NoNormalKind::Let { value, body, .. } => {
                    super::collect_calls(value, output);
                    pending.push(body);
                }
                NoNormalKind::NoStorageLet { value, body, .. } => {
                    pending.push(body);
                    pending.push(value);
                }
                NoNormalKind::If { condition, yes, no } => {
                    super::collect_calls(condition, output);
                    pending.push(no);
                    pending.push(yes);
                }
                NoNormalKind::Match { value, arms } => {
                    super::collect_calls(value, output);
                    pending.extend(arms.iter().rev().map(|arm| &arm.body));
                }
            }
        }
    }
}

/// No runtime value exists on this path. Prefixes and branch selection remain
/// ordinary evaluations; a genuine recursive call uses the same frame guard.
#[inline(never)]
pub(super) fn evaluate_no_normal(
    mut expression: &NoNormalExpr,
    locals: &mut super::CopyLocals,
    program: &super::TypedMain,
    depth: usize,
    state: &mut super::EvalState,
    owners: &mut std::collections::HashMap<LocalId, super::OwnerHandle>,
) -> Result<std::convert::Infallible, String> {
    let mut introduced = Vec::new();
    let outcome = (|| loop {
        match &expression.kind {
            NoNormalKind::Call(callee, args) => {
                super::evaluate_call(*callee, args, locals, program, depth, state, owners)?;
                break Err("E-TYPED-CONTROL: unexpected normal return".into());
            }
            NoNormalKind::Let {
                local, value, body, ..
            } => {
                let value = super::evaluate_expr(value, locals, program, depth, state, owners)?;
                locals.insert(*local, value);
                introduced.push(*local);
                expression = body;
            }
            NoNormalKind::NoStorageLet { value, .. } => expression = value,
            NoNormalKind::If { condition, yes, no } => {
                expression =
                    match super::evaluate_expr(condition, locals, program, depth, state, owners)? {
                        crate::interpreter::Value::Bool(true) => yes,
                        crate::interpreter::Value::Bool(false) => no,
                        _ => break Err("E-TYPED-CONTROL: invalid condition".into()),
                    };
            }
            NoNormalKind::Match { value, arms } => {
                let value = super::evaluate_expr(value, locals, program, depth, state, owners)?;
                let mut selected = None;
                for arm in arms {
                    let mut bindings = Vec::new();
                    if super::copy_match::matches_value(
                        &arm.pattern,
                        &value,
                        program,
                        &mut bindings,
                    ) {
                        introduced.extend(bindings.iter().map(|(id, _)| *id));
                        locals.extend(bindings);
                        selected = Some(&arm.body);
                        break;
                    }
                }
                expression = selected.ok_or("E-TYPED-MATCH: missing runtime arm")?;
            }
        }
    })();
    for local in introduced.into_iter().rev() {
        locals.remove(&local);
    }
    outcome
}

/// Ordinary type/scope/loan verification is independent of the source-control
/// relation. Every retained continuation is checked even after a false flow.
pub(super) fn verify_no_normal(
    mut expression: &NoNormalExpr,
    scope: &mut std::collections::HashMap<LocalId, super::Local>,
    ids: &mut std::collections::HashSet<LocalId>,
    context: &super::VerifyContext<'_>,
) -> Result<OpaqueId, String> {
    use super::*;
    let expected = expression.result.clone();
    let mut introduced = Vec::new();
    let outcome = (|| loop {
        context
            .logical
            .opaque_in_scope(&expression.result, context.function)?;
        if expression.result != expected {
            break Err("E-TYPED-CONTROL: inconsistent no-normal result identity".into());
        }
        match &expression.kind {
            NoNormalKind::Call(callee, arguments) => {
                let function = context
                    .functions
                    .get(*callee)
                    .ok_or("E-TYPED: invalid function identity")?;
                if function
                    .params
                    .iter()
                    .any(|(_, ty, _)| ty.is_owner() || ty.is_raw())
                {
                    break Err(
                        "E-TYPED-CONTROL: no-normal arguments require pure Copy layouts".into(),
                    );
                }
                if verify_call_arguments(*callee, arguments, scope, ids, context)?
                    != ResultRepr::NoNormal(expected.clone())
                {
                    break Err("E-TYPED-CONTROL: wrong no-normal call result".into());
                }
                break Ok(expected.clone());
            }
            NoNormalKind::Let {
                local,
                mutable,
                value,
                body,
            } => {
                let ty = verify_expr(value, scope, ids, context)?;
                if ty.is_owner() || ty.is_raw() {
                    break Err(
                        "E-TYPED-CONTROL: no-normal prefix requires a pure Copy value".into(),
                    );
                }
                if !ids.insert(*local) {
                    break Err("E-TYPED: duplicate local identity".into());
                }
                scope.insert(
                    *local,
                    Local {
                        id: *local,
                        ty,
                        mutable: *mutable,
                        initialization: Initialization::full(ty, context.adts)?,
                        loaned: false,
                        shared: 0,
                        field_loans: Vec::new(),
                        mode: ParamMode::Value,
                    },
                );
                introduced.push(*local);
                expression = body;
            }
            NoNormalKind::NoStorageLet { value, body, .. } => {
                verify_no_normal(value, scope, ids, context)?;
                expression = body;
            }
            NoNormalKind::If { condition, yes, no } => {
                if verify_expr(condition, scope, ids, context)? != Type::Bool {
                    break Err("E-TYPED: invalid condition".into());
                }
                let mut yes_scope = scope.clone();
                let mut no_scope = scope.clone();
                if verify_no_normal(yes, &mut yes_scope, ids, context)? != expected
                    || verify_no_normal(no, &mut no_scope, ids, context)? != expected
                    || !same_owner_states(&yes_scope, &no_scope)
                {
                    break Err("E-TYPED-CONTROL: inconsistent no-normal branches".into());
                }
                *scope = yes_scope;
                break Ok(expected.clone());
            }
            NoNormalKind::Match { value, arms } => {
                let ty = verify_expr(value, scope, ids, context)?;
                if ty.is_owner() || ty.is_raw() || arms.is_empty() {
                    break Err("E-TYPED-CONTROL: invalid no-normal match input or arms".into());
                }
                let mut budget = 100_000;
                let mut joined = None;
                for arm in arms {
                    let mut branch = scope.clone();
                    let mut bindings = Vec::new();
                    copy_match::pattern_analysis(context, &mut budget, |budget| {
                        copy_match::verify_pattern(
                            &arm.pattern,
                            ty,
                            context.adts,
                            &mut bindings,
                            0,
                            budget,
                        )
                    })?;
                    for &(id, ty) in &bindings {
                        if !ids.insert(id) {
                            return Err("E-TYPED-MATCH: duplicate pattern binding identity".into());
                        }
                        branch.insert(
                            id,
                            Local {
                                id,
                                ty,
                                mutable: false,
                                initialization: Initialization::full(ty, context.adts)?,
                                loaned: false,
                                shared: 0,
                                field_loans: Vec::new(),
                                mode: ParamMode::Value,
                            },
                        );
                    }
                    if verify_no_normal(&arm.body, &mut branch, ids, context)? != expected {
                        return Err("E-TYPED-CONTROL: inconsistent no-normal match result".into());
                    }
                    for (id, _) in bindings {
                        branch.remove(&id);
                    }
                    if let Some(first) = &joined {
                        if !same_owner_states(first, &branch) {
                            return Err(
                                "E-TYPED-CONTROL: incompatible no-normal match states".into()
                            );
                        }
                    } else {
                        joined = Some(branch);
                    }
                }
                copy_match::reserve_rows(context, arms.len())?;
                let rows = arms
                    .iter()
                    .map(|arm| vec![&arm.pattern])
                    .collect::<Vec<_>>();
                if !copy_match::pattern_analysis(context, &mut budget, |budget| {
                    copy_match::exhaustive_mode(
                        &[ty],
                        &rows,
                        context.adts,
                        budget,
                        context.pattern_work.is_some(),
                    )
                })? {
                    return Err("E-TYPED-MATCH: nonexhaustive match".into());
                }
                *scope = joined.ok_or("E-TYPED-MATCH: empty match")?;
                break Ok(expected.clone());
            }
        }
    })();
    for local in introduced.into_iter().rev() {
        scope.remove(&local);
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typed::{emit_c, evaluate, lower_main, verify, TypedMain};

    fn symbolic_program() -> TypedMain {
        let parsed = crate::parser::parse("type Flag<a> = Off | On fn main() { Off }").unwrap();
        lower_main(&crate::check::analyze(&parsed).unwrap()).unwrap()
    }
    fn rejects(program: &TypedMain) {
        assert!(verify(program).is_err());
        assert!(evaluate(program).is_err());
        assert!(emit_c(program).is_err());
    }

    #[test]
    fn no_normal_identity_is_not_a_runtime_value_type() {
        let program = symbolic_program();
        let identity = program.logical.opaque_id(0).unwrap();
        let result = ResultRepr::NoNormal(identity);
        assert!(result.value().is_err());
        assert!(result.value_ref().is_err());
        for ty in [Type::Int, Type::Bool, Type::Unit] {
            assert_ne!(result, ty);
            assert_eq!(ResultRepr::Value(ty).value().unwrap(), ty);
        }
    }

    #[test]
    fn equal_void_abi_does_not_merge_compilation_brands() {
        let first = symbolic_program();
        let second = symbolic_program();
        assert_ne!(
            ResultRepr::NoNormal(first.logical.opaque_id(0).unwrap()),
            ResultRepr::NoNormal(second.logical.opaque_id(0).unwrap())
        );
    }

    #[test]
    fn result_slot_migration_does_not_admit_unproved_no_normal_results() {
        let original = symbolic_program();
        let identity = original.logical.opaque_id(0).unwrap();
        let mut forged = original.clone();
        forged.functions[forged.entry].result = ResultRepr::NoNormal(identity.clone());
        rejects(&forged);
        forged.functions[forged.entry].body = Computation::NoNormal(NoNormalExpr {
            occurrence: 0,
            result: identity,
            kind: NoNormalKind::Call(forged.entry, Vec::new()),
        });
        rejects(&forged);
    }

    #[test]
    fn a_no_normal_body_cannot_be_used_as_a_value_expression() {
        let program = symbolic_program();
        let mut computation = Computation::NoNormal(NoNormalExpr {
            occurrence: 0,
            result: program.logical.opaque_id(0).unwrap(),
            kind: NoNormalKind::Call(program.entry, Vec::new()),
        });
        assert!(computation.value_expr().is_err());
        assert!(computation.value_expr_mut().is_err());
    }
    #[test]
    fn unproved_no_storage_is_rejected_inside_an_unselected_concrete_branch() {
        let mut program = symbolic_program();
        let identity = program.logical.opaque_id(0).unwrap();
        let concrete = ExprNode {
            ty: Type::Int,
            kind: super::super::ExprKind::Int(42),
        };
        let hidden = ExprNode {
            ty: Type::Int,
            kind: super::super::ExprKind::NoStorageLet {
                binding: NoStorageBinding {
                    domain: identity.domain.clone(),
                    occurrence: 0,
                },
                value: Box::new(NoNormalExpr {
                    occurrence: 0,
                    result: identity,
                    kind: NoNormalKind::Call(program.entry, vec![]),
                }),
                body: Box::new(concrete.clone()),
            },
        };
        program.functions[program.entry].result = ResultRepr::Value(Type::Int);
        program.functions[program.entry].body = ExprNode {
            ty: Type::Int,
            kind: super::super::ExprKind::If(
                Box::new(ExprNode {
                    ty: Type::Bool,
                    kind: super::super::ExprKind::Bool(false),
                }),
                Box::new(hidden),
                Box::new(concrete),
            ),
        }
        .into();
        rejects(&program);
    }

    #[test]
    fn no_normal_evaluator_executes_every_frame_and_preserves_inout_effects() {
        use super::super::*;
        use crate::interpreter::Value;
        let mut program = symbolic_program();
        let identity = program.logical.opaque_id(0).unwrap();
        let root = LocalId(0);
        let prefix = ExprNode {
            ty: Type::Unit,
            kind: ExprKind::Assign(
                root,
                Box::new(ExprNode {
                    ty: Type::Int,
                    kind: ExprKind::Binary(
                        crate::ast::BinaryOp::Add,
                        Box::new(ExprNode {
                            ty: Type::Int,
                            kind: ExprKind::Read(root),
                        }),
                        Box::new(ExprNode {
                            ty: Type::Int,
                            kind: ExprKind::Int(1),
                        }),
                    ),
                }),
            ),
        };
        program.functions = vec![TypedFunction {
            params: vec![(root, Type::Int, crate::ast::ParamMode::Inout)],
            result: ResultRepr::NoNormal(identity.clone()),
            body: Computation::NoNormal(NoNormalExpr {
                occurrence: 0,
                result: identity.clone(),
                kind: NoNormalKind::Let {
                    local: LocalId(1),
                    mutable: false,
                    value: Box::new(prefix),
                    body: Box::new(NoNormalExpr {
                        occurrence: 0,
                        result: identity,
                        kind: NoNormalKind::Call(0, vec![CallArg::InoutCopy(root)]),
                    }),
                },
            }),
            param_cleanup: vec![],
        }];
        program.entry = 0;
        let mut caller = CopyLocals::default();
        caller.insert(root, Value::Int(0));
        let result = evaluate_function(
            &program,
            0,
            vec![EvalArg::CopyLoan(caller.loan(&root).unwrap())],
            1,
            &mut EvalState::default(),
        );
        assert_eq!(result.unwrap_err(), "runtime trap");
        assert_eq!(caller.read(&root).unwrap(), Value::Int(32));
    }

    #[test]
    fn no_normal_evaluator_rejects_an_unexpected_return_instead_of_fabricating_a_value() {
        let program = symbolic_program();
        let expression = NoNormalExpr {
            occurrence: 0,
            result: program.logical.opaque_id(0).unwrap(),
            kind: NoNormalKind::Call(program.entry, vec![]),
        };
        let error = evaluate_no_normal(
            &expression,
            &mut super::super::CopyLocals::default(),
            &program,
            1,
            &mut super::super::EvalState::default(),
            &mut std::collections::HashMap::new(),
        )
        .unwrap_err();
        assert_eq!(error, "E-TYPED-CONTROL: unexpected normal return");
    }
    fn ordinary_no_normal(
        program: &TypedMain,
        expression: &NoNormalExpr,
    ) -> Result<OpaqueId, String> {
        let logical = program.logical.verify(program)?;
        let context = super::super::VerifyContext {
            pattern_work: None,
            logical: &logical,
            function: program.entry,
            functions: &program.functions,
            adts: &program.adts,
            native: &program.native,
        };
        verify_no_normal(
            expression,
            &mut std::collections::HashMap::new(),
            &mut std::collections::HashSet::new(),
            &context,
        )
    }
    #[test]
    fn ordinary_no_normal_verifier_checks_result_brand_arity_and_pattern_types() {
        let mut program = symbolic_program();
        let identity = program.logical.opaque_id(0).unwrap();
        program.functions[program.entry].result = ResultRepr::NoNormal(identity.clone());
        let call = NoNormalExpr {
            result: identity.clone(),
            occurrence: 0,
            kind: NoNormalKind::Call(program.entry, vec![]),
        };
        ordinary_no_normal(&program, &call).unwrap();
        let mut altered = call.clone();
        altered.result.domain = super::super::logical::Domain::default();
        assert!(ordinary_no_normal(&program, &altered).is_err());
        let mut altered = call.clone();
        altered.kind = NoNormalKind::Call(
            program.entry,
            vec![CallArg::Value(ExprNode {
                ty: Type::Int,
                kind: super::super::ExprKind::Int(1),
            })],
        );
        assert!(ordinary_no_normal(&program, &altered).is_err());
        let altered = NoNormalExpr {
            result: identity.clone(),
            occurrence: 0,
            kind: NoNormalKind::Match {
                value: Box::new(ExprNode {
                    ty: Type::Int,
                    kind: super::super::ExprKind::Int(1),
                }),
                arms: vec![NoNormalArm {
                    pattern: CopyPattern::Bool(true),
                    body: call.clone(),
                }],
            },
        };
        assert!(ordinary_no_normal(&program, &altered).is_err());
        let altered = NoNormalExpr {
            result: identity.clone(),
            occurrence: 0,
            kind: NoNormalKind::Match {
                value: Box::new(ExprNode {
                    ty: Type::Int,
                    kind: super::super::ExprKind::Int(1),
                }),
                arms: vec![NoNormalArm {
                    pattern: CopyPattern::Int(1),
                    body: call.clone(),
                }],
            },
        };
        assert!(ordinary_no_normal(&program, &altered).is_err());
        program.functions[program.entry].result = ResultRepr::Value(Type::Int);
        assert!(ordinary_no_normal(&program, &call).is_err());
    }
    #[test]
    fn ordinary_no_normal_verifier_checks_duplicate_locals_and_loan_modes() {
        use super::super::*;
        let mut program = symbolic_program();
        let identity = program.logical.opaque_id(0).unwrap();
        program.functions[program.entry].result = ResultRepr::NoNormal(identity.clone());
        let call = NoNormalExpr {
            result: identity.clone(),
            occurrence: 0,
            kind: NoNormalKind::Call(program.entry, vec![]),
        };
        let prefix = |body| NoNormalExpr {
            result: identity.clone(),
            occurrence: 0,
            kind: NoNormalKind::Let {
                local: LocalId(0),
                mutable: false,
                value: Box::new(ExprNode {
                    ty: Type::Int,
                    kind: ExprKind::Int(1),
                }),
                body: Box::new(body),
            },
        };
        assert!(ordinary_no_normal(&program, &prefix(prefix(call.clone()))).is_err());
        let mut target = program.functions[program.entry].clone();
        target.params = vec![(LocalId(1), Type::Int, ParamMode::Inout)];
        program.functions.push(target);
        // Extend the trusted logical scope tables only for this ordinary-rule
        // fixture, not as a source-certificate or public admission claim.
        program.logical.function_scopes.push(1);
        program
            .logical
            .scope_sources
            .push(program.logical.scope_sources[0]);
        let borrowed = NoNormalExpr {
            result: identity.clone(),
            occurrence: 0,
            kind: NoNormalKind::Call(1, vec![CallArg::InoutCopy(LocalId(0))]),
        };
        assert!(ordinary_no_normal(&program, &prefix(borrowed)).is_err());
    }
}
