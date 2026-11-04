//! Bound all closed parameter and result annotations before recursive declaration
//! resolution, cloning or owner classification. Public AST inputs bypass the
//! parser's independent nesting guard, so inspect the actual borrowed tree.
use crate::ast::{Program, TypeExpr};

// Parameter and result annotations share this admission diagnostic.
const ERROR: &str = "E-TYPE-COMPLEXITY: signature annotation input budget exceeded";
const MAX_DEPTH: usize = 128;
const NODE_WEIGHT: usize = 64;
const TREE_WEIGHT: usize = 16 * 1024 * 1024;
const TOTAL_WEIGHT: usize = 64 * 1024 * 1024;
const WORK: usize = 4_000_000;

pub(crate) fn preflight_signatures(program: &Program) -> Result<(), String> {
    Budget::default().program(program)
}

/// One allowance for an entire checked program or parsed module graph.
/// Source spelling and final resolved spelling each receive a complete pass
/// before any annotation trees are cloned for module rewriting.
pub(crate) struct Budget {
    remaining: usize,
    work: usize,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            remaining: TOTAL_WEIGHT,
            work: WORK,
        }
    }
}
impl Budget {
    pub(crate) fn program(&mut self, program: &Program) -> Result<(), String> {
        self.program_named(program, |name| Ok(name.len()), || ERROR.to_owned())
    }

    pub(crate) fn program_named<E>(
        &mut self,
        program: &Program,
        mut name_len: impl FnMut(&str) -> Result<usize, E>,
        error: impl Fn() -> E,
    ) -> Result<(), E> {
        for function in &program.functions {
            for annotation in function
                .params
                .iter()
                .filter_map(|parameter| parameter.annotation.as_ref())
                .chain(function.result_annotation.iter())
            {
                self.tree_named(annotation, MAX_DEPTH, TREE_WEIGHT, &mut name_len, &error)?;
            }
        }
        Ok(())
    }

