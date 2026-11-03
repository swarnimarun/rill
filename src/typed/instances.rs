//! Reachable, closed function instances. Source type inference remains the
//! generic semantic check; this worklist only chooses concrete representations.
use crate::ast::Expr;
use crate::check::{CheckedProgram, Type};
use crate::source_map::{AstPath, AstStep};
use crate::syntax::AstDeclaration;
use std::collections::{HashMap, HashSet};

pub(super) type Substitution = HashMap<usize, Type>;
const MAX_INSTANCES: usize = 1024;
const MAX_TYPE_DEPTH: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    source: usize,
    params: Vec<Type>,
    result: Type,
}

pub(super) struct Instance {
    pub source: usize,
    pub params: Vec<Type>,
    pub result: Type,
    pub substitution: Substitution,
}

pub(super) struct Instances {
    pub functions: HashMap<String, usize>,
    pub list: Vec<Instance>,
    pub entry: usize,
    ids: HashMap<Key, usize>,
}

impl Instances {
    #[cfg(test)]
    pub fn discover(checked: &CheckedProgram<'_>) -> Result<Self, String> {
        Self::discover_traced(checked, None)
    }

    pub fn discover_traced(
        checked: &CheckedProgram<'_>,
        mut trace: Option<&mut Trace>,
    ) -> Result<Self, String> {
        let functions = checked
            .program
            .functions
            .iter()
            .enumerate()
            .map(|(id, f)| (f.name.clone(), id))
            .collect::<HashMap<_, _>>();
        let main = *functions.get("main").ok_or("E-TYPED: missing main")?;
        let signature = &checked.functions["main"];
        let result = substitute(&signature.result, &Substitution::new())?;
        let entry_key = Key {
            source: main,
            params: vec![],
            result,
        };
        let mut result = Self {
            functions,
            list: vec![],
            entry: 0,
            ids: HashMap::new(),
        };
        let entry = result.insert(checked, entry_key.clone())?;
        if let Some(trace) = &mut trace {
            trace.request(None, None, entry);
        }
        let mut next = 0;
        while next < result.list.len() {
            let instance = &result.list[next];
            let context = instance.substitution.clone();
            let source = instance.source;
            let function = &checked.program.functions[source];
            // A single mutable path and depth checkpoints avoid copying ancestor
            // paths at every node. Only actual request edges retain a path.
            let mut path = AstPath::root(AstDeclaration::Function(source));
            let mut pending = vec![(&function.body, 0, ChildSteps::one(AstStep::FunctionBody))];
            while let Some((expr, depth, steps)) = pending.pop() {
                if trace.as_mut().is_some_and(|trace| trace.charge(1)) {
                    path.steps.truncate(depth);
                    steps.append(&mut path.steps);
                }
                let ty = expression_type(checked, expr, &context)?;
                if let Expr::Call(name, args) = expr {
                    if let Some(&source) = result.functions.get(name) {
                        let params = args
                            .iter()
                            .map(|a| expression_type(checked, a, &context))
                            .collect::<Result<_, _>>()?;
                        let callee = result.insert(
                            checked,
                            Key {
                                source,
                                params,
                                result: ty,
                            },
                        )?;
                        if let Some(trace) = &mut trace {
                            trace.request(Some(next), Some(&path), callee);
                        }
                    }
                }
                let depth = path.steps.len();
                runtime_children_with_steps(checked, expr, |child, steps| {
                    pending.push((child, depth, steps));
                });
            }
            next += 1;
        }
        // Source order keeps existing monomorphic identities stable, while each
        // function's concrete instances retain deterministic discovery order.
        if let Some(trace) = &mut trace {
            // Sort compact source slots, never clone whole signature trees just
            // to recover the discovery-to-final permutation.
            if trace.charge(result.list.len() * 2) {
                let mut order = (0..result.list.len()).collect::<Vec<_>>();
                order.sort_by_key(|&old| result.list[old].source);
                let mut remap = vec![0; order.len()];
                for (new, old) in order.into_iter().enumerate() {
                    remap[old] = new;
                }
                for request in &mut trace.requests {
                    request.callee = remap[request.callee];
                    request.caller = request.caller.map(|old| remap[old]);
                }
            }
        }
        result.list.sort_by_key(|instance| instance.source);
        result.ids.clear();
        for (id, instance) in result.list.iter().enumerate() {
            result.ids.insert(
                Key {
                    source: instance.source,
                    params: instance.params.clone(),
                    result: instance.result.clone(),
                },
                id,
            );
        }
        result.entry = result.ids[&entry_key];
        Ok(result)
    }

