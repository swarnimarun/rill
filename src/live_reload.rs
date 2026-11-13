//! Selective native function replacement with versioned host-owned Copy state.
//!
//! Each registered root owns its own image and complete reachable helper cohort.
//! A selected replacement leaves unselected roots and their helpers unchanged.
//! All calls and publication require exclusive access. Window, GPU and other
//! foreign resources belong to the long-lived host, outside [`State`].

use crate::abi::copy::{Function, Schema, Value};
use crate::abi::Export;
use crate::ast::Program;
use crate::native_reload::{CopyCandidate, CopySession, NativeCompiler};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const CHECKPOINT_FORMAT: u32 = 1;
const MAX_CHECKPOINT_BYTES: usize = 1024 * 1024;
const MAX_ROOTS: usize = 64;

/// An explicit application schema version and a validated portable Copy value.
///
/// Floating values preserve their exact binary32 bits in JSON, including NaNs
/// and signed zero. No native pointers, resource handles or owner values occur
/// in this representation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    version: u32,
    schema: Schema,
    value: Value,
}

impl State {
    pub fn new(version: u32, schema: Schema, value: Value) -> Result<Self, String> {
        let state = Self {
            version,
            schema,
            value,
        };
        state.validate()?;
        Ok(state)
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    pub fn value(&self) -> &Value {
        &self.value
    }

    fn validate(&self) -> Result<(), String> {
        self.schema.validate()?;
        self.schema.validate_value(&self.value)
    }

    /// Encode one bounded, versioned checkpoint without native memory layouts.
    pub fn checkpoint_json(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        let bytes = serde_json::to_vec_pretty(&Checkpoint {
            format: CHECKPOINT_FORMAT,
            state: self.clone(),
        })
        .map_err(|error| format!("E-RELOAD-STATE: cannot encode checkpoint: {error}"))?;
        if bytes.len() > MAX_CHECKPOINT_BYTES {
            return Err("E-RELOAD-STATE: checkpoint exceeds 1 MiB".into());
        }
        Ok(bytes)
    }

    /// Validate the checkpoint format, schema and full value before use.
    ///
    /// This reads data only. It never loads code or performs an implicit
    /// migration. Serde's ordinary recursion limit remains enabled.
    pub fn from_checkpoint_json(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_CHECKPOINT_BYTES {
            return Err("E-RELOAD-STATE: checkpoint exceeds 1 MiB".into());
        }
        let checkpoint: Checkpoint = serde_json::from_slice(bytes)
            .map_err(|error| format!("E-RELOAD-STATE: invalid checkpoint JSON: {error}"))?;
        if checkpoint.format != CHECKPOINT_FORMAT {
            return Err(format!(
                "E-RELOAD-STATE: unsupported checkpoint format {}",
                checkpoint.format
            ));
        }
        checkpoint.state.validate()?;
        Ok(checkpoint.state)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    format: u32,
    state: State,
}

/// How a registered function participates in persistent application state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Binding {
    /// Arguments and result are provided directly by the host.
    Pure,
    /// Exactly one state argument, with a fixed Copy result schema.
    Query,
    /// Exactly one state argument and the same state result schema.
    Step,
}

/// A stable host name and its source root in one checked source snapshot.
///
/// `name` is the public C name and must be a nonreserved `rill_` identifier.
/// `source` must be resolved afresh when loading a module graph;
/// compilation-local symbol IDs are not persistent identifiers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Registration {
    pub name: String,
    pub source: String,
    pub binding: Binding,
}

struct Unit {
    registration: Registration,
    contract: Function,
    session: CopySession,
    generation: u64,
}

impl Unit {
    fn build(
        program: &Program,
        registration: &Registration,
        compiler: &NativeCompiler,
        generation: u64,
    ) -> Result<Self, String> {
        let candidate = CopyCandidate::build(
            program,
            &[Export {
                source: registration.source.clone(),
                c_name: registration.name.clone(),
            }],
            compiler,
        )?;
        let contract = candidate
            .functions()
            .first()
            .ok_or("E-RELOAD-STATE: candidate has no root contract")?
            .clone();
        let session = CopySession::new(candidate)?;
        Ok(Self {
            registration: registration.clone(),
            contract,
            session,
            generation,
        })
    }

