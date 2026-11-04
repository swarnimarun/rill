# Negation and unit conditionals on v1-ernie

The port starts at `c3f41ef` and retains the upstream result annotations,
recursive constraints, deferred projections and typed source-origin machinery.
Boolean negation lowers to checked equality with `false`. An omitted braced
`else` lowers to unit and therefore still requires a unit-valued then branch.
No typed IR form or owner fallback changes.

Fresh checks passed 59 tests across `boolean_not`, `boolean_not_source`,
`conditional_if`, `ast_source_map`, `syntax_tree`, `source_format`,
`result_annotation_origins` and `deferred_projection_origins`. The new execution
tests compare checked evaluation, the source evaluator and C11 at O0/O2,
including short circuit effects and conditional owner cleanup.

The first combined run's formatting corpus encountered the next slice's new
lambda example while running the earlier compiled parser. Removing that
uncommitted example and rerunning the exact earlier `source_format` and
`syntax_tree` test binaries passed all 20 tests. No failing result was counted
as a pass. Formatting and staged diff whitespace checks passed.

This is a syntax increment, not a completed language phase or stability claim.
