# Closed function results

Run `cargo run -- run examples/result-annotations/main.rill` to print `42`.
The optional `-> Option<i32>` is a closed result constraint on a braced function.
It makes this empty option concrete before its value is observed or mutated.
Generic result variables, local annotations and new ownership rules are outside
this author-stage checkpoint. Unannotated functions keep their existing inference.
