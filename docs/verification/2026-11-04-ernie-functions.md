# Callable port on the upstream typed compiler

The callable port retains upstream `Function.result_annotation`, SCC-wide
closed signature constraints, projection solving before local/component
generalization, symbolic field-loan checking and checked typed-native dispatch.
Lambda parameter annotations share both source and resolved-name admission
budgets before recursive consumers or module cloning. Generated conversion
functions retain checked result annotations. Unsupported typed origin mapping
reports a diagnostic instead of reusing paths from the original AST.

Fresh verification passed 86 checker/solver unit tests and 63 integration tests
across `function_syntax`, `function_inference`, `function_modules`,
`function_reference`, `function_upstream`, `functions` and
`recursive_signatures`. The 27 conformance cases exercise the independent source
evaluator, checked conversion and C11 at O0/O2. Formatting, all-target checking,
strict all-target Clippy and diff whitespace checks passed.

Compatibility testing caught and fixed a depth-128 signature regression caused
by unused callable metadata on ordinary saturated calls. The complete existing
`recursive_signatures` target passed after removing that metadata; indirect and
partial application metadata remains. One new phantom fixture initially omitted
a space before `=`, producing `>=`; correcting the fixture yielded six passing
cross-feature tests, including closed recursion, deferred callback projection,
annotation admission and optional-origin rejection.

Broader execution/origin compatibility runs are separate evidence. Some existing
C harnesses exceeded their ten-second execution deadline in this environment.
Five retained annotation executables subsequently produced their expected values
and native event output in 7–10 milliseconds. Those replays do not turn the
failed suite runs into passes. The final integrated run must report its own
outcomes and any environmental timeouts.

Callable conversion still rejects unresolved stored phantom captures and
nonreturning callable signatures with `E-FUNCTION-LAYOUT`; ordinary upstream
programs keep their existing checked representation paths. Owner closures and
callable typed-origin indexes remain unsupported. No phase gate is closed.
