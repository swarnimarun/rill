//! SCC-local nominal projection obligations. This is not structural row inference.
//! Source loan names preserve exact segment overlap while nominal types are unknown.
use super::*;

const WORK_LIMIT: usize = 1_000_000;
const NAME_BYTES_LIMIT: usize = 1024 * 1024;
const OBLIGATION_LIMIT: usize = 4096;
const LIMIT_ERROR: &str =
    "E-PROJECTION-COMPLEXITY: projection and field-loan checking budget exceeded";
const UNKNOWN_RECORD: &str =
    "E-RECORD: field access requires a known record type (annotate the parameter)";

struct Budget {
    work: usize,
    name_bytes: usize,
    obligations: usize,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            work: WORK_LIMIT,
            name_bytes: NAME_BYTES_LIMIT,
            obligations: OBLIGATION_LIMIT,
        }
    }
}
impl Budget {
    fn charge(&mut self, count: usize) -> Result<(), String> {
        self.work = self.work.checked_sub(count).ok_or(LIMIT_ERROR)?;
        Ok(())
    }
    fn name(&mut self, name: &str) -> Result<(), String> {
        self.charge(1)?;
        self.charge(name.len())
    }
    fn path(&mut self, fields: &[String]) -> Result<(), String> {
        if fields.is_empty() || fields.len() > 16 {
            return Err("E-RECORD: field path must contain 1 to 16 fields".into());
        }
        self.charge(fields.len())?;
        for field in fields {
            self.name(field)?;
        }
        Ok(())
    }
    fn retain_path(&mut self, fields: &[String]) -> Result<Rc<[String]>, String> {
        self.path(fields)?;
        let bytes = fields.iter().try_fold(0usize, |sum, field| {
            sum.checked_add(field.len()).ok_or(LIMIT_ERROR)
        })?;
        self.name_bytes = self.name_bytes.checked_sub(bytes).ok_or(LIMIT_ERROR)?;
        Ok(fields.to_vec().into())
    }
    fn schema(&mut self, field: &TypeExpr) -> Result<(), String> {
        let mut pending = vec![field];
        while let Some(node) = pending.pop() {
            // declared_type hashes each source name even when substitution
            // erases a long generic parameter name from the resulting Type.
            self.name(&node.name)?;
            self.charge(node.args.len())?;
            pending.extend(&node.args);
        }
        Ok(())
    }
    fn obligation(&mut self) -> Result<(), String> {
        self.obligations = self.obligations.checked_sub(1).ok_or(LIMIT_ERROR)?;
        self.charge(1)
    }
}

struct Obligation {
    occurrence: *const Expr,
    // Preserve the exact per-use instance; retries never instantiate again.
    root: Type,
    current: Option<Type>,
    names: Rc<[String]>,
    prefix: Vec<FieldInfo>,
    result: Type,
}
#[derive(Default)]
pub(super) struct State {
    pending: Vec<Obligation>,
    budget: Budget,
    pub(super) dirty: bool,
}

fn paths_overlap(left: &[String], right: &[String], budget: &mut Budget) -> Result<bool, String> {
    for (a, b) in left.iter().zip(right) {
        // Complete segments, not textual dotted-path prefixes. Charge even
        // long equal names before comparing their bytes.
        budget.charge(1)?;
        budget.charge(a.len().min(b.len()))?;
        if a != b {
            return Ok(false);
        }
    }
    Ok(true)
}

impl Checker {
    pub(super) fn source_loan_path(&mut self, fields: &[String]) -> Result<Rc<[String]>, String> {
        self.projections.budget.retain_path(fields)
    }

