//! Independent mapping-budget oracle.
//! Private child of typed::control::relation::correspondence.
use super::*;
const CORE:&str="type O<a> = None|Some(a) type R<a> = {left:i32;hole:O<a>;right:i32} fn made(){record R{right=11;hole=None;left=22}} fn main(){let weak=made();42}";
fn lower(source: &str) -> TypedMain {
    let ast = crate::parser::parse(source).unwrap();
    crate::typed::lower_main(&crate::check::analyze(&ast).unwrap()).unwrap()
}
fn mapping(p: &TypedMain, budget: &mut Budget, repeats: usize) -> Result<(), String> {
    let frozen = p.logical.control.as_ref().unwrap();
    let (index, value) = p
        .functions
        .iter()
        .enumerate()
        .find_map(|(index, f)| match &f.body {
            Computation::Discarded(v) if matches!(v.kind, DiscardKind::Record { .. }) => {
                Some((index, v))
            }
            _ => None,
        })
        .unwrap();
    let DiscardKind::Record { definition, fields } = &value.kind else {
        panic!()
    };
    let result = frozen
        .logical_source(&value.result, &mut Budget::default(), 0)
        .unwrap();
    let returns = frozen.source_returns(&mut Budget::default()).unwrap();
    let owners = frozen.witness_owners(&mut Budget::default()).unwrap();
    let facts = IrFacts::build(p).unwrap();
    let mut edges = Vec::new();
    for _ in 0..repeats {
        let mut relation = Relation {
            frozen,
            program: p,
            instance: &frozen.instances[index],
            budget,
            locals: HashMap::new(),
            local_ids: HashSet::new(),
            pending: Vec::new(),
            seen: HashSet::new(),
            pattern_owners: &owners,
            source_returns: &returns,
            ir_facts: &facts,
            edges: &mut edges,
        };
        relation.discard_record(NodeId(value.occurrence), &result, *definition, fields)?;
        assert_eq!(relation.pending.len(), fields.len());
    }
    Ok(())
}
#[test]
fn independent_k4r_record_rule_cost_has_analytical_not_self_measured_boundary() {
    let p = lower(CORE);
    // 13 fixed units: four record-name bytes, two 3-slot vectors, final scan.
    // right: 3 candidates + (9+9+10) name bytes + seen 1 + schema 7 + logical 2 + child 3 = 44.
    // hole: 3 + (8+8) + 1 + schema 18 + logical 5 + child 3 = 46.
    // left: 3 + 8 + 1 + schema 7 + logical 2 + child 3 = 24.
    const EXPECTED: usize = 13 + 44 + 46 + 24;
    let mut measured = Budget::default();
    mapping(&p, &mut measured, 1).unwrap();
    assert_eq!(measured.used, EXPECTED);
    let mut exact = Budget {
        used: MAX_WORK - EXPECTED,
    };
    mapping(&p, &mut exact, 1).unwrap();
    assert_eq!(exact.used, MAX_WORK);
    let mut short = Budget {
        used: MAX_WORK - EXPECTED + 1,
    };
    assert_eq!(mapping(&p, &mut short, 1).unwrap_err(), LIMIT);
    let mut repeated = Budget {
        used: MAX_WORK - EXPECTED * 4,
    };
    mapping(&p, &mut repeated, 4).unwrap();
    assert_eq!(repeated.used, MAX_WORK);
    let mut repeat_short = Budget {
        used: MAX_WORK - EXPECTED * 4 + 1,
    };
    assert_eq!(mapping(&p, &mut repeat_short, 4).unwrap_err(), LIMIT);
}
#[test]
fn independent_k4r_every_scanned_label_byte_is_charged() {
    let p = lower(CORE);
    let mut baseline = Budget::default();
    mapping(&p, &mut baseline, 1).unwrap();
    let longer = "right_with_long_label";
    let delta = longer.len() - "right".len();
    let q = lower(&CORE.replace("right", longer));
    let mut changed = Budget::default();
    mapping(&q, &mut changed, 1).unwrap();
    // right is the third declaration field, so its written label occurs in
    // three name comparisons, while the canonical right name is visited once.
    assert_eq!(changed.used - baseline.used, 4 * delta);
    let mut exact = Budget {
        used: MAX_WORK - changed.used,
    };
    mapping(&q, &mut exact, 1).unwrap();
    let mut short = Budget {
        used: MAX_WORK - changed.used + 1,
    };
    assert_eq!(mapping(&q, &mut short, 1).unwrap_err(), LIMIT);
}
#[test]
fn independent_k4r_nested_substitution_and_wide_reverse_mapping_share_one_budget() {
    let nested=lower("type O<a> = None|Some(a) type I<a> = {hole:O<a>} type R<a,b> = {left:I<O<a>>;hole:O<b>;right:i32} fn made(){record R{right=11;hole=None;left=record I{hole=Some(None)}}} fn main(){let weak=made();42}");
    let mut baseline = Budget::default();
    mapping(&nested, &mut baseline, 1).unwrap();
    assert!(baseline.used > 127);
    let mut exact = Budget {
        used: MAX_WORK - baseline.used * 2,
    };
    mapping(&nested, &mut exact, 2).unwrap();
    assert_eq!(exact.used, MAX_WORK);
    let mut short = Budget {
        used: MAX_WORK - baseline.used * 2 + 1,
    };
    assert_eq!(mapping(&nested, &mut short, 2).unwrap_err(), LIMIT);
    let mut declaration = String::new();
    let mut values = String::new();
    for index in 0..32 {
        declaration.push_str(&format!("field_{index}:i32;"));
    }
    declaration.push_str("hole:O<a>");
    for index in (0..32).rev() {
        values.push_str(&format!("field_{index}={index};"));
    }
    values.push_str("hole=None");
    let p=lower(&format!("type O<a> = None|Some(a) type R<a> = {{{declaration}}} fn made(){{record R{{{values}}}}} fn main(){{let weak=made();42}}"));
    let mut measured = Budget::default();
    mapping(&p, &mut measured, 1).unwrap();
    assert!(measured.used > 5_000);
    let mut exact = Budget {
        used: MAX_WORK - measured.used,
    };
    mapping(&p, &mut exact, 1).unwrap();
    assert_eq!(exact.used, MAX_WORK);
    let mut short = Budget {
        used: MAX_WORK - measured.used + 1,
    };
    assert_eq!(mapping(&p, &mut short, 1).unwrap_err(), LIMIT);
}