    fn tree_named<E>(
        &mut self,
        root: &TypeExpr,
        depth_limit: usize,
        tree_limit: usize,
        name_len: &mut impl FnMut(&str) -> Result<usize, E>,
        error: &impl Fn() -> E,
    ) -> Result<(), E> {
        // One iterator per depth, not one allocation per sibling. Reserve the
        // node's full logical weight before scheduling its child iterator.
        let mut pending = Vec::new();
        let mut next = Some((root, true));
        let mut weight = 0usize;
        while let Some((node, mut rewrite)) = next {
            self.work = self.work.checked_sub(1).ok_or_else(error)?;
            if pending.len() >= depth_limit {
                return Err(error());
            }
            // Exactly mirror closed Rewrite::ty's raw-pointer exception. Even
            // malformed pointer arguments are still fully counted, unchanged.
            rewrite &= !matches!(node.name.as_str(), "ptr" | "const_ptr");
            let bytes = if rewrite {
                name_len(&node.name)?
            } else {
                node.name.len()
            };
            let node_weight = NODE_WEIGHT.checked_add(bytes).ok_or_else(error)?;
            weight = weight.checked_add(node_weight).ok_or_else(error)?;
            if weight > tree_limit {
                return Err(error());
            }
            self.remaining = self.remaining.checked_sub(node_weight).ok_or_else(error)?;
            pending.push((node.args.iter(), rewrite));
            next = loop {
                let Some((children, rewrite)) = pending.last_mut() else {
                    break None;
                };
                if let Some(child) = children.next() {
                    break Some((child, *rewrite));
                }
                pending.pop();
            };
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn with_limits(remaining: usize, work: usize) -> Self {
        Self { remaining, work }
    }
    #[cfg(test)]
    fn tree(&mut self, root: &TypeExpr, depth: usize, weight: usize) -> Result<(), String> {
        self.tree_named(root, depth, weight, &mut |name| Ok(name.len()), &|| {
            ERROR.to_owned()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn atom() -> TypeExpr {
        TypeExpr {
            name: "unit".into(),
            args: vec![],
        }
    }
    #[test]
    fn actual_tree_limits_are_charged_before_recursive_consumers() {
        let ty = TypeExpr {
            name: "Pair".into(),
            args: vec![atom(), atom()],
        };
        let cost = 3 * (NODE_WEIGHT + 4);
        let run =
            |remaining, work, depth, weight| Budget { remaining, work }.tree(&ty, depth, weight);
        assert!(run(cost, 3, 2, cost).is_ok());
        for result in [
            run(cost - 1, 3, 2, cost),
            run(cost, 2, 2, cost),
            run(cost, 3, 1, cost),
            run(cost, 3, 2, cost - 1),
        ] {
            assert_eq!(result.unwrap_err(), ERROR);
        }
        let mut shared = Budget {
            remaining: cost,
            work: 6,
        };
        shared.tree(&ty, 2, cost).unwrap();
        assert_eq!(shared.tree(&ty, 2, cost).unwrap_err(), ERROR);
    }
    #[test]
    fn budgets_cover_wide_trees_utf8_names_raw_pointees_and_multiple_programs() {
        let long = TypeExpr {
            name: "éλ".into(),
            args: vec![],
        };
        let weight = NODE_WEIGHT + 4;
        assert!(Budget::with_limits(weight, 1)
            .tree(&long, 1, weight)
            .is_ok());
        assert!(Budget::with_limits(weight - 1, 1)
            .tree(&long, 1, weight)
            .is_err());
        let wide = TypeExpr {
            name: "Tuple".into(),
            args: vec![atom(); 1024],
        };
        let mut visits = 0;
        let error = Budget::with_limits(TOTAL_WEIGHT, 1)
            .tree_named(
                &wide,
                MAX_DEPTH,
                TREE_WEIGHT,
                &mut |name| {
                    visits += 1;
                    Ok(name.len())
                },
                &|| ERROR.to_owned(),
            )
            .unwrap_err();
        assert_eq!(error, ERROR);
        assert_eq!(visits, 1, "work is charged before visiting the next child");
        let raw = TypeExpr {
            name: "ptr".into(),
            args: vec![long],
        };
        let raw_weight = NODE_WEIGHT + 3 + weight;
        let mut never_resolve =
            |_: &str| -> Result<usize, String> { panic!("raw C pointee entered Rill resolution") };
        assert!(Budget::with_limits(raw_weight, 2)
            .tree_named(&raw, 2, raw_weight, &mut never_resolve, &|| ERROR
                .to_owned())
            .is_ok());
        assert!(Budget::with_limits(raw_weight - 1, 2)
            .tree_named(&raw, 2, raw_weight, &mut never_resolve, &|| ERROR
                .to_owned())
            .is_err());
        let first = crate::parser::parse("fn first() -> unit {()}").unwrap();
        let second = crate::parser::parse("fn second() -> unit {()}").unwrap();
        let mut shared = Budget::with_limits(2 * weight - 1, 2);
        shared.program(&first).unwrap();
        assert_eq!(shared.program(&second).unwrap_err(), ERROR);
        assert!(Budget::with_limits(weight, 1).program(&second).is_ok());
    }

    #[test]
    fn default_depth_and_per_tree_weight_boundaries_are_exact() {
        let mut ty = atom();
        for _ in 1..MAX_DEPTH {
            ty = TypeExpr {
                name: "W".into(),
                args: vec![ty],
            };
        }
        assert!(Budget::default().tree(&ty, MAX_DEPTH, TREE_WEIGHT).is_ok());
        ty = TypeExpr {
            name: "W".into(),
            args: vec![ty],
        };
        assert_eq!(
            Budget::default()
                .tree(&ty, MAX_DEPTH, TREE_WEIGHT)
                .unwrap_err(),
            ERROR
        );
        let exactly = TypeExpr {
            name: "X".repeat(TREE_WEIGHT - NODE_WEIGHT),
            args: vec![],
        };
        assert!(Budget::default()
            .tree(&exactly, MAX_DEPTH, TREE_WEIGHT)
            .is_ok());
        assert!(Budget::default()
            .tree(&exactly, MAX_DEPTH, TREE_WEIGHT - 1)
            .is_err());
    }

    fn signature(parameter: TypeExpr, result: Option<TypeExpr>) -> Program {
        let mut program = crate::parser::parse("fn unused(x) {()} fn main() {()}").unwrap();
        program.functions[0].params[0].annotation = Some(parameter);
        program.functions[0].result_annotation = result;
        program
    }

    fn nested(depth: usize) -> TypeExpr {
        let mut ty = atom();
        for _ in 1..depth {
            ty = TypeExpr {
                name: "W".into(),
                args: vec![ty],
            };
        }
        ty
    }

    fn dismantle(program: Program) {
        let mut pending = Vec::new();
        for function in program.functions {
            pending.extend(function.params.into_iter().filter_map(|p| p.annotation));
            pending.extend(function.result_annotation);
        }
        while let Some(mut ty) = pending.pop() {
            pending.append(&mut ty.args);
        }
    }

    #[test]
    fn unused_parameter_and_mixed_signature_depth_boundaries_are_exact() {
        for mixed in [false, true] {
            for depth in [128, 129, 20_000] {
                let program = signature(nested(depth), mixed.then(atom));
                let result = preflight_signatures(&program);
                // A rejected caller-owned tree still belongs to the caller;
                // dismantle it without recursive Drop, even on test failure.
                dismantle(program);
                if depth == 128 {
                    result.unwrap();
                } else {
                    assert_eq!(result.unwrap_err(), ERROR);
                }
            }
        }
        let program = signature(atom(), Some(nested(129)));
        let result = preflight_signatures(&program);
        dismantle(program);
        assert_eq!(result.unwrap_err(), ERROR);
    }

    #[test]
    fn every_parameter_precedes_its_result_in_every_function() {
        let program = crate::parser::parse(
            "fn unused(a: i32, inferred, b: bool) -> unit {()}\nlet legacy (x: str) = ()\nfn main() {()}",
        ).unwrap();
        let mut names = Vec::new();
        Budget::with_limits(4 * NODE_WEIGHT + 14, 4)
            .program_named(
                &program,
                |name| {
                    names.push(name.to_owned());
                    Ok(name.len())
                },
                || ERROR.to_owned(),
            )
            .unwrap();
        assert_eq!(names, ["i32", "bool", "unit", "str"]);
    }

    #[test]
    fn malformed_raw_parameter_depth_is_not_exempt() {
        for depth in [128, 129, 20_000] {
            let program = signature(
                TypeExpr {
                    name: "ptr".into(),
                    args: vec![nested(depth - 1)],
                },
                None,
            );
            let result = Budget::default().program_named(
                &program,
                |_| -> Result<usize, String> { panic!("raw subtree must not resolve") },
                || ERROR.to_owned(),
            );
            dismantle(program);
            if depth == 128 {
                result.unwrap();
            } else {
                assert_eq!(result.unwrap_err(), ERROR);
            }
        }
    }

    #[test]
    fn parameter_and_result_share_exact_utf8_wide_weight_and_work() {
        for mixed in [false, true] {
            let wide = TypeExpr {
                name: "éλ".into(),
                args: vec![atom(); 1024],
            };
            let nodes = 1025 + usize::from(mixed);
            let cost = nodes * (NODE_WEIGHT + 4);
            let program = signature(wide, mixed.then(atom));
            let mut exact = Budget::with_limits(cost, nodes);
            exact.program(&program).unwrap();
            assert_eq!((exact.remaining, exact.work), (0, 0));
            for (weight, work) in [(cost - 1, nodes), (cost, nodes - 1)] {
                assert_eq!(
                    Budget::with_limits(weight, work)
                        .program(&program)
                        .unwrap_err(),
                    ERROR
                );
            }
        }
    }

    #[test]
    fn per_tree_limit_resets_but_graph_weight_and_work_do_not() {
        let program = signature(atom(), Some(atom()));
        // Inject logical resolved-name lengths rather than allocate giant names.
        let run = |name_bytes| {
            Budget::with_limits(2 * TREE_WEIGHT + 2, 2).program_named(
                &program,
                |_| Ok(name_bytes),
                || ERROR.to_owned(),
            )
        };
        run(TREE_WEIGHT - NODE_WEIGHT).unwrap();
        assert_eq!(run(TREE_WEIGHT - NODE_WEIGHT + 1).unwrap_err(), ERROR);
        let parameter_only = signature(atom(), None);
        Budget::default()
            .program_named(
                &parameter_only,
                |_| Ok(TREE_WEIGHT - NODE_WEIGHT),
                || ERROR.to_owned(),
            )
            .unwrap();
        assert_eq!(
            Budget::default()
                .program_named(
                    &parameter_only,
                    |_| Ok(TREE_WEIGHT - NODE_WEIGHT + 1),
                    || ERROR.to_owned()
                )
                .unwrap_err(),
            ERROR
        );

        // Legacy and unused declarations participate, not only reachable fn bodies.
        let legacy = crate::parser::parse("let unused (x: unit) = ()\nlet main = ()").unwrap();
        let cost = 3 * (NODE_WEIGHT + 4);
        for (weight, work, passes) in [(cost, 3, true), (cost - 1, 3, false), (cost, 2, false)] {
            let mut shared = Budget::with_limits(weight, work);
            shared.program(&program).unwrap();
            let result = shared.program(&legacy);
            if passes {
                result.unwrap();
                assert_eq!((shared.remaining, shared.work), (0, 0));
            } else {
                assert_eq!(result.unwrap_err(), ERROR);
            }
        }
    }

    #[test]
    fn malformed_raw_parameter_subtrees_remain_unresolved_but_fully_charged() {
        for name in ["ptr", "const_ptr"] {
            let parameter = TypeExpr {
                name: name.into(),
                // Malformed arity and nested unknown names must still be charged.
                args: vec![
                    TypeExpr {
                        name: "missing.Type".into(),
                        args: vec![atom()],
                    },
                    TypeExpr {
                        name: "éλ".into(),
                        args: vec![],
                    },
                ],
            };
            let raw_cost = 4 * NODE_WEIGHT + name.len() + "missing.Type".len() + 8;
            for mixed in [false, true] {
                let program = signature(parameter.clone(), mixed.then(atom));
                let cost = raw_cost + usize::from(mixed) * (NODE_WEIGHT + 4);
                let nodes = 4 + usize::from(mixed);
                for (weight, work, passes) in [
                    (cost, nodes, true),
                    (cost - 1, nodes, false),
                    (cost, nodes - 1, false),
                ] {
                    let mut visits = 0;
                    let result = Budget::with_limits(weight, work).program_named(
                        &program,
                        |spelling| {
                            visits += 1;
                            assert_eq!(spelling, "unit", "raw subtree reached name resolution");
                            Ok(spelling.len())
                        },
                        || ERROR.to_owned(),
                    );
                    if passes {
                        result.unwrap();
                        assert_eq!(visits, usize::from(mixed));
                    } else {
                        assert_eq!(result.unwrap_err(), ERROR);
                    }
                }
            }
        }
    }
}
