# Explicit selected-C configuration records

Status: accepted bounded implementation with focused checks, independent
execution and the full combined aggregate. This is a P5 tooling increment,
not phase completion or a stable artifact format.

The opt-in native configuration is an ordered recipe and expected observations,
not a cached proof. Ordinary generated Rill source never discovers or loads it.
The compiler is explicitly selected by the invoking CLI, and its resolved
invocation spelling is compared with the record. The record cannot choose an
executable, raw C assertion, environment override or output destination.

A captured working directory is relative to the artifact parent where possible.
Package resolution, libclang and target validation run freshly for every use,
with the same ordered common flags. Only O0 and O2 extend the legacy importer
allowlist. Strict profiles additionally reject ambiguous operands, validation
macro overrides and arbitrary driver/linker switches. Existing unconfigured CLI
behavior and ABI support remain unchanged.

Capture publishes one binary configuration file. A separate fresh command
materializes one ordinary binding module. Neither operation claims atomic
publication of two files. Configured consumers require exact module bytes from
the same owned source snapshot they lower, and reject additional native
configuration or override flags. Final emitted C includes safe assertions
rendered from fresh structured facts, never artifact-provided C text.

The binary schema is bounded, fixed-order and Unix-byte-preserving, with a
read-only escaped inspection command. Decoding gives only an unverified record.
It stores no timestamp, secret environment, validation-success flag or cache
key. It does not establish a hermetic toolchain identity or pin transitive
headers, native libraries or compiler contents. Trusted inputs must remain
stable while an invocation runs.

Non-Unix supervision, cache identity, transactional bundles, broader C ABI
forms and foreign ownership/lifetime/effect contracts remain separate work.
The importer timeout bounds supervised preparation, not the final application
compiler/linker or the explicitly requested runtime program.
