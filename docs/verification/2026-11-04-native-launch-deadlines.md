# Native fixture launch and execution deadlines

The untouched `c3f41ef` baseline and the core port both exceeded existing
10-second C execution deadlines while launching newly linked macOS binaries.
Retained returning executables subsequently produced their expected output in
milliseconds. These failed runs remain failures, not accepted language traps.

The bounded C harnesses now share `tests/support/native_launch.rs`. On macOS,
fixture compilation adds a small wrapper translation unit and renames the
emitted entry point through a compiler argument. Emitted C files remain byte
identical. The wrapper writes a unique start marker immediately before calling
the generated main function. The helper grants at most 60 seconds to reach that
marker, then starts the existing execution deadline. Compiler and resource-guard
deadlines remain unchanged; other platforms retain ordinary process deadlines.
Markers and wrapper sources remain with retained evidence. The helper does not
remove probe artifacts.

A launch timeout, execution timeout, missing handshake, signal, or unexpected
exit remains a failure. Existing result, stderr, event, cleanup and recursion
assertions are unchanged. In particular, timeout is never a successful language
trap, and an infinite loop after entering main still exhausts its execution
allowance.

Fresh verification passed three real C harness tests: exact result/trap output,
a slow pre-main initializer with a shorter main deadline, and a post-main
infinite loop that remains a timeout. The previously timing-out
`ordinary_discarded_results_return_without_layout_or_dummy_value` regression
passed with the unchanged 10-second main deadline. Both external independent
probe sources compiled as Rust metadata. Formatting, diff checks and strict
all-target Clippy passed in default and importer-enabled modes.

The broader affected execution matrix is separate evidence and was still
running when this focused harness increment was committed. Its final result
must be reported independently. No language or compiler representation behavior
changes, and no phase gate closes.