    /// Exclusive field access requires a mutable, empty-scheme root. All such
    /// paths share one monotype, hence each accepted nominal prefix has a fixed
    /// name/index correspondence. Quantified immutable roots admit shared reads
    /// only (typed local shapes use read temporaries); a whole-root operation
    /// overlaps every path regardless of its type instance. Revisit this argument
    /// before admitting quantified mutable or borrowed local schemes.
    pub(super) fn conflicts(
        &mut self,
        root: usize,
        path: &[String],
        exclusive: bool,
    ) -> Result<bool, String> {
        for loan in &self.active_loans {
            // Root-only loans retain their existing behavior; this new budget
            // covers field-path scans and symbolic segment comparisons.
            if !path.is_empty() || !loan.path.is_empty() {
                self.projections.budget.charge(1)?;
            }
            if loan.root != root || !(exclusive || loan.exclusive) {
                continue;
            }
            if paths_overlap(&loan.path, path, &mut self.projections.budget)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn projection_step(
        &mut self,
        ty: &Type,
        field: &str,
    ) -> Result<Option<(FieldInfo, Type)>, String> {
        self.projections.budget.charge(1)?;
        let resolved = self.resolve(ty)?;
        let (record, args) = match resolved {
            Type::Var(_) => return Ok(None),
            Type::Nominal(record, args) => (record, args),
            _ => return Err(UNKNOWN_RECORD.into()),
        };
        self.projections.budget.name(&record)?;
        let constructor_name = self
            .variants
            .get(&record)
            .and_then(|variants| variants.first())
            .ok_or("E-RECORD: unknown record type")?;
        self.projections.budget.name(constructor_name)?;
        let constructor = &self.constructors[constructor_name];
        let names = constructor
            .record_fields
            .as_ref()
            .ok_or("E-RECORD: field access requires a record")?;
        let mut index = None;
        for (candidate, name) in names.iter().enumerate() {
            self.projections.budget.name(name)?;
            if name == field {
                index = Some(candidate);
                break;
            }
        }
        let index = index.ok_or_else(|| format!("E-RECORD: record has no field '{field}'"))?;
        if constructor.params.len() != args.len() {
            return Err("E-RECORD: invalid nominal record arity".into());
        }
        // The temporary substitution map clones/hashes schema parameter names,
        // even if the selected field itself is primitive. Admit it first.
        for param in &constructor.params {
            self.projections.budget.name(param)?;
        }
        self.projections.budget.schema(&constructor.fields[index])?;
        let params = constructor.params.iter().cloned().zip(args).collect();
        let ty = self.declared_type(&constructor.fields[index], &params)?;
        Ok(Some((
            FieldInfo {
                record,
                index,
                arity: names.len(),
            },
            ty,
        )))
    }

    pub(super) fn record_field(
        &mut self,
        expr: &Expr,
        binding: &Scheme,
        fields: &[String],
    ) -> Result<Type, String> {
        self.projections.budget.path(fields)?;
        let root = self.instantiate(binding)?;
        let mut current = None;
        let mut prefix = Vec::with_capacity(fields.len());
        for field in fields {
            let Some((info, ty)) =
                self.projection_step(current.as_ref().unwrap_or(&root), field)?
            else {
                // Bounds precede retained clones and fresh result allocation.
                self.projections.budget.obligation()?;
                let names = self.projections.budget.retain_path(fields)?;
                let result = self.fresh();
                let returned = self.copy_type(&result)?;
                self.projections.pending.push(Obligation {
                    occurrence: expr as *const Expr,
                    root,
                    current,
                    names,
                    prefix,
                    result,
                });
                self.projections.dirty = true;
                return Ok(returned);
            };
            prefix.push(info);
            current = Some(ty);
        }
        self.field_info.insert(expr as *const Expr, prefix);
        Ok(current.expect("a validated path is nonempty"))
    }

    pub(super) fn resolve_projections(&mut self, required: bool) -> Result<(), String> {
        if self.projections.pending.is_empty() {
            self.projections.dirty = false;
            return Ok(());
        }
        if self.projections.dirty {
            loop {
                let mut solved = false;
                let pending = std::mem::take(&mut self.projections.pending);
                for mut obligation in pending {
                    self.projections.budget.charge(1)?;
                    while let Some(field) = obligation.names.get(obligation.prefix.len()) {
                        let Some((info, ty)) = self.projection_step(
                            obligation.current.as_ref().unwrap_or(&obligation.root),
                            field,
                        )?
                        else {
                            break;
                        };
                        obligation.prefix.push(info);
                        obligation.current = Some(ty);
                    }
                    if obligation.prefix.len() == obligation.names.len() {
                        self.unify(
                            obligation.current.expect("a resolved path is nonempty"),
                            obligation.result,
                        )?;
                        // Publish only complete, canonical occurrence metadata.
                        self.field_info
                            .insert(obligation.occurrence, obligation.prefix);
                        solved = true;
                    } else {
                        self.projections.pending.push(obligation);
                    }
                }
                if !solved || self.projections.pending.is_empty() {
                    break;
                }
            }
            self.projections.dirty = false;
        }
        if required && !self.projections.pending.is_empty() {
            return Err(UNKNOWN_RECORD.into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_exhaustion_is_exact_and_names_are_admitted_before_clone() {
        let mut budget = Budget {
            work: 3,
            name_bytes: 2,
            obligations: 1,
        };
        budget.charge(3).unwrap();
        assert_eq!(budget.work, 0);
        assert_eq!(budget.charge(1).unwrap_err(), LIMIT_ERROR);
        budget.work = 100;
        assert_eq!(&*budget.retain_path(&["ab".into()]).unwrap(), &["ab"]);
        assert_eq!(budget.name_bytes, 0);
        assert_eq!(budget.retain_path(&["c".into()]).unwrap_err(), LIMIT_ERROR);
        budget.obligation().unwrap();
        assert_eq!(budget.obligation().unwrap_err(), LIMIT_ERROR);
    }

    #[test]
    fn symbolic_overlap_uses_complete_segments_and_charges_equal_prefixes() {
        let mut budget = Budget::default();
        let a = vec!["a".into(), "b".into()];
        assert!(paths_overlap(&[], &a, &mut budget).unwrap());
        assert!(paths_overlap(&a, &a[..1], &mut budget).unwrap());
        assert!(!paths_overlap(&a, &["ab".into()], &mut budget).unwrap());
        assert!(!paths_overlap(&["a".into()], &["ab".into()], &mut budget).unwrap());
        let path = vec!["x".repeat(100)];
        budget.work = 101;
        assert!(paths_overlap(&path, &path, &mut budget).unwrap());
        assert_eq!(budget.work, 0);
        assert_eq!(
            paths_overlap(&path, &path, &mut budget).unwrap_err(),
            LIMIT_ERROR
        );
    }

    #[test]
    fn public_path_depth_is_checked_before_retention() {
        let mut budget = Budget::default();
        for count in [0, 17, 10_000] {
            let before = (budget.work, budget.name_bytes);
            assert!(budget
                .retain_path(&vec!["x".into(); count])
                .unwrap_err()
                .starts_with("E-RECORD"));
            assert_eq!((budget.work, budget.name_bytes), before);
        }
        budget.retain_path(&vec!["x".into(); 16]).unwrap();
    }

    #[test]
    fn unresolved_components_reject_before_caller_constraints() {
        let p = crate::parser::parse(
            "type R={value:i32} fn project(p){p.value} fn main(){project(record R{value=42})}",
        )
        .unwrap();
        assert_eq!(analyze(&p).err().unwrap(), UNKNOWN_RECORD);
    }

    #[test]
    fn deferred_generic_scc_regains_ordinary_polymorphism() {
        let f = "fn f(x){let r=g(x);let v=r.value;let alias=v;alias}";
        let g = "fn g(x){if true{record R{value=x}}else{record R{value=f(x)}}}";
        for declarations in [format!("{f} {g}"), format!("{g} {f}")] {
            let p = crate::parser::parse(&format!("type R<a> = {{value:a}} {declarations} fn main(){{let one=f(1);if f(true){{42}}else{{0}}}}")).unwrap();
            let checked = analyze(&p).unwrap();
            for name in ["f", "g"] {
                assert_eq!(checked.functions[name].quantified.len(), 1, "{name}");
            }
            assert_eq!(crate::interpreter::run(&p).unwrap().to_string(), "42");
        }
    }

    #[test]
    fn reversed_obligation_dependency_retries_to_fixed_point() {
        let source = "type I={value:i32} type O={inner:I} fn f(p,q){let first=p.value;let link=q.inner;let same=if true{p}else{link};g(p,q);first} fn g(p,q){let closed=if true{q}else{record O{inner=record I{value=0}}};if false{f(p,q)}else{0}} fn main(){f(record I{value=42},record O{inner=record I{value=42}})}";
        let p = crate::parser::parse(source).unwrap();
        let checked = analyze(&p).unwrap();
        let lowered = crate::typed::lower_main(&checked).unwrap();
        crate::typed::verify(&lowered).unwrap();
        assert_eq!(crate::typed::evaluate(&lowered).unwrap().to_string(), "42");
    }

    #[test]
    fn long_schema_parameter_names_are_charged_even_for_primitive_fields() {
        let mut p =
            crate::parser::parse("type R<a> = {value:i32} fn f(p:R<i32>){p.value} fn main(){42}")
                .unwrap();
        p.types[0].params[0] = "a".repeat(WORK_LIMIT + 1);
        assert_eq!(analyze(&p).err().unwrap(), LIMIT_ERROR);
    }

    #[test]
    fn repeated_long_schema_names_are_charged_before_small_substituted_output() {
        let mut p = crate::parser::parse("type Many<a> = Empty type R<a> = {value:Many<a>} fn f(p:R<i32>){p.value} fn main(){42}").unwrap();
        let width = 2000;
        let long = "a".repeat(1000);
        p.types[0].params = (0..width).map(|i| format!("x{i}")).collect();
        p.types[1].params[0] = long.clone();
        p.types[1].variants[0].fields[0].args = (0..width)
            .map(|_| TypeExpr {
                name: long.clone(),
                args: vec![],
            })
            .collect();
        assert_eq!(analyze(&p).err().unwrap(), LIMIT_ERROR);
    }

    #[test]
    fn hostile_public_names_and_wide_pending_obligations_reject_atomically() {
        let mut p = crate::parser::parse("fn consume(){42} fn f(p){p.x} fn main(){42}").unwrap();
        let Expr::Field(_, fields) = &mut p.functions[1].body else {
            unreachable!()
        };
        fields[0] = "x".repeat(WORK_LIMIT + 1);
        assert_eq!(analyze(&p).err().unwrap(), LIMIT_ERROR);
        p.functions[0].params = (0..=OBLIGATION_LIMIT)
            .map(|i| crate::ast::Param {
                name: format!("p{i}"),
                mode: ParamMode::Value,
                annotation: Some(TypeExpr {
                    name: "i32".into(),
                    args: vec![],
                }),
            })
            .collect();
        p.functions[1].body = Expr::Call(
            "consume".into(),
            (0..=OBLIGATION_LIMIT)
                .map(|_| Expr::Field("p".into(), vec!["x".into()]))
                .collect(),
        );
        assert_eq!(analyze(&p).err().unwrap(), LIMIT_ERROR);
        // Failure returned no CheckedProgram. A new independent analysis still
        // has a complete fresh budget and cannot observe provisional metadata.
        let valid =
            crate::parser::parse("type R={x:i32} fn main(){let r=record R{x=42};r.x}").unwrap();
        assert!(analyze(&valid).is_ok());
    }
}

#[cfg(test)]
mod independent_tests {
    use super::*;

    fn checker() -> Checker {
        Checker {
            allocation_types: None,
            solver: solver::Solver::default(),
            level: 1,
            functions: HashMap::new(),
            parameter_modes: HashMap::new(),
            next_local: 0,
            active_loans: Vec::new(),
            projections: State::default(),
            constructors: HashMap::new(),
            variants: HashMap::new(),
            pending_matches: Vec::new(),
            equality_types: Vec::new(),
            pointer_types: Vec::new(),
            constants: HashMap::new(),
            expression_types: HashMap::new(),
            field_info: HashMap::new(),
            record_info: HashMap::new(),
            polymorphic_locals: HashMap::new(),
            local_uses: HashMap::new(),
            has_owned: false,
        }
    }

    fn field_type(name: &str) -> TypeExpr {
        TypeExpr {
            name: name.into(),
            args: Vec::new(),
        }
    }

    fn record(checker: &mut Checker, name: &str, parameters: &[&str], fields: &[(&str, &str)]) {
        checker.variants.insert(name.into(), vec![name.into()]);
        checker.constructors.insert(
            name.into(),
            Constructor {
                owner: name.into(),
                params: parameters.iter().map(|name| (*name).into()).collect(),
                fields: fields.iter().map(|(_, ty)| field_type(ty)).collect(),
                record_fields: Some(fields.iter().map(|(name, _)| (*name).into()).collect()),
            },
        );
    }

    fn monotype(ty: Type) -> Scheme {
        Scheme {
            quantified: Vec::new(),
            ty,
            mutable: false,
            borrowed: false,
            local_id: 7,
        }
    }

    #[test]
    fn independent_quantified_roots_cannot_carry_mutation_or_borrowed_authority() {
        let mut checker = checker();
        let ty = checker.fresh();
        let mut binding = monotype(ty);
        binding.quantified = vec![0];
        assert!(checker.copy_scheme(&binding).is_ok());
        for (mutable, borrowed) in [(true, false), (false, true), (true, true)] {
            binding.mutable = mutable;
            binding.borrowed = borrowed;
            assert_eq!(
                checker.copy_scheme(&binding).err().unwrap(),
                "E-INTERNAL-TYPE: quantified mutable or borrowed local"
            );
        }
    }

    #[test]
    fn independent_read_operand_has_one_obligation_and_one_scheme_instance() {
        let mut checker = checker();
        record(&mut checker, "R", &[], &[("value", "i32")]);
        let template = checker.fresh();
        assert_eq!(template, Type::Var(0));
        let mut binding = monotype(template.clone());
        binding.quantified = vec![0];
        checker.functions.insert(
            "inspect".into(),
            FunctionType {
                quantified: Vec::new(),
                params: vec![Type::Int],
                result: Type::Int,
            },
        );
        checker
            .parameter_modes
            .insert("inspect".into(), vec![ParamMode::Read]);
        let mut locals = HashMap::from([("p".into(), binding)]);
        let expression = Expr::Call(
            "inspect".into(),
            vec![Expr::Field("p".into(), vec!["value".into()])],
        );
        checker.infer(&expression, &mut locals).unwrap();
        let Expr::Call(_, args) = &expression else {
            unreachable!()
        };
        let argument = &args[0];
        assert_eq!(checker.projections.pending.len(), 1);
        assert_eq!(
            checker.projections.pending[0].occurrence,
            argument as *const Expr
        );
        // One new instance and one projected result, with no preliminary read
        // lookup allocating another pair of variables or another obligation.
        assert_eq!(checker.fresh(), Type::Var(3));
        assert!(checker.active_loans.is_empty());
        assert!(!checker.field_info.contains_key(&(argument as *const Expr)));
        assert_eq!(checker.projections.pending[0].root, Type::Var(1));
        checker
            .unify(Type::Var(1), Type::Nominal("R".into(), Vec::new()))
            .unwrap();
        checker.resolve_projections(true).unwrap();
        assert_eq!(
            checker.resolve(&template).unwrap(),
            template,
            "the quantified template was mutated"
        );
        assert_eq!(
            checker
                .resolve(&checker.expression_types[&(argument as *const Expr)])
                .unwrap(),
            Type::Int
        );
        let info = &checker.field_info[&(argument as *const Expr)];
        assert_eq!(info.len(), 1);
        assert_eq!(
            (info[0].record.as_str(), info[0].index, info[0].arity),
            ("R", 0, 1)
        );
        assert!(checker.projections.pending.is_empty());
    }

    #[test]
    fn independent_partial_path_retries_only_its_unresolved_suffix() {
        let mut checker = checker();
        record(&mut checker, "R", &["a"], &[("pad", "i32"), ("child", "a")]);
        record(&mut checker, "I", &[], &[("value", "i32")]);
        let child = checker.fresh();
        let binding = monotype(Type::Nominal("R".into(), vec![child.clone()]));
        let expression = Expr::Field("p".into(), vec!["child".into(), "value".into()]);
        let Expr::Field(_, path) = &expression else {
            unreachable!()
        };
        let result = checker.record_field(&expression, &binding, path).unwrap();
        assert_eq!(checker.projections.pending.len(), 1);
        assert_eq!(checker.projections.pending[0].prefix.len(), 1);
        assert_eq!(checker.projections.pending[0].prefix[0].index, 1);
        checker.resolve_projections(false).unwrap();
        assert_eq!(checker.projections.pending[0].prefix.len(), 1);
        assert_eq!(checker.projections.pending[0].current, Some(child.clone()));
        assert!(!checker
            .field_info
            .contains_key(&(&expression as *const Expr)));
        checker
            .unify(child, Type::Nominal("I".into(), Vec::new()))
            .unwrap();
        // Exactly: one obligation + one step + two bytes each for record and
        // constructor identity + six for 'value' + four for schema name 'i32'.
        // Replaying the already resolved R.child prefix cannot fit this budget.
        checker.projections.budget.work = 16;
        checker.resolve_projections(true).unwrap();
        assert_eq!(checker.projections.budget.work, 0);
        assert_eq!(checker.resolve(&result).unwrap(), Type::Int);
        let info = &checker.field_info[&(&expression as *const Expr)];
        assert_eq!(info.len(), 2);
        assert_eq!(
            (info[0].record.as_str(), info[0].index, info[0].arity),
            ("R", 1, 2)
        );
        assert_eq!(
            (info[1].record.as_str(), info[1].index, info[1].arity),
            ("I", 0, 1)
        );
    }

    #[test]
    fn independent_unchanged_checkpoints_do_not_rescan_pending_work() {
        let mut checker = checker();
        record(&mut checker, "R", &[], &[("value", "i32")]);
        let root = checker.fresh();
        let binding = monotype(root.clone());
        let expression = Expr::Field("p".into(), vec!["value".into()]);
        let Expr::Field(_, path) = &expression else {
            unreachable!()
        };
        checker.record_field(&expression, &binding, path).unwrap();
        checker.resolve_projections(false).unwrap();
        assert!(!checker.projections.dirty);
        let remaining = checker.projections.budget.work;
        for _ in 0..100 {
            checker.resolve_projections(false).unwrap();
        }
        assert_eq!(checker.projections.budget.work, remaining);
        assert_eq!(checker.projections.pending.len(), 1);
        checker
            .unify(root, Type::Nominal("R".into(), Vec::new()))
            .unwrap();
        assert!(checker.projections.dirty);
        checker.resolve_projections(true).unwrap();
        assert!(checker.projections.pending.is_empty());
        assert!(!checker.projections.dirty);
    }

    #[test]
    fn independent_failed_admission_allocates_no_projected_result_or_metadata() {
        for fail_names in [false, true] {
            let mut checker = checker();
            let root = checker.fresh();
            let binding = monotype(root);
            if fail_names {
                checker.projections.budget.name_bytes = 0;
            } else {
                checker.projections.budget.obligations = 0;
            }
            let expression = Expr::Field("p".into(), vec!["value".into()]);
            let Expr::Field(_, path) = &expression else {
                unreachable!()
            };
            assert_eq!(
                checker
                    .record_field(&expression, &binding, path)
                    .unwrap_err(),
                LIMIT_ERROR
            );
            assert_eq!(checker.fresh(), Type::Var(1));
            assert!(checker.projections.pending.is_empty());
            assert!(checker.field_info.is_empty());
        }
    }

    #[test]
    fn independent_schema_budget_charges_names_and_scheduled_children_exactly() {
        let field = TypeExpr {
            name: "Tree".into(),
            args: vec![field_type("a"), field_type("b")],
        };
        let mut budget = Budget {
            work: 11,
            name_bytes: 0,
            obligations: 0,
        };
        budget.schema(&field).unwrap();
        assert_eq!(budget.work, 0);
        let mut budget = Budget {
            work: 10,
            name_bytes: 0,
            obligations: 0,
        };
        assert_eq!(budget.schema(&field).unwrap_err(), LIMIT_ERROR);
    }
}