    fn insert(&mut self, checked: &CheckedProgram<'_>, key: Key) -> Result<usize, String> {
        if let Some(&id) = self.ids.get(&key) {
            return Ok(id);
        }
        if self.list.len() >= MAX_INSTANCES {
            return Err("E-TYPED-INSTANCES: specialization limit exceeded".into());
        }
        let function = &checked.program.functions[key.source];
        let signature = &checked.functions[&function.name];
        if signature.params.len() != key.params.len() {
            return Err("E-TYPED-INSTANCES: wrong specialized arity".into());
        }
        let quantified = signature.quantified.iter().copied().collect();
        let mut substitution = Substitution::new();
        for (expected, actual) in signature.params.iter().zip(&key.params) {
            match_type(expected, actual, &quantified, &mut substitution, 0)?;
        }
        match_type(
            &signature.result,
            &key.result,
            &quantified,
            &mut substitution,
            0,
        )?;
        let id = self.list.len();
        self.list.push(Instance {
            source: key.source,
            params: key.params.clone(),
            result: key.result.clone(),
            substitution,
        });
        self.ids.insert(key, id);
        Ok(id)
    }

    pub fn call(&self, name: &str, params: Vec<Type>, result: Type) -> Result<usize, String> {
        let &source = self
            .functions
            .get(name)
            .ok_or("E-TYPED-UNSUPPORTED: builtin or unknown function")?;
        self.ids
            .get(&Key {
                source,
                params,
                result,
            })
            .copied()
            .ok_or_else(|| "E-TYPED-INSTANCES: missing reachable function instance".into())
    }
}

pub(super) fn expression_type(
    checked: &CheckedProgram<'_>,
    expr: &Expr,
    context: &Substitution,
) -> Result<Type, String> {
    substitute(
        checked
            .expression_type(expr)
            .ok_or("E-TYPED: missing checked expression type")?,
        context,
    )
}

pub(super) fn substitute(ty: &Type, context: &Substitution) -> Result<Type, String> {
    fn walk(ty: &Type, context: &Substitution, depth: usize) -> Result<Type, String> {
        if depth > MAX_TYPE_DEPTH {
            return Err("E-TYPED-INSTANCES: concrete type nesting exceeds 128".into());
        }
        Ok(match ty {
            Type::Var(id) => context.get(id).cloned().ok_or(
                "E-TYPED-UNSUPPORTED: unresolved polymorphic value requires local specialization",
            )?,
            Type::Nominal(name, args) => Type::Nominal(
                name.clone(),
                args.iter()
                    .map(|a| walk(a, context, depth + 1))
                    .collect::<Result<_, _>>()?,
            ),
            ty => ty.clone(),
        })
    }
    let concrete = walk(ty, context, 0)?;
    // Substituting a variable may insert an already nested concrete type.
    // Recheck the whole result before hashing it or expanding another call.
    let mut pending = vec![(&concrete, 0usize)];
    while let Some((ty, depth)) = pending.pop() {
        if depth > MAX_TYPE_DEPTH {
            return Err("E-TYPED-INSTANCES: concrete type nesting exceeds 128".into());
        }
        match ty {
            Type::Var(_) => {
                return Err("E-TYPED-UNSUPPORTED: unresolved polymorphic value requires local specialization".into());
            }
            Type::Nominal(_, args) => pending.extend(args.iter().map(|arg| (arg, depth + 1))),
            _ => {}
        }
    }
    Ok(concrete)
}

fn match_type(
    pattern: &Type,
    actual: &Type,
    quantified: &HashSet<usize>,
    context: &mut Substitution,
    depth: usize,
) -> Result<(), String> {
    if depth > MAX_TYPE_DEPTH {
        return Err("E-TYPED-INSTANCES: concrete type nesting exceeds 128".into());
    }
    match (pattern, actual) {
        (Type::Var(id), actual) if quantified.contains(id) => {
            if context.get(id).is_some_and(|previous| previous != actual) {
                return Err("E-TYPED-INSTANCES: inconsistent function substitution".into());
            }
            context.insert(*id, actual.clone());
        }
        (Type::Nominal(a, aa), Type::Nominal(b, bb)) if a == b && aa.len() == bb.len() => {
            for (a, b) in aa.iter().zip(bb) {
                match_type(a, b, quantified, context, depth + 1)?;
            }
        }
        (a, b) if a == b => {}
        _ => return Err("E-TYPED-INSTANCES: inconsistent function substitution".into()),
    }
    Ok(())
}

