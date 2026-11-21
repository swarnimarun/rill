# Fixed-operand type diagnostics

The checker infers both operand expressions in source order, then constrains each operand to the primitive type required by the operator. For example, `fn main() { 40 + false }` reports `E-TYPE: right operand of '+' expects i32, found bool`. Arithmetic and ordered comparisons require `i32`. Boolean conjunction and disjunction require `bool`. Subtraction also represents unary minus in the AST, so its diagnostic identifies the operator without a left/right label.

The solver reports the first conflicting resolved type heads. A generic nominal value is identified by its type name, a callable by its category. Nominal names longer than 64 bytes use the category. Formatting examines a single arena node on either side of the failed constraint. It never walks type arguments or a function environment to describe a mismatch. Ordinary equality, occurs-check, loan and complexity errors keep their existing paths.

Run `cargo test --locked --test operand_diagnostics` for source-to-checker/interpreter/C rejection parity and `cargo test --locked --lib contextual_mismatch_reads_only_the_resolved_heads` for the shared-DAG test with a zero materialization budget.
