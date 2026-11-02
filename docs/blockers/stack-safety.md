# Stack safety and recursion

The parser rejects deeply nested expression, pattern, and type syntax with
`E-PARSE-DEPTH`. Its recursion guard and expression-tree limit are 64; a single
binary-operator chain also has a 64-operator construction limit. The recursion
guard counts parser calls, so a program can hit it with fewer than 64 visible
parentheses. These are temporary implementation limits, not a language
compatibility promise. The parser tests exercise very deep malformed and valid
inputs in a subprocess with a 2 MiB test thread stack.

The interpreter and C backend trap before entering a 33rd active Rill function
call. The initial call to `main` counts toward this limit. Both backends
evaluate call arguments before checking the limit. A depth cap of 64 still
overflowed a default-size Rust test thread on a nested JSON example; 32 traps
before that reproducer overflows.

Neither guard establishes general stack safety. Other compiler traversals can
recurse over a large number of declarations, dependencies, or types. The
interpreter uses host stack to evaluate expression trees, and parsing limits do
not prove those evaluations fit every thread stack. Generated C uses stack
space for inline values, and large layouts or a small native thread stack can
overflow before the call-depth limit. The lexer also has no total source-size or
token-count budget; very large input can exhaust memory or processing time.
Public APIs that accept manually constructed ASTs bypass parser limits.

Replace recursive interpretation and relevant compiler traversals with explicit
work stacks, and lower calls using a specified stack policy or checked stack
budget. Add bounded source/input resource policies as appropriate. Test deep
source and declarations, manually constructed ASTs, deeply nested values, small
thread stacks, and large inline layouts on supported targets before raising or
removing the limits.

The C emitter also rejects a function when its conservative count of value
temporaries and parameter copies exceeds a 1 MiB estimated frame budget
(`E-C-STACK`). The uniform and typed paths use different per-value estimates.
Generated C asserts the type sizes used by each estimate.
Finite enums in the owner path also have a 1 MiB conservative inline-layout
budget. Layout sizes are computed once in dependency order, so repeatedly
nesting a two-field type cannot trigger exponential layout traversal. The
typed registry currently caps nominal nesting at 128 and concrete instances
at 1024. Enum cleanup passes aggregate addresses to its recursive helpers.
This rejects some programs that could run, and is not a proof of native stack
safety: ABI copies, compiler-generated temporaries, call chains, runtime helpers,
and platform stack size are outside the estimate. For example, a wide nested
`Pair<a> = Pair(a, a)` value produced a segmentation fault on a 128 KiB stack
before this check; `tests/stack_budget.rs` now checks rejection of that source
and runs a small program under the same stack limit.
