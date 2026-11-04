//! Bound newly accepted result annotation trees before recursive declaration
//! resolution, cloning or owner classification. Public AST inputs bypass the
//! parser's independent nesting guard, so inspect the actual borrowed tree.
use crate::ast::{Program, TypeExpr};

const ERROR: &str = "E-TYPE-COMPLEXITY: result annotation input budget exceeded";
const MAX_DEPTH: usize = 128;
const NODE_WEIGHT: usize = 64;
const TREE_WEIGHT: usize = 16 * 1024 * 1024;
const TOTAL_WEIGHT: usize = 64 * 1024 * 1024;
const WORK: usize = 4_000_000;

pub(crate) fn preflight_results(program: &Program) -> Result<(), String> {
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
            if let Some(annotation) = &function.result_annotation {
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
}