    fn validate_binding(&self, state: &State) -> Result<(), String> {
        if self.registration.binding == Binding::Pure {
            return Ok(());
        }
        if self.contract.parameters.as_slice() != std::slice::from_ref(&state.schema) {
            return Err(format!(
                "E-RELOAD-STATE: root {:?} must accept exactly the application state schema",
                self.registration.name
            ));
        }
        if self.registration.binding == Binding::Step && self.contract.result != state.schema {
            return Err(format!(
                "E-RELOAD-STATE: step {:?} must return the application state schema",
                self.registration.name
            ));
        }
        Ok(())
    }

    fn bound_schema(&self) -> Result<Option<&Schema>, String> {
        if self.registration.binding == Binding::Pure {
            return Ok(None);
        }
        if self.contract.parameters.len() != 1 {
            return Err("E-RELOAD-STATE: state-bound roots require exactly one parameter".into());
        }
        let schema = &self.contract.parameters[0];
        if self.registration.binding == Binding::Step && self.contract.result != *schema {
            return Err(
                "E-RELOAD-STATE: a step must accept and return the same state schema".into(),
            );
        }
        Ok(Some(schema))
    }
}

/// An exclusive registry of independently replaceable native roots and state.
///
/// Each root and its helpers run from one image. Two roots may deliberately
/// retain different helper versions after a selective replacement. Replace
/// those roots together when they require a common behavior change.
///
/// The underlying native sessions make this type neither `Send` nor `Sync`.
/// Generated runtime traps remain process fatal. Publication does not patch
/// active frames, callbacks, arbitrary threads or in-flight GPU operations.
pub struct LiveSession {
    units: BTreeMap<String, Unit>,
    state: State,
    generation: u64,
}

impl LiveSession {
    /// Compile and load every registered root before constructing the session.
    pub fn new(
        program: &Program,
        registrations: &[Registration],
        state: State,
        compiler: &NativeCompiler,
    ) -> Result<Self, String> {
        state.validate()?;
        validate_registrations(registrations)?;
        let mut units = BTreeMap::new();
        for registration in registrations {
            let unit = Unit::build(program, registration, compiler, 0)?;
            unit.validate_binding(&state)?;
            units.insert(registration.name.clone(), unit);
        }
        Ok(Self {
            units,
            state,
            generation: 0,
        })
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    /// Supply host input without changing the active schema or state version.
    ///
    /// Invalid values leave the previous state intact. Application invariants
    /// beyond the Copy schema remain the host's responsibility.
    pub fn set_state(&mut self, value: Value) -> Result<(), String> {
        self.state.schema.validate_value(&value)?;
        self.state.value = value;
        Ok(())
    }

    /// Successful code-and-state publications since construction.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn unit_generation(&self, name: &str) -> Option<u64> {
        self.units.get(name).map(|unit| unit.generation)
    }

    pub fn registrations(&self) -> impl Iterator<Item = &Registration> {
        self.units.values().map(|unit| &unit.registration)
    }

    pub fn contract(&self, name: &str) -> Option<&Function> {
        self.units.get(name).map(|unit| &unit.contract)
    }

    pub fn build_identity(&self, name: &str) -> Option<&str> {
        self.units
            .get(name)
            .map(|unit| unit.session.build_identity())
    }

    /// Call a `Pure` root with explicit host arguments.
    pub fn call(&mut self, name: &str, arguments: &[Value]) -> Result<Value, String> {
        self.unit(name, Binding::Pure)?
            .session
            .call(name, arguments)
    }

    /// Call a `Query` root with the current state, without changing that state.
    pub fn query(&mut self, name: &str) -> Result<Value, String> {
        let state = self.state.value.clone();
        self.unit(name, Binding::Query)?
            .session
            .call(name, &[state])
    }

    /// Call a `Step` root once and publish only its validated returned state.
    pub fn step(&mut self, name: &str) -> Result<&State, String> {
        let state = self.state.value.clone();
        let value = self
            .unit(name, Binding::Step)?
            .session
            .call(name, &[state])?;
        self.state.schema.validate_value(&value)?;
        self.state.value = value;
        Ok(&self.state)
    }

