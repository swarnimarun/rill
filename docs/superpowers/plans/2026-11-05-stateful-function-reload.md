# Stateful function reload implementation plan

> **For agentic workers:** Use the delegate skill for ABI, runtime and host
> integration work. Keep each implementation with its verification and review.

**Goal:** Replace selected native Rill functions between calls while retaining
validated application state, with explicit schema migration and checkpoints.

**Architecture:** A checked Copy-value byte ABI marshals actual typed C values.
A host runtime owns one private image per public function and publishes selected
images plus migrated state as one transaction. Window/GPU resources stay in a
separate, permanently loaded host adapter.

**Tech Stack:** Rust, C11, Serde JSON, existing trusted native compiler and Unix
loader, Raylib SDK for the optional renderer.

**Spec:** User request for function-level reload, persistent state and an
integrated workflow; PLAN14.5 and19; docs/blockers/hot-reload.md.

## Global constraints

- Do not push, publish or rewrite completed history. Use focused local commits.
- Preserve concurrent user documentation changes. README, CONTINUATION_PLAN,
  build-performance, language-features, next-increments and stdlib/README already
  have edits outside this task; do not overwrite them.
- Keep old i32 ABI and project watch/restart behavior compatible.
- Source checking and independent typed verification remain mandatory. Derive
  C fields/function IDs from verified IR, never parse or guess emitted layouts.
- Admit pure closed Copy i32/bool/f32 records/enums only at the new boundary.
  Reject pointers, foreign declarations, owners, unknown layouts and callables.
- Use exclusive synchronous calls, no escaping code pointers, no Send/Sync.
  Native arithmetic/depth traps remain process-fatal, not rollback results.
- External schema identity is structural plus source leaf names. Compilation
  IDs are not stable names; internal nominal Rill type checking is unchanged.
- Limit schema depth16, nodes4096, encoded value64KiB, roots64 and arguments32.
  Checkpoints are strict bounded JSON, at most1MiB. F32 stores raw u32 bits.
- GPU handles never enter persistent values. The host owns their lifetime.

## Grounding and design synthesis

The old NativeSession owns one image and validates sorted i32 exports before
replacement. typed::library independently checks all bodies and private root
IDs, while typed::c knows actual record/enum field names. Source watch already
rebuilds candidates and retains the previous process on failure, but successful
builds restart the process and cannot retain its resources.

Choose a typed byte dispatcher over indexed i32 adapters. An i32 adapter cannot
return a multiword state once without repeated evaluation or retained globals.
The byte dispatcher decodes all arguments, checks maximum output capacity,
calls the typed root once and encodes its result.

Choose independently loaded function images over live-frame trampolines.
Each selected root includes a coherent snapshot of its reachable helpers.
Unselected roots retain their old helper snapshots. Explicit batch selection
updates dependent roots together; state-schema changes must update every bound
root that consumes the changed state.

## Task1: checked Copy state ABI

Owned by ABI worker: src/abi/copy.rs and submodules, src/typed/library.rs,
src/typed/c/wire.rs, narrowly scoped raw-source admission, ABI tests.

```rust
use rill::abi::{copy::{Schema, Value, Function}, Export};
// Schema: I32, Bool, F32, named Record fields, named Enum variants.
// Value mirrors it; F32(u32) preserves transport/checkpoint bits.
// Function { name, parameters: Vec<Schema>, result: Schema }.
// Schema::validate, validate_value, encode, decode, max_encoded_len.
// Function::encode_arguments and decode_result validate the host boundary.
```

- [ ] Add strict Serde data types and bounded LE codec; reject invalid tags,
  booleans, trailing bytes, missing/duplicate fields and unsupported shapes.
- [ ] Lower explicit roots using existing verified instance IDs. Keep the i32
  verifier strict and add a separate Copy entry policy allowing pure f32 only.
- [ ] Emit portable byte wrappers and the private identity/dispatch bridge:
  `rill_abi_copy_identity_v1` and `rill_abi_invoke_copy_v1`.
- [ ] Compare identity/record/enum/f32 results against interpreters and generated
  native callers at O0/O3. Mutate IR, input buffers and schemas to test rejection.

## Task2: private native image lifetime

Owned by root: src/native_reload/copy.rs, shared private loader/build helpers,
src/native_reload.rs reexports, tests/native_copy_reload.rs.

```rust
use rill::native_reload::{CopyCandidate, CopySession, NativeCompiler};
// CopyCandidate::build(&Program, &[Export], &NativeCompiler).
// CopySession::new(candidate), call(name, &[Value]), replace(candidate).
// functions(), manifest(), build_identity(); generation() is u64.
```

- [ ] Compile source-generated candidates only; bind the canonical manifest,
  emitted C, host/compiler identity and fixed flags to a versioned identity.
- [ ] Load/validate candidates before publication, use one fixed byte-call
  signature and keep no native pointer outside the exclusive session.
- [ ] Keep old code callable on failed compile/load/identity/contract checks;
  unload images before removing private files. Reject unsupported platforms.
- [ ] Test changed bodies/helpers, field and enum payloads, f32 bits, rejected
  replacements, missing symbols, wrong identities and repeated cleanup.

## Task3: selected functions and persistent state

Owned by runtime worker: src/live_reload.rs and submodules, runtime tests.

```rust
use rill::live_reload::{Binding, LiveSession, Registration, State};
// Binding::Pure, Query, Step. Registration { name, source, binding }.
// State::new(version, schema, value) validates before use.
// LiveSession::new(program, registrations, state, compiler).
// step(name), query(name), call(name,args), set_state(value).
// replace_selected(program, registrations, compiler, migrate).
// checkpoint_json(), restore_json(bytes,migrate), write_checkpoint(path).
```

- [ ] Compile one selected image per root; retain unselected image/helper
  generations and expose per-root/session generation information.
- [ ] Stage all images, check every retained/selected state contract, migrate
  an immutable old-state snapshot, validate values/version, then publish once.
- [ ] Reject schema changes without explicit forward migration. Failed or
  panicking migration must leave the old state and selected roots untouched.
- [ ] Add strict checkpoint parse/restore with explicit migration; write private
  temporary data, sync and rename atomically, preserving old files on failure.
  Parent-directory sync after publication is best effort, not crash proof.
- [ ] Test selective helper retention, batched migrations, invalid values,
  mixed-version contracts, checkpoint restoration and rollback.

## Task4: integrated development host

Owned by integration worker: examples/native_reload_renderer.rs,
examples/reload-renderer, tests/native_reload_renderer.rs.

- [ ] Keep a C11 Raylib adapter loaded once. Watch source snapshots and reload
  selected pure update/query units after frame scopes return.
- [ ] Demonstrate persistent state, update-only replacement retaining color,
  explicit version1-to2 speed migration updating both bound functions, and
  checkpoint restore after process restart.
- [ ] Use explicit logic-only mode for CI; a counting adapter proves protocol
  lifecycle behavior without pretending to render. Require actual GPU tests
  when RILL_REQUIRE_GPU=1; capture and validate real pixels separately.
- [ ] Document exact source/SDK/watch/checkpoint commands and limits. Preserve
  ordinary project watch as rebuild/restart rather than silently changing it.

## Task5: acceptance and review

- [ ] Run existing reload/watch regressions and all new focused suites.
- [ ] Cross-review ABI, image lifetime, migration transaction and host resource
  ownership; repair concrete load-bearing findings before integration.
- [ ] Run format, all-target Clippy and full locked tests for compiler changes.
- [ ] Record actual hardware/loader platforms, failures/skips and limitations.
- [ ] Commit intentional files and advance local v1 only; preserve user edits.
