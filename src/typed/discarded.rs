//! Demand-only computations retain exact logical results but never inhabit storage.
//! The mandatory canonical relation independently proves every discard edge.
use super::*;

#[derive(Clone, Debug)]
pub struct DiscardExpr {
    pub result: LogicalType,
    pub(super) occurrence: usize,
    pub kind: DiscardKind,
}
#[derive(Clone, Debug)]
pub struct DiscardArm {
    pub pattern: CopyPattern,
    pub body: DiscardExpr,
}
#[derive(Clone, Debug)]
pub enum DiscardKind {
    Value(Box<ExprNode>),
    FromNoNormal(Box<NoNormalExpr>),
    Call(usize, Vec<CallArg>),
    Constructor {
        definition: usize,
        variant: usize,
        fields: Vec<DiscardExpr>,
    },
    Let {
        local: LocalId,
        mutable: bool,
        value: Box<ExprNode>,
        body: Box<DiscardExpr>,
    },
    DiscardLet {
        binding: NoStorageBinding,
        value: Box<DiscardExpr>,
        body: Box<DiscardExpr>,
    },
    NoStorageLet {
        binding: NoStorageBinding,
        value: Box<NoNormalExpr>,
        body: Box<DiscardExpr>,
    },
    If {
        condition: Box<ExprNode>,
        yes: Box<DiscardExpr>,
        no: Box<DiscardExpr>,
    },
    Match {
        value: Box<ExprNode>,
        arms: Vec<DiscardArm>,
    },
}