    fn unit(&mut self, name: &str, binding: Binding) -> Result<&mut Unit, String> {
        let unit = self
            .units
            .get_mut(name)
            .ok_or_else(|| format!("E-RELOAD-STATE: unknown root {name:?}"))?;
        if unit.registration.binding != binding {
            return Err(format!(
                "E-RELOAD-STATE: root {name:?} is not registered as {binding:?}"
            ));
        }
        Ok(unit)
    }

    /// Stage selected roots, migrate a copy of state, then publish all together.
    ///
    /// Names and bindings must already exist. Pure signatures and query result
    /// schemas stay fixed. A changed state schema requires an increasing state
    /// version and replacement of every affected state-bound root. Even retained
    /// roots must accept the migrated state. Use `|state| Ok(state.clone())` when
    /// no migration is needed.
    ///
    /// Compilation, loading, migration and validation finish before mutation.
    /// An `Err`, or a Rust migration panic that unwinds, leaves all active code,
    /// state and generations unchanged. External side effects performed by the
    /// trusted host callback are outside this transaction. No native candidate
    /// function executes during publication.
    pub fn replace_selected<F>(
        &mut self,
        program: &Program,
        registrations: &[Registration],
        compiler: &NativeCompiler,
        migrate: F,
    ) -> Result<u64, String>
    where
        F: FnOnce(&State) -> Result<State, String>,
    {
        validate_registrations(registrations)?;
        let generation = self
            .generation
            .checked_add(1)
            .ok_or("E-RELOAD-STATE: generation counter exhausted")?;
        // Check all requested identities before any compiler or callback runs.
        for registration in registrations {
            let previous = self.units.get(&registration.name).ok_or_else(|| {
                format!(
                    "E-RELOAD-STATE: cannot replace unregistered root {:?}",
                    registration.name
                )
            })?;
            if previous.registration.binding != registration.binding {
                return Err("E-RELOAD-STATE: replacement changes a root's state binding".into());
            }
        }
        let mut candidates = BTreeMap::new();
        for registration in registrations {
            let unit = Unit::build(program, registration, compiler, generation)?;
            let previous = &self.units[&registration.name];
            match registration.binding {
                Binding::Pure if unit.contract != previous.contract => {
                    return Err("E-RELOAD-STATE: replacement changes a pure root contract".into());
                }
                Binding::Query if unit.contract.result != previous.contract.result => {
                    return Err("E-RELOAD-STATE: replacement changes a query result schema".into());
                }
                _ => {}
            }
            candidates.insert(registration.name.clone(), unit);
        }
        let mut bound_schema = None;
        for (name, previous) in &self.units {
            let unit = candidates.get(name).unwrap_or(previous);
            if let Some(schema) = unit.bound_schema()? {
                if bound_schema.is_some_and(|expected| expected != schema) {
                    return Err(
                        "E-RELOAD-STATE: selected and retained roots require different state schemas"
                            .into(),
                    );
                }
                bound_schema = Some(schema);
            }
        }
        let state = migrate(&self.state)?;
        state.validate()?;
        validate_state_version(&self.state, &state)?;
        for (name, previous) in &self.units {
            candidates
                .get(name)
                .unwrap_or(previous)
                .validate_binding(&state)?;
        }
        // Every key already exists. Assignment performs no allocation, callback
        // or fallible operation; retired generated libraries have no user drops.
        for (name, unit) in candidates {
            *self
                .units
                .get_mut(&name)
                .expect("validated registered root") = unit;
        }
        self.state = state;
        self.generation = generation;
        Ok(generation)
    }

    /// Restore a validated checkpoint, with an explicit host migration.
    ///
    /// The migrated state must exactly match the active version and schema.
    /// This changes no code or generation and preserves active state on failure.
    pub fn restore_checkpoint<F>(&mut self, bytes: &[u8], migrate: F) -> Result<(), String>
    where
        F: FnOnce(&State) -> Result<State, String>,
    {
        let checkpoint = State::from_checkpoint_json(bytes)?;
        let state = migrate(&checkpoint)?;
        state.validate()?;
        validate_state_version(&checkpoint, &state)?;
        if state.schema != self.state.schema || state.version != self.state.version {
            return Err(
                "E-RELOAD-STATE: restored state must match the active version and schema".into(),
            );
        }
        self.state = state;
        Ok(())
    }

