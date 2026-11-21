# Scalar C library

`update.rill` exports a checked integer computation to an independent C caller.
From the repository root, choose a new output directory:

```sh
mkdir -p build
cargo run --locked --example c_exports -- build/c-exports
cc -std=c11 -O3 build/c-exports/update.c examples/c-exports/host.c -Ibuild/c-exports -o build/c-exports/host
build/c-exports/host
```

The caller prints `42`. See the [C export contract](../../docs/c-exports.md)
for admitted signatures, traps and the remaining native reload work.
