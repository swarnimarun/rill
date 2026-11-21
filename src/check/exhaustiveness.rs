//! Bounded coverage proof for already type-checked patterns. Coverage ignores
//! binding names and arm order; the AST retains both for runtime first match.
use super::{
    type_cursor::{TypeCursor, TypeHead},
    Checker, Type,
};
use crate::ast::{Pattern, TypeExpr};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

const WORK_LIMIT: usize = 100_000;
const LIMIT_ERROR: &str =
    "E-MATCH-COMPLEXITY: pattern analysis exceeded 100000 work units; simplify the match";
type Id = usize;
const WILD: Id = 0;

struct Budget(usize);
impl Budget {
    fn spend(&mut self, amount: usize) -> Result<(), String> {
        self.0 = self.0.checked_sub(amount).ok_or(LIMIT_ERROR)?;
        Ok(())
    }
    fn cells(&mut self, rows: usize, columns: usize) -> Result<(), String> {
        self.spend(rows.checked_mul(columns).ok_or(LIMIT_ERROR)?)
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
enum Pat<'a> {
    Wild,
    Bool(bool),
    Unit,
    // Integer literals do not cover the default integer domain.
    Literal,
    Constructor(&'a str, Vec<Id>),
}
#[derive(Clone, Hash, PartialEq, Eq)]
enum Ty<'a> {
    Bool,
    Unit,
    Other,
    Nominal(&'a str, Vec<Id>),
}
struct Arena<T> {
    values: Vec<T>,
    ids: HashMap<T, Id>,
}
impl<T: Clone + Eq + Hash> Arena<T> {
    fn new() -> Self {
        Self {
            values: Vec::new(),
            ids: HashMap::new(),
        }
    }
    // Include both retained copies and the lookup, even on a cache hit.
    fn intern(&mut self, value: T, width: usize, budget: &mut Budget) -> Result<Id, String> {
        budget.cells(3, width + 1)?;
        if let Some(&id) = self.ids.get(&value) {
            return Ok(id);
        }
        let id = self.values.len();
        self.values.push(value.clone());
        self.ids.insert(value, id);
        Ok(id)
    }
}
#[derive(Clone, Hash, PartialEq, Eq)]
struct Matrix {
    types: Vec<Id>,
    rows: Vec<Vec<Id>>,
}
struct Proof<'a> {
    checker: &'a Checker,
    budget: Budget,
    patterns: Arena<Pat<'a>>,
    types: Arena<Ty<'a>>,
    #[cfg(test)]
    cache_hits: usize,
}

impl Checker {
    pub(super) fn exhaustive(&self, ty: &Type, patterns: &[Pattern]) -> Result<bool, String> {
        Proof::new(self).check(ty, patterns)
    }
}
impl<'a> Proof<'a> {
    fn new(checker: &'a Checker) -> Self {
        Self {
            checker,
            budget: Budget(WORK_LIMIT),
            patterns: Arena::new(),
            types: Arena::new(),
            #[cfg(test)]
            cache_hits: 0,
        }
    }
    fn check(&mut self, ty: &'a Type, patterns: &'a [Pattern]) -> Result<bool, String> {
        self.budget.spend(patterns.len())?;
        if patterns
            .iter()
            .any(|p| matches!(p, Pattern::Wildcard | Pattern::Binding(_)))
        {
            return Ok(true);
        }
        assert_eq!(self.patterns.intern(Pat::Wild, 0, &mut self.budget)?, WILD);
        let rows = patterns
            .iter()
            .map(|p| self.pattern(p).map(|id| vec![id]))
            .collect::<Result<_, _>>()?;
        let ty = self.source_type(ty)?;
        self.solve(Matrix {
            types: vec![ty],
            rows,
        })
    }
    fn pattern(&mut self, pattern: &'a Pattern) -> Result<Id, String> {
        enum Task<'a> {
            Visit(&'a Pattern),
            Finish(&'a str, usize),
        }
        let mut pending = vec![Task::Visit(pattern)];
        let mut values = Vec::new();
        while let Some(task) = pending.pop() {
            self.budget.spend(1)?;
            let node = match task {
                Task::Visit(Pattern::Constructor(name, fields)) => {
                    self.budget.spend(fields.len())?;
                    pending.push(Task::Finish(name, fields.len()));
                    pending.extend(fields.iter().rev().map(Task::Visit));
                    continue;
                }
                Task::Visit(Pattern::Wildcard | Pattern::Binding(_)) => Pat::Wild,
                Task::Visit(Pattern::Bool(b)) => Pat::Bool(*b),
                Task::Visit(Pattern::Unit) => Pat::Unit,
                Task::Visit(Pattern::Int(_)) => Pat::Literal,
                Task::Finish(name, count) => {
                    self.budget.spend(count)?;
                    Pat::Constructor(name, values.split_off(values.len() - count))
                }
            };
            let width = match &node {
                Pat::Constructor(name, fields) => name.len() + fields.len(),
                _ => 0,
            };
            values.push(self.patterns.intern(node, width, &mut self.budget)?);
        }
        Ok(values[0])
    }
    // Inspect only borrowed heads. A solver DAG can be much smaller than its
    // expanded public Type, so charge BEFORE scheduling its child cursors.
    fn source_type(&mut self, ty: &'a Type) -> Result<Id, String> {
        enum Task<'a> {
            Visit(TypeCursor<'a>),
            Finish(&'a str, usize),
        }
        let mut pending = vec![Task::Visit(TypeCursor::Public(ty))];
        let mut values = Vec::new();
        while let Some(task) = pending.pop() {
            self.budget.spend(1)?;
            let node = match task {
                Task::Visit(cursor) => {
                    match self.checker.type_head(cursor, || self.budget.spend(1))? {
                        TypeHead::Variable(_representative) => Ty::Other,
                        TypeHead::Atom(Type::Bool) => Ty::Bool,
                        TypeHead::Atom(Type::Unit) => Ty::Unit,
                        TypeHead::Atom(_) | TypeHead::Callable(_, _) => Ty::Other,
                        TypeHead::Nominal(name, children) => {
                            let count = children.len();
                            self.budget.spend(count)?;
                            pending.push(Task::Finish(name, count));
                            for index in (0..count).rev() {
                                pending.push(Task::Visit(
                                    children.get(index).expect("index below child count"),
                                ));
                            }
                            continue;
                        }
                    }
                }
                Task::Finish(name, count) => {
                    self.budget.spend(count)?;
                    Ty::Nominal(name, values.split_off(values.len() - count))
                }
            };
            values.push(self.intern_type(node)?);
        }
        Ok(values[0])
    }
    fn declared_type(&mut self, ty: &'a TypeExpr, vars: &HashMap<&str, Id>) -> Result<Id, String> {
        enum Task<'a> {
            Visit(&'a TypeExpr),
            Finish(&'a str, usize),
        }
        let mut pending = vec![Task::Visit(ty)];
        let mut values = Vec::new();
        while let Some(task) = pending.pop() {
            self.budget.spend(1)?;
            let node = match task {
                Task::Visit(ty) => {
                    self.budget.spend(ty.name.len())?;
                    if let Some(&id) = vars.get(ty.name.as_str()) {
                        values.push(id);
                        continue;
                    }
                    match ty.name.as_str() {
                        "bool" => Ty::Bool,
                        "unit" => Ty::Unit,
                        name if self.checker.variants.contains_key(name) => {
                            self.budget.spend(ty.args.len())?;
                            pending.push(Task::Finish(name, ty.args.len()));
                            pending.extend(ty.args.iter().rev().map(Task::Visit));
                            continue;
                        }
                        _ => Ty::Other,
                    }
                }
                Task::Finish(name, count) => {
                    self.budget.spend(count)?;
                    Ty::Nominal(name, values.split_off(values.len() - count))
                }
            };
            values.push(self.intern_type(node)?);
        }
        Ok(values[0])
    }
    fn intern_type(&mut self, node: Ty<'a>) -> Result<Id, String> {
        let width = match &node {
            Ty::Nominal(name, args) => name.len() + args.len(),
            _ => 0,
        };
        self.types.intern(node, width, &mut self.budget)
    }
    fn solve(&mut self, initial: Matrix) -> Result<bool, String> {
        let mut pending = vec![initial];
        let mut seen = HashSet::new();
        while let Some(mut matrix) = pending.pop() {
            self.budget.spend(1)?;
            let width = matrix.types.len();
            self.budget.cells(matrix.rows.len(), width + 1)?;
            if matrix.rows.iter().any(|row| row.iter().all(|&p| p == WILD)) {
                continue;
            }
            if matrix.rows.is_empty() {
                return Ok(false);
            }
            // Validated finite nominal types are inhabited. A column wild in
            // every row therefore has no effect on this coverage obligation.
            self.budget.cells(matrix.rows.len() + 1, width)?;
            let keep: Vec<_> = (0..width)
                .map(|i| matrix.rows.iter().any(|row| row[i] != WILD))
                .collect();
            for row in &mut matrix.rows {
                let mut i = 0;
                row.retain(|_| {
                    i += 1;
                    keep[i - 1]
                });
            }
            let mut i = 0;
            matrix.types.retain(|_| {
                i += 1;
                keep[i - 1]
            });
            let width = matrix.types.len();
            let rounds = usize::BITS as usize - matrix.rows.len().leading_zeros() as usize;
            self.budget
                .cells(matrix.rows.len(), (width + 1) * (rounds + 4))?;
            matrix.rows.sort_unstable();
            matrix.rows.dedup();
            if !seen.insert(matrix.clone()) {
                #[cfg(test)]
                {
                    self.cache_hits += 1;
                }
                continue;
            }
            let first = matrix.types[0];
            match &self.types.values[first] {
                Ty::Nominal(name, args) => {
                    let name = *name;
                    self.budget.spend(name.len() + args.len())?;
                    let args = args.clone();
                    for constructor in &self.checker.variants[name] {
                        self.budget.spend(1 + constructor.len())?;
                        let info = &self.checker.constructors[constructor];
                        self.budget.spend(info.params.len() + info.fields.len())?;
                        for param in &info.params {
                            self.budget.spend(param.len())?;
                        }
                        let vars = info
                            .params
                            .iter()
                            .map(String::as_str)
                            .zip(args.iter().copied())
                            .collect();
                        let fields = info
                            .fields
                            .iter()
                            .map(|f| self.declared_type(f, &vars))
                            .collect::<Result<Vec<_>, _>>()?;
                        pending.push(self.specialize(&matrix, Some(constructor), None, &fields)?);
                    }
                }
                Ty::Bool => {
                    for value in [false, true] {
                        pending.push(self.specialize(&matrix, None, Some(value), &[])?);
                    }
                }
                Ty::Unit | Ty::Other => pending.push(self.specialize(&matrix, None, None, &[])?),
            }
        }
        Ok(true)
    }
    fn specialize(
        &mut self,
        matrix: &Matrix,
        constructor: Option<&str>,
        boolean: Option<bool>,
        fields: &[Id],
    ) -> Result<Matrix, String> {
        let width = fields.len() + matrix.types.len() - 1;
        // Charge all candidate storage, including rows later rejected.
        self.budget.cells(matrix.rows.len() + 1, width + 1)?;
        let mut types = fields.to_vec();
        types.extend_from_slice(&matrix.types[1..]);
        let mut rows = Vec::new();
        for row in &matrix.rows {
            let mut next = match &self.patterns.values[row[0]] {
                Pat::Wild => vec![WILD; fields.len()],
                Pat::Constructor(name, patterns) => {
                    self.budget.spend(name.len())?;
                    if constructor != Some(*name) {
                        continue;
                    }
                    patterns.clone()
                }
                Pat::Bool(value) if boolean == Some(*value) => Vec::new(),
                Pat::Unit if self.types.values[matrix.types[0]] == Ty::Unit => Vec::new(),
                _ => continue,
            };
            next.extend_from_slice(&row[1..]);
            rows.push(next);
        }
        Ok(Matrix { types, rows })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn checker() -> Checker {
        Checker {
            allocation_types: None,
            solver: super::super::solver::Solver::default(),
            level: 0,
            functions: HashMap::new(),
            source_functions: HashSet::new(),
            parameter_modes: HashMap::new(),
            next_local: 0,
            active_loans: Vec::new(),
            projections: crate::check::projections::State::default(),
            constructors: HashMap::new(),
            variants: HashMap::new(),
            pending_matches: Vec::new(),
            equality_types: Vec::new(),
            pointer_types: Vec::new(),
            constants: HashMap::new(),
            expression_types: HashMap::new(),
            application_types: HashMap::new(),
            expression_sites: HashMap::new(),
            closures: HashMap::new(),
            field_info: HashMap::new(),
            record_info: HashMap::new(),
            polymorphic_locals: HashMap::new(),
            local_uses: HashMap::new(),
            has_owned: false,
        }
    }
    #[test]
    fn catchall_needs_one_unit_without_inspecting_a_shared_dag() {
        const CHILD: &str = "RILL_COVERAGE_DAG_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "check::exhaustiveness::tests::catchall_needs_one_unit_without_inspecting_a_shared_dag"])
                .env(CHILD, "1")
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn().unwrap();
            let start = std::time::Instant::now();
            loop {
                if child.try_wait().unwrap().is_some() {
                    let output = child.wait_with_output().unwrap();
                    assert!(output.status.success(), "DAG child failed: {output:?}");
                    return;
                }
                if start.elapsed() > std::time::Duration::from_secs(30) {
                    let _ = child.kill();
                    let output = child.wait_with_output().unwrap();
                    panic!("DAG proof exceeded safety deadline: {output:?}");
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        let mut checker = checker();
        let mut ty = checker.fresh();
        checker.unify(ty.clone(), Type::Bool).unwrap();
        for _ in 0..80 {
            let parent = checker.fresh();
            checker
                .unify(parent.clone(), Type::Nominal("Pair".into(), vec![ty; 2]))
                .unwrap();
            ty = parent;
        }
        for pattern in [Pattern::Wildcard, Pattern::Binding("name".into())] {
            let patterns = [pattern];
            let mut proof = Proof::new(&checker);
            proof.budget = Budget(1);
            assert!(proof.check(&ty, &patterns).unwrap());
            assert_eq!(proof.budget.0, 0);
            assert!(proof.types.values.is_empty());
            assert!(proof.patterns.values.is_empty());
        }
        let mut proof = Proof::new(&checker);
        assert_eq!(proof.source_type(&ty).unwrap_err(), LIMIT_ERROR);
        assert!(proof.types.values.len() < 81);
    }
    #[test]
    fn cursor_resolves_aliases_open_variables_and_ordered_arguments() {
        let mut checker = checker();
        let bool_ty = checker.fresh();
        let bool_alias = checker.fresh();
        checker.unify(bool_ty.clone(), bool_alias.clone()).unwrap();
        checker.unify(bool_alias, Type::Bool).unwrap();
        let open_ty = checker.fresh();
        let alias_open = checker.fresh();
        checker.unify(open_ty.clone(), alias_open.clone()).unwrap();
        let nominal = Type::Nominal("Pair".into(), vec![bool_ty.clone(), Type::Unit]);
        let bound_nominal = checker.fresh();
        checker
            .unify(bound_nominal.clone(), nominal.clone())
            .unwrap();
        let Type::Var(open_id) = open_ty else {
            panic!("expected variable")
        };
        assert!(matches!(
            checker.type_head(TypeCursor::Public(&alias_open), || Ok(())),
            Ok(TypeHead::Variable(id)) if id == open_id
        ));
        let mut proof = Proof::new(&checker);
        let b = proof.source_type(&bool_ty).unwrap();
        let o = proof.source_type(&open_ty).unwrap();
        assert!(matches!(proof.types.values[b], Ty::Bool));
        assert!(matches!(proof.types.values[o], Ty::Other));
        assert_eq!(proof.source_type(&alias_open).unwrap(), o);
        let p = proof.source_type(&nominal).unwrap();
        assert_eq!(proof.source_type(&bound_nominal).unwrap(), p);
        let Ty::Nominal("Pair", args) = &proof.types.values[p] else {
            panic!()
        };
        assert!(matches!(proof.types.values[args[0]], Ty::Bool));
        assert!(matches!(proof.types.values[args[1]], Ty::Unit));
    }
    #[test]
    fn alias_lookup_is_charged_before_each_hidden_step() {
        let mut checker = checker();
        let root = checker.fresh();
        checker.unify(root.clone(), Type::Bool).unwrap();
        // Public head, root parent, root binding, private atom: four charges.
        let mut budget = Budget(3);
        assert!(matches!(
            checker.type_head(TypeCursor::Public(&root), || budget.spend(1)),
            Err(error) if error == LIMIT_ERROR
        ));
        assert_eq!(budget.0, 0);
        let mut budget = Budget(4);
        assert!(matches!(
            checker.type_head(TypeCursor::Public(&root), || budget.spend(1)),
            Ok(TypeHead::Atom(Type::Bool))
        ));
        assert_eq!(budget.0, 0);
    }
    #[test]
    fn repeated_ranked_aliases_cannot_escape_the_proof_budget() {
        let mut checker = checker();
        let count = 1 << 14;
        let aliases: Vec<_> = (0..count).map(|_| checker.fresh()).collect();
        let mut width = 1;
        while width < count {
            for start in (0..count).step_by(width * 2) {
                checker
                    .unify(aliases[start].clone(), aliases[start + width].clone())
                    .unwrap();
            }
            width *= 2;
        }
        checker.unify(aliases[0].clone(), Type::Bool).unwrap();
        // The last leaf retains a rank-deep parent path. Repetition must charge
        // that path on every visit, not just charge the shallow final boolean.
        let root = Type::Nominal("Phantom".into(), vec![aliases[count - 1].clone(); 8_000]);
        let mut proof = Proof::new(&checker);
        assert_eq!(proof.source_type(&root).unwrap_err(), LIMIT_ERROR);
        assert_eq!(proof.budget.0, 0);
    }
    #[test]
    fn equivalent_constructor_suffixes_share_one_state() {
        let mut checker = checker();
        checker
            .variants
            .insert("T".into(), vec!["A".into(), "B".into()]);
        for name in ["A", "B"] {
            checker.constructors.insert(
                name.into(),
                super::super::Constructor {
                    owner: "T".into(),
                    params: vec![],
                    fields: vec![TypeExpr {
                        name: "bool".into(),
                        args: vec![],
                    }],
                    record_fields: None,
                },
            );
        }
        let ty = Type::Nominal("T".into(), vec![]);
        let patterns = [
            Pattern::Constructor("A".into(), vec![Pattern::Bool(false)]),
            Pattern::Constructor("A".into(), vec![Pattern::Bool(true)]),
            Pattern::Constructor("B".into(), vec![Pattern::Bool(true)]),
            Pattern::Constructor("B".into(), vec![Pattern::Bool(false)]),
        ];
        let mut proof = Proof::new(&checker);
        assert!(proof.check(&ty, &patterns).unwrap());
        assert_eq!(proof.cache_hits, 1);
    }
    #[test]
    fn budget_has_an_exact_boundary_and_rejects_overflow() {
        let mut budget = Budget(WORK_LIMIT);
        budget.spend(WORK_LIMIT).unwrap();
        assert_eq!(budget.spend(1).unwrap_err(), LIMIT_ERROR);
        assert_eq!(
            Budget(WORK_LIMIT).cells(usize::MAX, 2).unwrap_err(),
            LIMIT_ERROR
        );
    }
}