impl Lower<'_, '_> {
    pub(super) fn discard(&mut self, expression: &Expr) -> Result<DiscardExpr, String> {
        self.instances.charge_control(1)?;
        let facts = self
            .control
            .ok_or("E-TYPED-CONTROL: discard needs canonical source")?;
        let source_type = self.instances.expression_type_demand(
            self.checked,
            expression,
            self.substitution,
            self.control,
            instances::Demand::Discard,
        )?;
        let result = self
            .registry
            .logical
            .logical_type(&source_type, &mut *self.instances.control_budget()?)?;
        let occurrence = facts.occurrence(expression)?;
        let kind = if matches!(source_type, InferredType::Var(_))
            && facts.expression_may_return(expression) == Some(false)
        {
            DiscardKind::FromNoNormal(Box::new(self.no_normal(expression)?))
        } else {
            match expression {
                Expr::Call(name, args) if self.functions.contains_key(name) => {
                    if let Some(call) = self.interrupted_call(expression)? {
                        let ty = self.registry.convert(&source_type)?;
                        pure(ty)?;
                        return Ok(DiscardExpr {
                            result,
                            occurrence,
                            kind: DiscardKind::Value(Box::new(ExprNode {
                                ty,
                                kind: ExprKind::InterruptedCall(Box::new(call)),
                            })),
                        });
                    }
                    let (callee, args) = self.call_arguments_demand(
                        expression,
                        name,
                        args,
                        instances::Demand::Discard,
                    )?;
                    DiscardKind::Call(callee, args)
                }
                Expr::Let(name, value, body) | Expr::LetMut(name, value, body) => {
                    if self.instances.initializer_demand(
                        self.checked,
                        expression,
                        self.substitution,
                        self.control,
                    )? == instances::Demand::Discard
                    {
                        DiscardKind::DiscardLet {
                            binding: facts.discard_binding(expression, &self.instances.domain)?,
                            value: Box::new(self.discard(value)?),
                            body: Box::new(self.discard(body)?),
                        }
                    } else if matches!(self.source_type(value)?, InferredType::Var(_)) {
                        DiscardKind::NoStorageLet {
                            binding: facts
                                .no_storage_binding(expression, &self.instances.domain)?,
                            value: Box::new(self.no_normal(value)?),
                            body: Box::new(self.discard(body)?),
                        }
                    } else {
                        let value = self.expr(value)?;
                        pure(value.ty)?;
                        let local = self.fresh()?;
                        let mutable = matches!(expression, Expr::LetMut(..));
                        let previous = self.locals.insert(
                            name.clone(),
                            Local {
                                id: local,
                                ty: value.ty,
                                mutable,
                                initialization: Initialization::full(
                                    value.ty,
                                    &self.registry.types,
                                )?,
                                loaned: false,
                                shared: 0,
                                field_loans: Vec::new(),
                                mode: ParamMode::Value,
                            },
                        );
                        let body = self.discard(body);
                        if let Some(previous) = previous {
                            self.locals.insert(name.clone(), previous);
                        } else {
                            self.locals.remove(name);
                        }
                        DiscardKind::Let {
                            local,
                            mutable,
                            value: Box::new(value),
                            body: Box::new(body?),
                        }
                    }
                }
                Expr::If(condition, yes, no) => {
                    let condition = self.expr(condition)?;
                    let before = self.locals.clone();
                    let yes = self.discard(yes)?;
                    let after = self.locals.clone();
                    self.locals = before;
                    let no = self.discard(no)?;
                    if !Self::same_states(&after, &self.locals) {
                        return Err("E-TYPED-CONTROL: incompatible discard branch states".into());
                    }
                    self.locals = after;
                    DiscardKind::If {
                        condition: Box::new(condition),
                        yes: Box::new(yes),
                        no: Box::new(no),
                    }
                }
                Expr::Match(value, arms) => {
                    let value = self.expr(value)?;
                    pure(value.ty)?;
                    let outer = self.locals.clone();
                    self.instances.charge_control(arms.len())?;
                    let mut lowered = Vec::with_capacity(arms.len());
                    let mut joined = None;
                    for arm in arms {
                        self.locals = outer.clone();
                        let (pattern, _) = self.copy_pattern(&arm.pattern, value.ty)?;
                        let body = self.discard(&arm.body)?;
                        let after = outer
                            .iter()
                            .map(|(name, old)| {
                                (
                                    name.clone(),
                                    self.locals
                                        .get(name)
                                        .filter(|local| local.id == old.id)
                                        .unwrap_or(old)
                                        .clone(),
                                )
                            })
                            .collect();
                        if let Some(first) = &joined {
                            if !Self::same_states(first, &after) {
                                return Err(
                                    "E-TYPED-CONTROL: incompatible discard match states".into()
                                );
                            }
                        } else {
                            joined = Some(after);
                        }
                        lowered.push(DiscardArm { pattern, body });
                    }
                    self.locals = joined.unwrap_or(outer);
                    DiscardKind::Match {
                        value: Box::new(value),
                        arms: lowered,
                    }
                }
                Expr::Call(name, _) | Expr::Var(name)
                    if self.instances.discard_structural(
                        self.checked,
                        expression,
                        self.control,
                    )? =>
                {
                    let LogicalType::Nominal { definition, .. } = &result else {
                        return Err(
                            "E-TYPED-CONTROL: discarded constructor needs nominal result".into(),
                        );
                    };
                    let declaration = self
                        .checked
                        .program
                        .types
                        .get(*definition)
                        .ok_or("E-TYPED-CONTROL: invalid discard definition")?;
                    self.instances.charge_control(declaration.variants.len())?;
                    for variant in &declaration.variants {
                        self.instances
                            .charge_control(name.len().saturating_add(variant.name.len()))?;
                    }
                    let variant = declaration
                        .variants
                        .iter()
                        .position(|v| v.name == *name)
                        .ok_or("E-TYPED-CONTROL: wrong discard constructor")?;
                    let args: &[Expr] = match expression {
                        Expr::Call(_, args) => args,
                        _ => &[],
                    };
                    if declaration.record_fields.is_some()
                        || declaration.variants[variant].fields.len() != args.len()
                    {
                        return Err("E-TYPED-CONTROL: invalid discarded constructor arity".into());
                    }
                    self.instances.charge_control(args.len())?;
                    let fields = args
                        .iter()
                        .map(|arg| self.discard(arg))
                        .collect::<Result<_, _>>()?;
                    DiscardKind::Constructor {
                        definition: *definition,
                        variant,
                        fields,
                    }
                }
                _ => {
                    let value = self.expr(expression)?;
                    pure(value.ty)?;
                    DiscardKind::Value(Box::new(value))
                }
            }
        };
        Ok(DiscardExpr {
            result,
            occurrence,
            kind,
        })
    }
}
fn pure(ty: Type) -> Result<(), String> {
    if ty.is_owner() || ty.is_raw() {
        Err("E-TYPED-CONTROL: discarded computation requires pure Copy storage".into())
    } else {
        Ok(())
    }
}

