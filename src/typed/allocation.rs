//! Canonical nominal contracts and recoverable hosted allocation.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemAllocationKind {
    BoxI32,
    BytesZeroed,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AllocationResultLayout {
    pub result_adt: usize,
    pub ok_variant: usize,
    pub err_variant: usize,
    pub error_adt: usize,
    pub invalid_length_variant: usize,
    pub size_overflow_variant: usize,
    pub out_of_memory_variant: usize,
}
#[derive(Clone, Debug)]
pub struct AllocationContract {
    pub result_name: String,
    pub ok_name: String,
    pub err_name: String,
    pub error_name: String,
    pub invalid_length_name: String,
    pub size_overflow_name: String,
    pub out_of_memory_name: String,
}
impl From<&crate::ast::AllocationTypes> for AllocationContract {
    fn from(m: &crate::ast::AllocationTypes) -> Self {
        Self {
            result_name: m.result_type.clone(),
            ok_name: m.ok_constructor.clone(),
            err_name: m.err_constructor.clone(),
            error_name: m.error_type.clone(),
            invalid_length_name: m.invalid_length_constructor.clone(),
            size_overflow_name: m.size_overflow_constructor.clone(),
            out_of_memory_name: m.out_of_memory_constructor.clone(),
        }
    }
}
pub(super) fn resolve(
    ty: Type,
    adts: &[AdtType],
    c: &AllocationContract,
) -> Result<AllocationResultLayout, String> {
    let Type::Adt { id: result_adt, .. } = ty else {
        return Err("E-TYPED-ALLOC: expected Result".into());
    };
    let result = adts
        .get(result_adt)
        .ok_or("E-TYPED-ALLOC: missing Result")?;
    let Some(Type::Adt { id: error_adt, .. }) = result.arguments.get(1) else {
        return Err("E-TYPED-ALLOC: missing AllocError".into());
    };
    let error = adts
        .get(*error_adt)
        .ok_or("E-TYPED-ALLOC: missing AllocError")?;
    let find = |adt: &AdtType, name: &str| {
        adt.variants
            .iter()
            .position(|v| v.name == name)
            .ok_or_else(|| "E-TYPED-ALLOC: missing canonical constructor".to_string())
    };
    Ok(AllocationResultLayout {
        result_adt,
        error_adt: *error_adt,
        ok_variant: find(result, &c.ok_name)?,
        err_variant: find(result, &c.err_name)?,
        invalid_length_variant: find(error, &c.invalid_length_name)?,
        size_overflow_variant: find(error, &c.size_overflow_name)?,
        out_of_memory_variant: find(error, &c.out_of_memory_name)?,
    })
}
pub(super) fn validate(
    kind: SystemAllocationKind,
    l: &AllocationResultLayout,
    adts: &[AdtType],
    contract: Option<&AllocationContract>,
) -> Result<(), String> {
    let bad = || "E-TYPED-ALLOC: invalid canonical allocation layout".to_string();
    let c = contract.ok_or_else(bad)?;
    let identities = [
        &c.result_name,
        &c.error_name,
        &c.ok_name,
        &c.err_name,
        &c.invalid_length_name,
        &c.size_overflow_name,
        &c.out_of_memory_name,
    ];
    if identities.iter().any(|n| n.is_empty())
        || identities.iter().collect::<HashSet<_>>().len() != 7
    {
        return Err(bad());
    }
    let result = adts.get(l.result_adt).ok_or_else(bad)?;
    let error = adts.get(l.error_adt).ok_or_else(bad)?;
    let owner = match kind {
        SystemAllocationKind::BoxI32 => Type::OwnedI32,
        SystemAllocationKind::BytesZeroed => Type::OwnedBytes,
    };
    let error_ty = Type::Adt {
        id: l.error_adt,
        owned: false,
    };
    if result.name != c.result_name
        || result.arguments != [owner, error_ty]
        || !result.owned
        || result.variants.len() != 2
        || error.name != c.error_name
        || !error.arguments.is_empty()
        || error.owned
        || error.variants.len() != 3
        || l.ok_variant == l.err_variant
    {
        return Err(bad());
    }
    for (index, name, fields) in [
        (l.ok_variant, &c.ok_name, vec![owner]),
        (l.err_variant, &c.err_name, vec![error_ty]),
    ] {
        let v = result.variants.get(index).ok_or_else(bad)?;
        if &v.name != name || v.fields != fields {
            return Err(bad());
        }
    }
    let errors = [
        (l.invalid_length_variant, &c.invalid_length_name),
        (l.size_overflow_variant, &c.size_overflow_name),
        (l.out_of_memory_variant, &c.out_of_memory_name),
    ];
    if errors.iter().map(|(i, _)| i).collect::<HashSet<_>>().len() != 3 {
        return Err(bad());
    }
    for (index, name) in errors {
        let v = error.variants.get(index).ok_or_else(bad)?;
        if &v.name != name || !v.fields.is_empty() {
            return Err(bad());
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AllocationFailure {
    InvalidLength,
    SizeOverflow,
    OutOfMemory,
}
/// The interpreter models the supported host's size_t header and address space.
pub(super) fn checked_byte_size(
    len: i32,
    header: u128,
    maximum: u128,
) -> Result<usize, AllocationFailure> {
    if len < 0 {
        return Err(AllocationFailure::InvalidLength);
    }
    let size = header
        .checked_add(len as u128)
        .filter(|n| *n <= maximum)
        .ok_or(AllocationFailure::SizeOverflow)?;
    usize::try_from(size).map_err(|_| AllocationFailure::SizeOverflow)
}
pub(super) fn evaluate(
    kind: SystemAllocationKind,
    input: i32,
    l: &AllocationResultLayout,
    program: &TypedMain,
    state: &mut EvalState,
) -> Result<OwnerHandle, String> {
    let outcome = match kind {
        SystemAllocationKind::BoxI32 => state.try_allocate(input)?,
        SystemAllocationKind::BytesZeroed => state.try_allocate_bytes(input)?,
    };
    let (variant, fields) = match outcome {
        Ok(owner) => (l.ok_variant, vec![EvalArg::Owner(owner)]),
        Err(failure) => {
            let variant = match failure {
                AllocationFailure::InvalidLength => l.invalid_length_variant,
                AllocationFailure::SizeOverflow => l.size_overflow_variant,
                AllocationFailure::OutOfMemory => l.out_of_memory_variant,
            };
            (
                l.err_variant,
                vec![EvalArg::Value(crate::interpreter::Value::Adt(
                    program.adts[l.error_adt].variants[variant].name.clone(),
                    vec![],
                ))],
            )
        }
    };
    match adt_eval::construct_values(l.result_adt, variant, fields, program, state)? {
        EvalArg::Owner(owner) => Ok(owner),
        _ => Err("E-TYPED-ALLOC: Result must be affine".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Vec<AdtType>, AllocationContract, AllocationResultLayout) {
        let c = AllocationContract {
            result_name: "Result".into(),
            ok_name: "Ok".into(),
            err_name: "Err".into(),
            error_name: "AllocError".into(),
            invalid_length_name: "InvalidLength".into(),
            size_overflow_name: "SizeOverflow".into(),
            out_of_memory_name: "OutOfMemory".into(),
        };
        // Deliberately permuted tags ensure no intrinsic depends on tag order.
        let types = vec![
            AdtType {
                name: c.error_name.clone(),
                arguments: vec![],
                owned: false,
                variants: vec![
                    AdtVariant {
                        name: c.out_of_memory_name.clone(),
                        fields: vec![],
                    },
                    AdtVariant {
                        name: c.invalid_length_name.clone(),
                        fields: vec![],
                    },
                    AdtVariant {
                        name: c.size_overflow_name.clone(),
                        fields: vec![],
                    },
                ],
            },
            AdtType {
                name: c.result_name.clone(),
                arguments: vec![
                    Type::OwnedI32,
                    Type::Adt {
                        id: 0,
                        owned: false,
                    },
                ],
                owned: true,
                variants: vec![
                    AdtVariant {
                        name: c.err_name.clone(),
                        fields: vec![Type::Adt {
                            id: 0,
                            owned: false,
                        }],
                    },
                    AdtVariant {
                        name: c.ok_name.clone(),
                        fields: vec![Type::OwnedI32],
                    },
                ],
            },
        ];
        let l = resolve(Type::Adt { id: 1, owned: true }, &types, &c).unwrap();
        (types, c, l)
    }
    #[test]
    fn independent_verifier_rejects_missing_contract_and_wrong_node_types() {
        let (adts, contract, layout) = fixture();
        let input = ExprNode {
            ty: Type::Int,
            kind: ExprKind::Int(42),
        };
        let mut program = TypedMain {
            adts,
            allocation_contract: Some(contract),
            entry: 0,
            functions: vec![TypedFunction {
                params: vec![],
                result: Type::Int,
                param_cleanup: vec![],
                body: ExprNode {
                    ty: Type::Int,
                    kind: ExprKind::OwnerLet {
                        local: LocalId(0),
                        mutable: false,
                        init: OwnerInit::Expr(Box::new(ExprNode {
                            ty: Type::Adt {
                                id: layout.result_adt,
                                owned: true,
                            },
                            kind: ExprKind::TryAlloc {
                                kind: SystemAllocationKind::BoxI32,
                                input: Box::new(input),
                                layout,
                            },
                        })),
                        body: Box::new(ExprNode {
                            ty: Type::Int,
                            kind: ExprKind::Int(0),
                        }),
                        cleanup: Some(LocalId(0)),
                    },
                },
            }],
        };
        verify(&program).unwrap();
        let saved = program.allocation_contract.take();
        assert!(verify(&program).is_err());
        program.allocation_contract = saved;
        let ExprKind::OwnerLet {
            init: OwnerInit::Expr(node),
            ..
        } = &mut program.functions[0].body.kind
        else {
            panic!()
        };
        node.ty = Type::OwnedI32;
        assert!(verify(&program).is_err());
        let ExprKind::OwnerLet {
            init: OwnerInit::Expr(node),
            ..
        } = &mut program.functions[0].body.kind
        else {
            panic!()
        };
        node.ty = Type::Adt { id: 1, owned: true };
        let ExprKind::TryAlloc { input, .. } = &mut node.kind else {
            panic!()
        };
        **input = ExprNode {
            ty: Type::Bool,
            kind: ExprKind::Bool(true),
        };
        assert!(verify(&program).is_err());
    }
    #[test]
    fn rejects_each_forged_allocation_index_and_kind() {
        let (types, c, good) = fixture();
        validate(SystemAllocationKind::BoxI32, &good, &types, Some(&c)).unwrap();
        assert!(validate(SystemAllocationKind::BoxI32, &good, &types, None).is_err());
        assert!(validate(SystemAllocationKind::BytesZeroed, &good, &types, Some(&c)).is_err());
        for field in 0..7 {
            let mut bad = good.clone();
            let slot = match field {
                0 => &mut bad.result_adt,
                1 => &mut bad.error_adt,
                2 => &mut bad.ok_variant,
                3 => &mut bad.err_variant,
                4 => &mut bad.invalid_length_variant,
                5 => &mut bad.size_overflow_variant,
                _ => &mut bad.out_of_memory_variant,
            };
            *slot = usize::MAX;
            assert!(validate(SystemAllocationKind::BoxI32, &bad, &types, Some(&c)).is_err());
        }
        let mut bad = good.clone();
        bad.ok_variant = good.err_variant;
        assert!(validate(SystemAllocationKind::BoxI32, &bad, &types, Some(&c)).is_err());
        let mut bad = good.clone();
        bad.invalid_length_variant = good.size_overflow_variant;
        assert!(validate(SystemAllocationKind::BoxI32, &bad, &types, Some(&c)).is_err());
    }
    #[test]
    fn rejects_forged_names_payloads_arguments_and_shapes() {
        let (types, c, l) = fixture();
        for case in 0..14 {
            let mut types = types.clone();
            match case {
                0 => types[0].name = "Impostor".into(),
                1 => types[1].name = "Impostor".into(),
                2 => types[0].arguments.push(Type::Int),
                3 => types[1].arguments[0] = Type::OwnedBytes,
                4 => types[1].arguments[1] = Type::Int,
                5 => types[1].variants[l.ok_variant].fields = vec![Type::OwnedBytes],
                6 => types[1].variants[l.err_variant].fields = vec![Type::Int],
                7 => types[1].owned = false,
                8 => types[0].owned = true,
                9 => {
                    let extra = types[1].variants[0].clone();
                    types[1].variants.push(extra);
                }
                10 => types[0].variants[0].name = "Impostor".into(),
                11 => types[0].variants[1].fields.push(Type::Int),
                12 => types[1].variants[l.ok_variant].name = "Impostor".into(),
                _ => {
                    types[0].variants.pop();
                }
            }
            assert!(
                validate(SystemAllocationKind::BoxI32, &l, &types, Some(&c)).is_err(),
                "case {case}"
            );
        }
    }
    #[test]
    fn checked_sizes_cover_target_boundaries_without_allocating() {
        assert_eq!(
            checked_byte_size(i32::MIN, 8, 255),
            Err(AllocationFailure::InvalidLength)
        );
        assert_eq!(
            checked_byte_size(-1, 8, 255),
            Err(AllocationFailure::InvalidLength)
        );
        assert_eq!(checked_byte_size(0, 8, 255), Ok(8));
        assert_eq!(checked_byte_size(1, 8, 255), Ok(9));
        assert_eq!(checked_byte_size(247, 8, 255), Ok(255));
        assert_eq!(
            checked_byte_size(248, 8, 255),
            Err(AllocationFailure::SizeOverflow)
        );
        assert_eq!(
            checked_byte_size(0, 256, 255),
            Err(AllocationFailure::SizeOverflow)
        );
        assert_eq!(
            checked_byte_size(1, u128::MAX, u128::MAX),
            Err(AllocationFailure::SizeOverflow)
        );
        assert_eq!(
            checked_byte_size(i32::MAX, 8, u32::MAX as u128),
            Ok(i32::MAX as usize + 8)
        );
    }
    #[test]
    fn ordinary_results_recover_then_allocate_and_cleanup() {
        let mut source = crate::parser::parse("type Result<a,b> = Ok(a) | Err(b)\ntype AllocError = InvalidLength | SizeOverflow | OutOfMemory\nlet main = let outer = system_box_i32_abort(20) in let outcome = system_box_i32(1) in let recovered = match move outcome { Ok(value) => box_read(value) | Err(error) => match error { InvalidLength => 1 | SizeOverflow => 2 | OutOfMemory => let later = system_box_i32_abort(22) in box_read(later) } } in box_read(outer) + recovered").unwrap();
        source.allocation_types = Some(crate::ast::AllocationTypes {
            result_type: "Result".into(),
            ok_constructor: "Ok".into(),
            err_constructor: "Err".into(),
            error_type: "AllocError".into(),
            invalid_length_constructor: "InvalidLength".into(),
            size_overflow_constructor: "SizeOverflow".into(),
            out_of_memory_constructor: "OutOfMemory".into(),
        });
        let checked = crate::check::analyze(&source).unwrap();
        let typed = lower_main(&checked).unwrap();
        let mut state = EvalState {
            fail_at_attempt: Some(2),
            ..EvalState::default()
        };
        let result = evaluate_function(&typed, typed.entry, vec![], 1, &mut state).unwrap();
        assert!(matches!(
            result,
            EvalArg::Value(crate::interpreter::Value::Int(42))
        ));
        assert_eq!(state.allocation_attempts, 3);
        assert_eq!(state.allocations, 2);
        assert_eq!(state.released, [22, 20]);
        assert!(state.storage.is_empty());
    }
    #[test]
    fn mixed_fixture_replays_every_failure_with_exact_owner_traces() {
        use SystemAllocationKind::{BoxI32, BytesZeroed};
        let directory = std::env::temp_dir().join(format!(
            "rill-interpreter-allocation-matrix-{}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        let entry = directory.join("main.rill");
        std::fs::write(
            &entry,
            include_str!("../../tests/fixtures/fallible_mixed.rill"),
        )
        .unwrap();
        let source = crate::modules::compile::load_program(&entry).unwrap();
        let checked = crate::check::analyze(&source).unwrap();
        let program = lower_main(&checked).unwrap();
        let kinds = [
            BoxI32,
            BoxI32,
            BoxI32,
            BytesZeroed,
            BytesZeroed,
            BoxI32,
            BytesZeroed,
        ];
        for failure in std::iter::once(None).chain((1..=7).map(Some)) {
            let mut state = EvalState {
                fail_at_attempt: failure,
                ..EvalState::default()
            };
            let result = evaluate_function(&program, program.entry, vec![], 1, &mut state).unwrap();
            let expected_value = match failure {
                Some(1) => 1000,
                Some(2) => 52,
                Some(4) => 142,
                _ => 42,
            };
            assert!(
                matches!(result, EvalArg::Value(crate::interpreter::Value::Int(value)) if value == expected_value),
                "failure {failure:?}, result {result:?}"
            );
            let attempts = if failure == Some(1) { 1 } else { 7 };
            assert_eq!(state.allocation_attempts, attempts, "failure {failure:?}");
            let mut ids_by_attempt = [None; 7];
            let mut next_id = 0;
            let mut expected_allocations = vec![];
            for attempt in 1..=attempts {
                if failure != Some(attempt) {
                    ids_by_attempt[attempt - 1] = Some(next_id);
                    expected_allocations.push((next_id, kinds[attempt - 1]));
                    next_id += 1;
                }
                // Every outcome has one inline-ADT bookkeeping carrier. The two
                // rejected lengths after request 1 also create ordinary Errs;
                // request 6's result is nested inside one ordinary Wrapped ADT.
                next_id += 1;
                if attempt == 1 && attempts > 1 {
                    next_id += 2;
                }
                if attempt == 6 {
                    next_id += 1;
                }
            }
            let expected_releases: Vec<_> = [3, 2, 4, 7, 6, 5, 1]
                .into_iter()
                .filter_map(|attempt| {
                    ids_by_attempt[attempt - 1].map(|id| (id, kinds[attempt - 1]))
                })
                .collect();
            assert_eq!(
                state.allocation_trace, expected_allocations,
                "failure {failure:?}"
            );
            assert_eq!(
                state.release_trace, expected_releases,
                "failure {failure:?}"
            );
            assert_eq!(
                state.allocations,
                expected_allocations.len(),
                "failure {failure:?}"
            );
            assert_eq!(state.frees, expected_releases.len(), "failure {failure:?}");
            assert_eq!(state.next_id, next_id, "failure {failure:?}");
            assert!(state.storage.is_empty(), "failure {failure:?}");
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn invalid_requests_do_not_consume_one_shot_failure() {
        let mut state = EvalState {
            fail_at_attempt: Some(1),
            ..EvalState::default()
        };
        assert!(matches!(
            state.try_allocate_bytes(-1).unwrap(),
            Err(AllocationFailure::InvalidLength)
        ));
        assert_eq!(state.allocation_attempts, 0);
        assert!(matches!(
            state.try_allocate(1).unwrap(),
            Err(AllocationFailure::OutOfMemory)
        ));
        let owner = state.try_allocate_bytes(0).unwrap().unwrap();
        assert_eq!(state.allocation_attempts, 2);
        assert_eq!(state.allocations, 1);
        state.release(owner.id).unwrap();
        assert_eq!(state.frees, 1);
        state.next_id = usize::MAX;
        assert!(state
            .try_allocate(1)
            .unwrap_err()
            .contains("identity overflow"));
    }
}