/// A generalized initializer is represented by its checked static shape and
/// frozen concrete leaves, not by a guessed monomorphic nominal layout. Its
/// grammar cannot contain function calls, so skipping it discovers no fewer
/// reachable function instances. Lowering independently validates that grammar.
pub(super) fn runtime_children<'a>(
    checked: &CheckedProgram<'_>,
    expr: &'a Expr,
    pending: &mut Vec<&'a Expr>,
) {
    runtime_children_with_steps(checked, expr, |child, _| pending.push(child));
}

#[derive(Clone, Copy)]
struct ChildSteps(AstStep, Option<AstStep>);
impl ChildSteps {
    fn one(step: AstStep) -> Self {
        Self(step, None)
    }
    fn append(self, path: &mut Vec<AstStep>) {
        path.push(self.0);
        path.extend(self.1);
    }
}

fn runtime_children_with_steps<'a>(
    checked: &CheckedProgram<'_>,
    expr: &'a Expr,
    mut push: impl FnMut(&'a Expr, ChildSteps),
) {
    if checked.polymorphic_local(expr).is_some() {
        if let Expr::Let(_, _, body) = expr {
            push(body, ChildSteps::one(AstStep::LetBody));
            return;
        }
    }
    // A verified direct cstr operand belongs to the native ABI, not the
    // registry of storable values. Do not skip any other expression.
    if let Expr::Call(name, args) = expr {
        if let Some(function) = checked.program.externs.iter().find(|f| f.name == *name) {
            if args.len() == function.params.len() {
                for (index, (arg, param)) in args.iter().zip(&function.params).enumerate().rev() {
                    if !matches!((arg, &param.ty), (Expr::CStr(value), crate::ast::ExternType::CStr) if !value.as_bytes().contains(&0))
                    {
                        push(arg, ChildSteps::one(AstStep::Argument(index)));
                    }
                }
                return;
            }
        }
    }
    children_with_steps(expr, push);
}

fn children_with_steps<'a>(expr: &'a Expr, mut push: impl FnMut(&'a Expr, ChildSteps)) {
    use AstStep::*;
    let one = ChildSteps::one;
    match expr {
        Expr::Binary(_, a, b) => {
            push(b, one(BinaryRight));
            push(a, one(BinaryLeft));
        }
        Expr::Let(_, a, b) | Expr::LetMut(_, a, b) => {
            push(b, one(LetBody));
            push(a, one(LetValue));
        }
        Expr::While(a, b) => {
            push(b, one(WhileBody));
            push(a, one(Condition));
        }
        Expr::If(a, b, c) => {
            push(c, one(Else));
            push(b, one(Then));
            push(a, one(Condition));
        }
        Expr::Assign(_, v) | Expr::FieldAssign(_, _, v) => push(v, one(AssignValue)),
        Expr::Call(_, args) => {
            for (i, arg) in args.iter().enumerate().rev() {
                push(arg, one(Argument(i)));
            }
        }
        Expr::Match(value, arms) => {
            for (i, arm) in arms.iter().enumerate().rev() {
                push(&arm.body, ChildSteps(Arm(i), Some(ArmBody)));
            }
            push(value, one(MatchValue));
        }
        Expr::Record(_, fields) => {
            for (i, (_, value)) in fields.iter().enumerate().rev() {
                push(value, one(RecordField(i)));
            }
        }
        _ => {}
    }
}