impl DiscardExpr {
    pub(super) fn any_value_root(&self, predicate: &mut impl FnMut(&ExprNode) -> bool) -> bool {
        let mut pending = vec![self];
        while let Some(node) = pending.pop() {
            match &node.kind {
                DiscardKind::Value(value) => {
                    if predicate(value) {
                        return true;
                    }
                }
                DiscardKind::FromNoNormal(value) => {
                    if value.any_value_root(predicate) {
                        return true;
                    }
                }
                DiscardKind::Call(_, args) => {
                    for arg in args {
                        if let CallArg::Value(v) | CallArg::ReadTemporary(v) = arg {
                            if predicate(v) {
                                return true;
                            }
                        }
                    }
                }
                DiscardKind::Constructor { fields, .. } => pending.extend(fields.iter().rev()),
                DiscardKind::Let { value, body, .. } => {
                    if predicate(value) {
                        return true;
                    }
                    pending.push(body);
                }
                DiscardKind::DiscardLet { value, body, .. } => {
                    pending.push(body);
                    pending.push(value);
                }
                DiscardKind::NoStorageLet { value, body, .. } => {
                    if value.any_value_root(predicate) {
                        return true;
                    }
                    pending.push(body);
                }
                DiscardKind::If { condition, yes, no } => {
                    if predicate(condition) {
                        return true;
                    }
                    pending.push(no);
                    pending.push(yes);
                }
                DiscardKind::Match { value, arms } => {
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
                DiscardKind::Value(value) => collect_calls(value, output),
                DiscardKind::FromNoNormal(value) => value.visit_calls(output),
                DiscardKind::Call(callee, args) => {
                    output.push(*callee);
                    for arg in args {
                        if let CallArg::Value(v) | CallArg::ReadTemporary(v) = arg {
                            collect_calls(v, output);
                        }
                    }
                }
                DiscardKind::Constructor { fields, .. } => pending.extend(fields.iter().rev()),
                DiscardKind::Let { value, body, .. } => {
                    collect_calls(value, output);
                    pending.push(body);
                }
                DiscardKind::DiscardLet { value, body, .. } => {
                    pending.push(body);
                    pending.push(value);
                }
                DiscardKind::NoStorageLet { value, body, .. } => {
                    value.visit_calls(output);
                    pending.push(body);
                }
                DiscardKind::If { condition, yes, no } => {
                    collect_calls(condition, output);
                    pending.push(no);
                    pending.push(yes);
                }
                DiscardKind::Match { value, arms } => {
                    collect_calls(value, output);
                    pending.extend(arms.iter().rev().map(|arm| &arm.body));
                }
            }
        }
    }
}

#[inline(never)]
pub(super) fn evaluate(
    mut expression: &DiscardExpr,
    locals: &mut CopyLocals,
    program: &TypedMain,
    depth: usize,
    state: &mut EvalState,
    owners: &mut HashMap<LocalId, OwnerHandle>,
) -> Result<(), String> {
    let mut introduced = Vec::new();
    let outcome = (|| loop {
        match &expression.kind {
            DiscardKind::Value(value) => {
                evaluate_expr(value, locals, program, depth, state, owners)?;
                break Ok(());
            }
            DiscardKind::FromNoNormal(value) => {
                break match results::evaluate_no_normal(
                    value, locals, program, depth, state, owners,
                ) {
                    Ok(never) => match never {},
                    Err(error) => Err(error),
                };
            }
            DiscardKind::Call(callee, args) => {
                break match evaluate_call_outcome(
                    *callee, args, locals, program, depth, state, owners,
                )? {
                    FunctionReturn::Discarded => Ok(()),
                    FunctionReturn::Value(_) => {
                        Err("E-TYPED-CONTROL: value returned by discard callee".into())
                    }
                };
            }
            DiscardKind::Constructor { fields, .. } => {
                for value in fields {
                    evaluate(value, locals, program, depth, state, owners)?;
                }
                break Ok(());
            }
            DiscardKind::Let {
                local, value, body, ..
            } => {
                let value = evaluate_expr(value, locals, program, depth, state, owners)?;
                locals.insert(*local, value);
                introduced.push(*local);
                expression = body;
            }
            DiscardKind::DiscardLet { value, body, .. } => {
                evaluate(value, locals, program, depth, state, owners)?;
                expression = body;
            }
            DiscardKind::NoStorageLet { value, .. } => {
                break match results::evaluate_no_normal(
                    value, locals, program, depth, state, owners,
                ) {
                    Ok(never) => match never {},
                    Err(error) => Err(error),
                };
            }
            DiscardKind::If { condition, yes, no } => {
                expression = match evaluate_expr(condition, locals, program, depth, state, owners)?
                {
                    crate::interpreter::Value::Bool(true) => yes,
                    crate::interpreter::Value::Bool(false) => no,
                    _ => break Err("E-TYPED-CONTROL: invalid discard condition".into()),
                };
            }
            DiscardKind::Match { value, arms } => {
                let value = evaluate_expr(value, locals, program, depth, state, owners)?;
                let mut selected = None;
                for arm in arms {
                    let mut bindings = Vec::new();
                    if copy_match::matches_value(&arm.pattern, &value, program, &mut bindings) {
                        introduced.extend(bindings.iter().map(|(id, _)| *id));
                        locals.extend(bindings);
                        selected = Some(&arm.body);
                        break;
                    }
                }
                expression = selected.ok_or("E-TYPED-MATCH: missing discard arm")?;
            }
        }
    })();
    for local in introduced.into_iter().rev() {
        locals.remove(&local);
    }
    outcome
}

pub(super) fn verify(
    mut expression: &DiscardExpr,
    scope: &mut std::collections::HashMap<LocalId, super::Local>,
    ids: &mut std::collections::HashSet<LocalId>,
    context: &super::VerifyContext<'_>,
) -> Result<LogicalType, String> {
    let expected = expression.result.clone();
    let mut introduced = Vec::new();
    let outcome = (|| loop {
        context
            .logical
            .logical_in_scope(&expression.result, context.function)?;
        if expression.result != expected {
            break Err("E-TYPED-CONTROL: inconsistent discarded result identity".into());
        }
        match &expression.kind {
            DiscardKind::Value(value) => {
                pure(verify_expr(value, scope, ids, context)?)?;
                break Ok(expected.clone());
            }
            DiscardKind::FromNoNormal(value) => {
                results::verify_no_normal(value, scope, ids, context)?;
                break Ok(expected.clone());
            }
            DiscardKind::Constructor { fields, .. } => {
                for field in fields {
                    verify(field, scope, ids, context)?;
                }
                break Ok(expected.clone());
            }
            DiscardKind::DiscardLet { value, body, .. } => {
                verify(value, scope, ids, context)?;
                expression = body;
            }
            DiscardKind::Call(callee, arguments) => {
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
                        "E-TYPED-CONTROL: discarded arguments require pure Copy layouts".into(),
                    );
                }
                if verify_call_arguments(*callee, arguments, scope, ids, context)?
                    != ResultRepr::Discarded(expected.clone())
                {
                    break Err("E-TYPED-CONTROL: wrong discarded call result".into());
                }
                break Ok(expected.clone());
            }
            DiscardKind::Let {
                local,
                mutable,
                value,
                body,
            } => {
                let ty = verify_expr(value, scope, ids, context)?;
                if ty.is_owner() || ty.is_raw() {
                    break Err(
                        "E-TYPED-CONTROL: discarded prefix requires a pure Copy value".into(),
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
            DiscardKind::NoStorageLet { value, body, .. } => {
                results::verify_no_normal(value, scope, ids, context)?;
                expression = body;
            }
            DiscardKind::If { condition, yes, no } => {
                if verify_expr(condition, scope, ids, context)? != Type::Bool {
                    break Err("E-TYPED: invalid condition".into());
                }
                let mut yes_scope = scope.clone();
                let mut no_scope = scope.clone();
                if verify(yes, &mut yes_scope, ids, context)? != expected
                    || verify(no, &mut no_scope, ids, context)? != expected
                    || !same_owner_states(&yes_scope, &no_scope)
                {
                    break Err("E-TYPED-CONTROL: inconsistent discarded branches".into());
                }
                *scope = yes_scope;
                break Ok(expected.clone());
            }
            DiscardKind::Match { value, arms } => {
                let ty = verify_expr(value, scope, ids, context)?;
                if ty.is_owner() || ty.is_raw() || arms.is_empty() {
                    break Err("E-TYPED-CONTROL: invalid discarded match input or arms".into());
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
                    if verify(&arm.body, &mut branch, ids, context)? != expected {
                        return Err("E-TYPED-CONTROL: inconsistent discarded match result".into());
                    }
                    for (id, _) in bindings {
                        branch.remove(&id);
                    }
                    if let Some(first) = &joined {
                        if !same_owner_states(first, &branch) {
                            return Err(
                                "E-TYPED-CONTROL: incompatible discarded match states".into()
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
    const SOURCE: &str =
        "type O<a> = None | Some(a) fn absent(){None} fn main(){let weak=absent();42}";
    fn lower(source: &str) -> TypedMain {
        let parsed = crate::parser::parse(source).unwrap();
        lower_main(&crate::check::analyze(&parsed).unwrap()).unwrap()
    }
    fn reject(ir: &TypedMain) {
        assert!(super::super::verify(ir).is_err());
        assert!(super::super::evaluate(ir).is_err());
        assert!(super::super::emit_c(ir).is_err());
    }
    fn discarded_body(ir: &mut TypedMain) -> &mut DiscardExpr {
        let Computation::Discarded(value) = &mut ir.functions[0].body else {
            panic!()
        };
        value
    }
    #[test]
    fn public_consumers_reject_discard_result_identity_and_brand_fraud() {
        let original = lower(SOURCE);
        let other = lower(SOURCE);
        let mut changed = original.clone();
        changed.functions[0].result = other.functions[0].result.clone();
        reject(&changed);
        let mut changed = original.clone();
        changed.functions[0].result = ResultRepr::Value(Type::Unit);
        reject(&changed);
        let mut changed = original.clone();
        let LogicalType::Nominal { arguments, .. } = &mut discarded_body(&mut changed).result
        else {
            panic!()
        };
        arguments[0] = LogicalType::Atom(InferredType::Int);
        reject(&changed);
        let mut changed = original;
        changed.entry = 0;
        reject(&changed);
    }
    #[test]
    fn public_consumers_reject_missing_discard_evidence_and_changed_unused_identity() {
        let original = lower(SOURCE);
        let mut changed = original.clone();
        changed.logical.control = None;
        reject(&changed);
        let mut changed = original.clone();
        let ExprKind::DiscardLet { binding, .. } = &mut changed.functions[changed.entry]
            .body
            .value_expr_mut()
            .unwrap()
            .kind
        else {
            panic!()
        };
        binding.occurrence += 1;
        reject(&changed);
        let mut changed = original;
        let ExprKind::DiscardLet { body, .. } = &mut changed.functions[changed.entry]
            .body
            .value_expr_mut()
            .unwrap()
            .kind
        else {
            panic!()
        };
        body.kind = ExprKind::Int(99);
        reject(&changed);
    }
    #[test]
    fn public_consumers_reject_constructor_schema_arity_and_occurrence_mutations() {
        let original = lower(SOURCE);
        for mode in 0..4 {
            let mut changed = original.clone();
            let body = discarded_body(&mut changed);
            match mode {
                0 => body.occurrence += 1,
                1 => {
                    let DiscardKind::Constructor { definition, .. } = &mut body.kind else {
                        panic!()
                    };
                    *definition += 1;
                }
                2 => {
                    let DiscardKind::Constructor { variant, .. } = &mut body.kind else {
                        panic!()
                    };
                    *variant = 1;
                }
                _ => {
                    let field = body.clone();
                    let DiscardKind::Constructor { fields, .. } = &mut body.kind else {
                        panic!()
                    };
                    fields.push(field);
                }
            }
            reject(&changed);
        }
    }
    #[test]
    fn same_signature_observed_and_discarded_calls_are_not_interchangeable() {
        let original = lower("type O<a> = None|Some(a) type W<a> = W(i32,O<a>) fn mark(inout n,d){n<-n*10+d;n} fn made(inout n){W(mark(inout n,2),None)} fn main(){let mut n=0;let first=mark(inout n,1);let weak=made(inout n);first*30+n}");
        let value = original
            .functions
            .iter()
            .position(|f| matches!(f.result, ResultRepr::Value(Type::Int)) && f.params.len() == 2)
            .unwrap();
        let discard = original
            .functions
            .iter()
            .position(|f| {
                matches!(
                    f.result,
                    ResultRepr::Discarded(LogicalType::Atom(InferredType::Int))
                ) && f.params.len() == 2
            })
            .unwrap();
        assert_ne!(value, discard);
        let mut changed = original;
        changed.functions.swap(value, discard);
        reject(&changed);
    }
    #[test]
    fn malformed_deep_discard_types_are_rejected_before_recursive_consumers() {
        let mut changed = lower(SOURCE);
        let mut ty = LogicalType::Atom(InferredType::Int);
        for _ in 0..132 {
            ty = LogicalType::Nominal {
                definition: 0,
                arguments: vec![ty],
            };
        }
        discarded_body(&mut changed).result = ty;
        assert!(super::super::verify(&changed)
            .unwrap_err()
            .contains("COMPLEXITY"));
        reject(&changed);
    }
}

#[cfg(test)]
mod execution_tests {
    use super::*;
    #[test]
    fn constructor_trap_preserves_exact_mutable_source_main_slot() {
        for (first, last, expected) in
            [("mark(inout n,1)", "1/0", 1), ("1/0", "mark(inout n,2)", 0)]
        {
            let source=format!("type O<a> = None|Some(a) type W<a> = W(i32,O<a>,i32) fn mark(inout n,d){{n<-n*10+d;n}} fn made(inout n){{W({first},None,{last})}} fn main(){{let mut n=0;let weak=made(inout n);n}}");
            let ast = crate::parser::parse(&source).unwrap();
            let ir = lower_main(&crate::check::analyze(&ast).unwrap()).unwrap();
            let ExprKind::Let {
                local,
                mutable: true,
                value,
                body,
            } = &ir.functions[ir.entry].body.value_expr().unwrap().kind
            else {
                panic!()
            };
            let mut locals = CopyLocals::default();
            let mut state = EvalState::default();
            let mut owners = HashMap::new();
            let initial =
                evaluate_expr(value, &mut locals, &ir, 1, &mut state, &mut owners).unwrap();
            locals.insert(*local, initial);
            assert_eq!(
                evaluate_expr(body, &mut locals, &ir, 1, &mut state, &mut owners).unwrap_err(),
                "runtime trap"
            );
            assert_eq!(
                locals.read(local).unwrap(),
                crate::interpreter::Value::Int(expected)
            );
        }
    }
}
