# Library and overlay foundation

Open-document overlays feed the existing bounded loader and source-preserving
module compiler. They retain canonical path and hardlink identity, root
containment, cycles, privacy, size limits and exact owned source text. Missing
files need a matching overlay and an existing parent directory. Library checking
uses the ordinary checker without requiring a main function; any existing main
must retain its zero-argument signature.

The upstream immutable `ProgramSnapshot`, closed-result rewrite, graph-wide
annotation admission and checked provenance remain intact. A new unsaved library
fixture verifies the exact `FunctionResult` origin and checked result annotation.
Native cached-header augmentation belongs to the subsequent native slice.

Focused library, overlay, graph, module-origin, callable-module and upstream
annotation/projection-origin targets report 59 passing tests. The non-UTF-8 path
case reports a runtime SKIP on macOS because this filesystem/sandbox rejects
invalid byte directory names with EPERM. The archived narrow platform guard was
retained after reproducing that denial; no path-support claim is made for this
skipped case. All-target strict Clippy, formatting and diff checks pass.

No external native dependency tools run through these foundational APIs. This
increment does not complete a language phase or broaden foreign ownership.
