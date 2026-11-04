# Integration onto v1-ernie

This is a running verification record for the port from archive snapshot
`69277817ff0408c44dacb723bb1edc20da86159d` onto upstream
`c3f41ef6027033553bd19202bbf457e5d9a3c9d1`.
The [plan](../superpowers/plans/2026-11-04-ernie-integration.md) and
[decision trail](2026-11-04-ernie-decisions.tsv) distinguish completed slices
from remaining work. Earlier reports retain their original checkpoint scope.

## Compiler-action validation

Integrated commit `7e899f8` preserves the upstream configured-native workflow
and working-directory-aware C validation. Ordinary compiler arguments cannot
replace the controlled compilation action, language mode or output path.

Fresh checks on the integration worktree:

```sh
cargo test --locked --test native_validation --test native_cli \
  --test native_configuration --test c_import_direct
```

All 19 tests passed across four targets. The independent slice worktree also
ran these tests and formatting before its commit. Inspection confirmed that
explicit saved configurations retain their separate argument grammar and
directory handling.

## Publication

The plan commit `cf42928` and compiler-action slice `7e899f8` were pushed as
normal fast-forwards to `v1-ernie`. The configured GPG signer timed out waiting
for an interactive prompt. These integration commits use a per-command signing
override; the user's Git configuration remains unchanged.

## Portable SDL capture

The capture collector uses a presentation-call macro redirect, retaining a
separate call to the real SDL function. Both the shell utility and the upstream
matrix retain overwrite refusal. The matrix still inspects the exact typed C,
hashes compiler dependencies and compares six real O0/O2 framebuffers.

Fresh checks on macOS AArch64 with SDL3 3.4.18:

- All 18 Python matrix tests passed; shell syntax checking passed.
- A c-import-enabled integration binary passed six captures and five RGBA
  comparisons, including direct versus generated bindings and O0 versus O2.
- The shell utility captured both actual SDL game framebuffers successfully.

The first matrix attempt used a binary without `c-import` and rejected import.
The enabled run then reported a missing `AvailabilityMacros.h`. Supplying the
SDK returned by `xcrun --show-sdk-path` through repeated `--c-arg` options fixed
that configuration. Those exact flags now reach header extraction, C emission
and compilation, and are retained in the manifest. The successful capture
does not establish hardware-renderer or physical-input support.

## Open verification

The upstream baseline full suite is running separately. Its result is not yet
an integrated acceptance claim. Compiler callables, automatic headers, editor
support and demonstrations are still being ported. No delivery phase is marked
complete by the slices above.