    /// Write a checkpoint through one same-directory atomic file replacement.
    ///
    /// The temporary file is synced before rename. All reported errors occur
    /// before publication and preserve any prior checkpoint. Parent-directory
    /// syncing after publication is best effort; crash durability is not promised.
    pub fn write_checkpoint(&self, path: impl AsRef<Path>) -> Result<(), String> {
        let bytes = self.state.checkpoint_json()?;
        write_checkpoint(path.as_ref(), &bytes)
    }
}

fn validate_state_version(previous: &State, next: &State) -> Result<(), String> {
    if next.version < previous.version
        || (next.schema != previous.schema && next.version == previous.version)
    {
        return Err(
            "E-RELOAD-STATE: schema changes require an increasing application state version".into(),
        );
    }
    Ok(())
}

fn validate_registrations(registrations: &[Registration]) -> Result<(), String> {
    if registrations.is_empty() || registrations.len() > MAX_ROOTS {
        return Err("E-RELOAD-STATE: select 1 through 64 roots".into());
    }
    let mut names = std::collections::BTreeSet::new();
    for registration in registrations {
        if !names.insert(&registration.name) {
            return Err(format!(
                "E-RELOAD-STATE: duplicate root {:?}",
                registration.name
            ));
        }
    }
    Ok(())
}

struct PendingCheckpoint(PathBuf);