/// Private optional worklist observer. Failures latch while the ordinary
/// compiler continues; only the opt-in API consults them after verification.
pub(super) struct Trace {
    pub requests: Vec<Request>,
    pub failure: Option<String>,
    work: usize,
    max_requests: usize,
}
#[derive(Clone, Debug)]
pub(super) struct Request {
    pub caller: Option<usize>,
    pub path: Option<AstPath>,
    pub callee: usize,
}
impl Default for Trace {
    fn default() -> Self {
        Self::with_limits(1_000_000, 8_000_000)
    }
}
impl Trace {
    pub fn with_limits(max_requests: usize, work: usize) -> Self {
        Self {
            requests: Vec::new(),
            failure: None,
            work,
            max_requests,
        }
    }
    pub fn charge(&mut self, amount: usize) -> bool {
        if self.failure.is_some() {
            return false;
        }
        if let Some(remaining) = self.work.checked_sub(amount) {
            self.work = remaining;
            true
        } else {
            self.failure =
                Some("E-TYPED-SOURCE-MAP-LIMIT: function origin work budget exceeded".into());
            false
        }
    }
    fn request(&mut self, caller: Option<usize>, path: Option<&AstPath>, callee: usize) {
        if !self.charge(1 + path.map_or(0, |path| path.steps.len())) {
            return;
        }
        if self.requests.len() >= self.max_requests {
            self.failure =
                Some("E-TYPED-SOURCE-MAP-LIMIT: function request budget exceeded".into());
            return;
        }
        self.requests.push(Request {
            caller,
            path: path.cloned(),
            callee,
        });
    }
    pub fn check(&self) -> Result<(), String> {
        match &self.failure {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{check, parser, typed};

    #[test]
    fn discovery_trace_work_limit_has_an_exact_boundary() {
        let program = parser::parse("fn id(x) { x } fn main() { id(42) }").unwrap();
        let checked = check::analyze(&program).unwrap();
        // Entry (1), three visits (3), one retained one-step call path (2),
        // and the two compact permutation arrays for two instances (4).
        let mut exact = Trace::with_limits(2, 10);
        let instances = Instances::discover_traced(&checked, Some(&mut exact)).unwrap();
        exact.check().unwrap();
        assert_eq!(instances.list.len(), 2);
        assert_eq!(exact.requests.len(), 2);
        let mut short = Trace::with_limits(2, 9);
        let ordinary = Instances::discover_traced(&checked, Some(&mut short)).unwrap();
        assert_eq!(ordinary.list.len(), 2);
        assert!(short
            .check()
            .unwrap_err()
            .starts_with("E-TYPED-SOURCE-MAP-LIMIT:"));
    }

    #[test]
    fn worklist_matches_independent_uniform_layout_instances() {
        for source in [
            "fn id(x) { x }\nfn main() { let a = id(1); if id(true) { a + id(41) } else { 0 } }",
            "type Option<a> = None | Some(a)\nfn wrap(x) { Some(x) }\nfn main() { wrap(wrap(42)) }",
            "fn first(n, x) { if n == 0 { x } else { second(n - 1, x) } }\nfn second(n, x) { if n == 0 { x } else { first(n - 1, x) } }\nfn main() { let a = first(7, 42); if second(6, true) { a } else { 0 } }",
            "type Phantom<a> = Marker\nfn make(x) { Marker }\nfn one(x: Phantom<i32>) { 40 }\nfn two(x: Phantom<bool>) { 2 }\nfn main() { one(make(0)) + two(make(0)) }",
            "fn id(x) { x }\nfn unused(x) { x }\nfn main() { byte_len(id(\"é\")) + id(40) }",
        ] {
            let program = parser::parse(source).unwrap();
            let checked = check::analyze(&program).unwrap();
            let instances = Instances::discover(&checked).unwrap();
            let actual = instances.list.iter().map(|instance| (program.functions[instance.source].name.clone(), instance.params.clone(), instance.result.clone())).collect::<HashSet<_>>();
            assert_eq!(actual, checked.reference_instance_keys().unwrap(), "{source}");
        }
    }

    #[test]
    fn local_shapes_do_not_require_a_concrete_initializer_layout() {
        let program = parser::parse("type Option<a> = None | Some(a)\nfn main() { let n = None; if true { Some(1) } else { n } }").unwrap();
        let checked = check::analyze(&program).unwrap();
        Instances::discover(&checked).unwrap();
        typed::lower_main(&checked).unwrap();
        assert!(checked
            .reference_instance_keys()
            .unwrap_err()
            .contains("local specialization"));
    }

    #[test]
    fn concrete_instance_identity_includes_result_and_is_deterministic() {
        let source = "type Phantom<a> = Marker\nfn make(x) { Marker }\nfn one(x: Phantom<i32>) { 40 }\nfn two(x: Phantom<bool>) { 2 }\nfn main() { one(make(0)) + two(make(0)) }";
        let program = parser::parse(source).unwrap();
        let checked = check::analyze(&program).unwrap();
        let instances = Instances::discover(&checked).unwrap();
        let make = instances.functions["make"];
        let made = instances
            .list
            .iter()
            .filter(|i| i.source == make)
            .collect::<Vec<_>>();
        assert_eq!(made.len(), 2);
        assert_eq!(made[0].params, made[1].params);
        assert_ne!(made[0].result, made[1].result);
        let first = crate::backend::emit(&program).unwrap();
        for _ in 0..8 {
            assert_eq!(crate::backend::emit(&program).unwrap(), first);
        }
    }

    #[test]
    fn substitution_checks_shared_variables_and_full_inserted_nesting() {
        let mut context = Substitution::new();
        let vars = HashSet::from([0]);
        match_type(&Type::Var(0), &Type::Int, &vars, &mut context, 0).unwrap();
        assert!(match_type(&Type::Var(0), &Type::Bool, &vars, &mut context, 0).is_err());
        assert!(substitute(&Type::Var(1), &context)
            .unwrap_err()
            .contains("local specialization"));
        let mut nested = Type::Int;
        for _ in 0..128 {
            nested = Type::Nominal("Wrap".into(), vec![nested]);
        }
        context.insert(0, nested);
        substitute(&Type::Var(0), &context).unwrap();
        assert!(
            substitute(&Type::Nominal("Wrap".into(), vec![Type::Var(0)]), &context)
                .unwrap_err()
                .contains("nesting")
        );
    }

    #[test]
    fn instance_limit_deduplicates_before_rejecting_new_work() {
        let program = parser::parse("fn id(x) { x }\nfn main() { id(42) }").unwrap();
        let checked = check::analyze(&program).unwrap();
        let mut instances = Instances::discover(&checked).unwrap();
        let source = instances.functions["id"];
        for index in 0..MAX_INSTANCES - 2 {
            let ty = Type::Nominal(format!("Distinct{index}"), vec![]);
            instances
                .insert(
                    &checked,
                    Key {
                        source,
                        params: vec![ty.clone()],
                        result: ty,
                    },
                )
                .unwrap();
        }
        assert_eq!(instances.list.len(), MAX_INSTANCES);
        instances
            .insert(
                &checked,
                Key {
                    source,
                    params: vec![Type::Int],
                    result: Type::Int,
                },
            )
            .unwrap();
        assert!(instances
            .insert(
                &checked,
                Key {
                    source,
                    params: vec![Type::Bool],
                    result: Type::Bool
                }
            )
            .unwrap_err()
            .contains("specialization limit"));
    }

    #[test]
    fn typed_verifier_rejects_a_wrong_specialized_callee() {
        let program = parser::parse(
            "fn id(x) { x }\nfn main() { let a = id(42); if id(true) { a } else { 0 } }",
        )
        .unwrap();
        let checked = check::analyze(&program).unwrap();
        let mut ir = typed::lower_main(&checked).unwrap();
        let bool_id = ir
            .functions
            .iter()
            .position(|f| f.result == typed::Type::Bool)
            .unwrap();
        let typed::ExprKind::Let { value, .. } = &mut ir.functions[ir.entry].body.kind else {
            panic!()
        };
        let typed::ExprKind::Call(id, _) = &mut value.kind else {
            panic!()
        };
        *id = bool_id;
        assert!(typed::verify(&ir).is_err());
        assert!(typed::evaluate(&ir).is_err());
        assert!(typed::emit_c(&ir).is_err());
    }
    #[test]
    fn typed_verifier_rejects_same_parameter_wrong_result_instance() {
        let source = "type Phantom<a> = Marker\nfn make(x) { Marker }\nfn one(x: Phantom<i32>) { 40 }\nfn two(x: Phantom<bool>) { 2 }\nfn main() { one(make(0)) + two(make(0)) }";
        let program = parser::parse(source).unwrap();
        let checked = check::analyze(&program).unwrap();
        let mut ir = typed::lower_main(&checked).unwrap();
        let ids = ir
            .functions
            .iter()
            .enumerate()
            .filter(|(_, f)| {
                f.params.len() == 1
                    && f.params[0].1 == typed::Type::Int
                    && matches!(f.result, typed::Type::Adt { .. })
            })
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        assert_eq!(ids.len(), 2);
        assert_ne!(ir.functions[ids[0]].result, ir.functions[ids[1]].result);
        let typed::ExprKind::Binary(_, left, _) = &mut ir.functions[ir.entry].body.kind else {
            panic!()
        };
        let typed::ExprKind::Call(_, args) = &mut left.kind else {
            panic!()
        };
        let typed::CallArg::Value(value) = &mut args[0] else {
            panic!()
        };
        let typed::ExprKind::Call(id, _) = &mut value.kind else {
            panic!()
        };
        *id = if *id == ids[0] { ids[1] } else { ids[0] };
        assert!(typed::verify(&ir)
            .unwrap_err()
            .contains("incorrect expression type"));
        assert!(typed::evaluate(&ir).is_err());
        assert!(typed::emit_c(&ir).is_err());
    }
}