impl Drop for PendingCheckpoint {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn write_checkpoint(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if path.file_name().is_none() {
        return Err("E-RELOAD-STATE: checkpoint destination requires a file name".into());
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() => {
            return Err("E-RELOAD-STATE: checkpoint destination is not a regular file".into());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "E-RELOAD-STATE: cannot inspect checkpoint: {error}"
            ));
        }
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    for _ in 0..128 {
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".rill-checkpoint-{}-{sequence}.tmp",
            std::process::id()
        ));
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = match options.open(&temporary) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "E-RELOAD-STATE: cannot create checkpoint temporary: {error}"
                ));
            }
        };
        let pending = PendingCheckpoint(temporary);
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("E-RELOAD-STATE: cannot sync checkpoint: {error}"))?;
        drop(file);
        fs::rename(&pending.0, path)
            .map_err(|error| format!("E-RELOAD-STATE: cannot publish checkpoint: {error}"))?;
        if let Ok(directory) = fs::File::open(parent) {
            let _ = directory.sync_all();
        }
        return Ok(());
    }
    Err("E-RELOAD-STATE: cannot allocate a checkpoint temporary".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::copy::{Field, Variant};

    fn integer_state(value: i32) -> State {
        State::new(1, Schema::I32, Value::I32(value)).unwrap()
    }

    #[test]
    fn checkpoints_preserve_nested_copy_values_and_all_float_bits() {
        for bits in [0, 0x8000_0000, 0x7f80_0000, 0x7fc0_1234, u32::MAX] {
            let state = State::new(
                9,
                Schema::Record {
                    name: "Scene".into(),
                    fields: vec![
                        Field {
                            name: "position".into(),
                            schema: Schema::F32,
                        },
                        Field {
                            name: "phase".into(),
                            schema: Schema::Enum {
                                name: "Phase".into(),
                                variants: vec![Variant {
                                    name: "Running".into(),
                                    fields: vec![Schema::Bool, Schema::I32],
                                }],
                            },
                        },
                    ],
                },
                Value::Record(vec![
                    ("position".into(), Value::F32(bits)),
                    (
                        "phase".into(),
                        Value::Enum {
                            variant: "Running".into(),
                            fields: vec![Value::Bool(true), Value::I32(-42)],
                        },
                    ),
                ]),
            )
            .unwrap();
            let bytes = state.checkpoint_json().unwrap();
            assert_eq!(State::from_checkpoint_json(&bytes).unwrap(), state);
        }
    }

    #[test]
    fn checkpoints_reject_unknown_fields_versions_shapes_and_trailing_data() {
        let bytes = integer_state(42).checkpoint_json().unwrap();
        let original: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for field in [
            "format",
            "state",
            "extra",
            "value",
            "schema",
            "schema_extra",
            "value_extra",
        ] {
            let mut record = original.clone();
            match field {
                "format" => record["format"] = serde_json::json!(2),
                "state" => record["state"]["extra"] = serde_json::json!(true),
                "extra" => record["extra"] = serde_json::json!(true),
                "value" => {
                    record["state"]["value"] = serde_json::json!({"kind": "bool", "value": true})
                }
                "schema" => record["state"]["schema"] = serde_json::json!("Unknown"),
                "schema_extra" => record["state"]["schema"]["extra"] = serde_json::json!(true),
                "value_extra" => record["state"]["value"]["extra"] = serde_json::json!(true),
                _ => unreachable!(),
            }
            assert!(
                State::from_checkpoint_json(&serde_json::to_vec(&record).unwrap()).is_err(),
                "{field}"
            );
        }
        let mut trailing = bytes;
        trailing.extend_from_slice(b" null");
        assert!(State::from_checkpoint_json(&trailing).is_err());
        assert!(State::from_checkpoint_json(&vec![b' '; MAX_CHECKPOINT_BYTES + 1]).is_err());
        assert!(State::from_checkpoint_json(
            br#"{"format":1,"format":1,"state":{"version":1,"schema":{"kind":"i32"},"value":{"kind":"i32","value":1}}}"#
        )
        .is_err());
    }

    #[test]
    fn state_versions_never_silently_reinterpret_an_existing_schema() {
        let previous = integer_state(3);
        assert!(validate_state_version(&previous, &integer_state(4)).is_ok());
        let changed = State::new(1, Schema::Bool, Value::Bool(true)).unwrap();
        assert!(validate_state_version(&previous, &changed).is_err());
        let changed = State::new(2, Schema::Bool, Value::Bool(true)).unwrap();
        assert!(validate_state_version(&previous, &changed).is_ok());
        assert!(validate_state_version(&changed, &previous).is_err());
        let invalid: State = serde_json::from_value(serde_json::json!({
            "version": 2,
            "schema": {"kind": "i32"},
            "value": {"kind": "bool", "value": true}
        }))
        .unwrap();
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn checkpoint_file_replacement_preserves_previous_output_on_failure() {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "rill-state-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("state.json");
        let first = integer_state(1).checkpoint_json().unwrap();
        let second = integer_state(2).checkpoint_json().unwrap();
        write_checkpoint(&path, &first).unwrap();
        write_checkpoint(&path, &second).unwrap();
        assert_eq!(fs::read(&path).unwrap(), second);
        assert!(write_checkpoint(&directory, &first).is_err());
        assert_eq!(fs::read(&path).unwrap(), second);
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            let link = directory.join("link.json");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert!(write_checkpoint(&link, &first).is_err());
            assert_eq!(fs::read(&path).unwrap(), second);
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn registry() -> (LiveSession, NativeCompiler, Vec<Registration>) {
        let registrations = vec![
            Registration {
                name: "rill_update".into(),
                source: "update".into(),
                binding: Binding::Step,
            },
            Registration {
                name: "rill_observe".into(),
                source: "observe".into(),
                binding: Binding::Query,
            },
        ];
        let program = crate::parser::parse(
            "fn helper(x: i32) -> i32 { x + 1 }
             fn update(x: i32) -> i32 { helper(x) }
             fn observe(x: i32) -> i32 { helper(x) }",
        )
        .unwrap();
        let compiler = NativeCompiler::default();
        let session =
            LiveSession::new(&program, &registrations, integer_state(10), &compiler).unwrap();
        (session, compiler, registrations)
    }

    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn selected_roots_retain_unselected_helper_cohorts() {
        let (mut session, compiler, registrations) = registry();
        session.step("rill_update").unwrap();
        assert_eq!(session.state().value(), &Value::I32(11));
        let candidate = crate::parser::parse(
            "fn helper(x: i32) -> i32 { x + 9 }
             fn update(x: i32) -> i32 { helper(x) }
             fn observe(x: i32) -> i32 { helper(x) }",
        )
        .unwrap();
        session
            .replace_selected(&candidate, &registrations[..1], &compiler, |state| {
                Ok(state.clone())
            })
            .unwrap();
        session.step("rill_update").unwrap();
        assert_eq!(session.state().value(), &Value::I32(20));
        assert_eq!(session.query("rill_observe").unwrap(), Value::I32(21));
        assert_eq!(session.unit_generation("rill_update"), Some(1));
        assert_eq!(session.unit_generation("rill_observe"), Some(0));
        assert!(session.set_state(Value::Bool(true)).is_err());
        assert_eq!(session.state().value(), &Value::I32(20));
        session.set_state(Value::I32(30)).unwrap();
        assert_eq!(session.query("rill_observe").unwrap(), Value::I32(31));
        assert!(session.call("rill_update", &[Value::I32(1)]).is_err());
        assert!(session.step("rill_observe").is_err());
    }

    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn migration_errors_panics_and_generation_exhaustion_preserve_every_root() {
        let (mut session, compiler, registrations) = registry();
        let candidate = crate::parser::parse(
            "fn update(x: i32) -> i32 { x + 9 }
             fn observe(x: i32) -> i32 { x + 8 }",
        )
        .unwrap();
        assert!(session
            .replace_selected(&candidate, &registrations, &compiler, |_| Err(
                "rejected".into()
            ))
            .is_err());
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = session.replace_selected(&candidate, &registrations, &compiler, |_| {
                panic!("migration panicked")
            });
        }));
        assert!(panic.is_err());
        assert_eq!(session.generation(), 0);
        assert_eq!(session.unit_generation("rill_update"), Some(0));
        assert_eq!(session.unit_generation("rill_observe"), Some(0));
        assert_eq!(session.state(), &integer_state(10));
        assert_eq!(session.query("rill_observe").unwrap(), Value::I32(11));
        session.step("rill_update").unwrap();
        assert_eq!(session.state().value(), &Value::I32(11));
        session.generation = u64::MAX;
        let callback_ran = std::cell::Cell::new(false);
        assert!(session
            .replace_selected(&candidate, &registrations, &compiler, |state| {
                callback_ran.set(true);
                Ok(state.clone())
            })
            .is_err());
        assert!(!callback_ran.get());
        assert_eq!(session.query("rill_observe").unwrap(), Value::I32(12));
    }

    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn malformed_code_and_incompatible_metadata_never_invoke_migration() {
        let (mut session, compiler, registrations) = registry();
        for source in [
            "fn update(x: i32) -> i32 { missing(x) }",
            "fn update(x: i32, y: i32) -> i32 { x + y }",
            "fn update(x: bool) -> bool { x }",
        ] {
            let program = crate::parser::parse(source).unwrap();
            let callback_ran = std::cell::Cell::new(false);
            assert!(session
                .replace_selected(&program, &registrations[..1], &compiler, |state| {
                    callback_ran.set(true);
                    Ok(state.clone())
                })
                .is_err());
            assert!(!callback_ran.get(), "{source}");
            assert_eq!(session.generation(), 0);
            assert_eq!(session.state(), &integer_state(10));
            assert_eq!(session.query("rill_observe").unwrap(), Value::I32(11));
        }
    }

    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn restore_requires_valid_json_and_explicit_forward_migration() {
        let (mut session, _, _) = registry();
        let checkpoint = integer_state(50).checkpoint_json().unwrap();
        session
            .restore_checkpoint(&checkpoint, |state| Ok(state.clone()))
            .unwrap();
        assert_eq!(session.state(), &integer_state(50));
        assert!(session
            .restore_checkpoint(b"garbage", |state| Ok(state.clone()))
            .is_err());
        let newer = State::new(2, Schema::I32, Value::I32(3))
            .unwrap()
            .checkpoint_json()
            .unwrap();
        assert!(session
            .restore_checkpoint(&newer, |_| Ok(integer_state(3)))
            .is_err());
        let older = State::new(0, Schema::I32, Value::I32(7))
            .unwrap()
            .checkpoint_json()
            .unwrap();
        session
            .restore_checkpoint(&older, |state| {
                State::new(1, state.schema().clone(), state.value().clone())
            })
            .unwrap();
        assert_eq!(session.state(), &integer_state(7));
        assert_eq!(session.generation(), 0);
    }
}
