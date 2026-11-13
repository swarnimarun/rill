//! Shared state, source validation and reload loop for the reload-renderer.

use rill::abi::copy::{Field, Schema, Value};
use rill::live_reload::{Binding, LiveSession, Registration, State};
use rill::native_reload::{NativeCompiler, Optimization};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct Options {
    pub source: PathBuf,
    pub checkpoint: PathBuf,
    pub renderer: Option<PathBuf>,
    pub headless: bool,
    pub frames: u32,
    pub interval: Duration,
    pub optimization: Optimization,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            source: "examples/reload-renderer/logic.rill".into(),
            checkpoint: "target/reload-renderer/state.json".into(),
            renderer: None,
            headless: false,
            frames: 0,
            interval: Duration::from_millis(16),
            optimization: Optimization::O0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub ticks: i32,
    pub x: i32,
    pub color: i32,
    pub state_version: u32,
    pub generation: u64,
    pub update_generation: u64,
    pub color_generation: u64,
}

pub trait Renderer {
    fn frame(&mut self, x: i32, color: i32) -> Result<bool, String>;
    fn close(&mut self) -> Result<(), String>;
}

pub fn registrations() -> Vec<Registration> {
    vec![
        Registration {
            name: "rill_update".into(),
            source: "update".into(),
            binding: Binding::Step,
        },
        Registration {
            name: "rill_color".into(),
            source: "color".into(),
            binding: Binding::Query,
        },
    ]
}

pub fn schema(version: u32) -> Result<Schema, String> {
    if !matches!(version, 1 | 2) {
        return Err(format!("unsupported example state version {version}"));
    }
    let mut fields = vec![
        Field {
            name: "ticks".into(),
            schema: Schema::I32,
        },
        Field {
            name: "x".into(),
            schema: Schema::I32,
        },
    ];
    if version == 2 {
        fields.push(Field {
            name: "speed".into(),
            schema: Schema::I32,
        });
    }
    Ok(Schema::Record {
        name: "Scene".into(),
        fields,
    })
}

pub fn migrate(old: &State, version: u32) -> Result<State, String> {
    if old.schema() != &schema(old.version())? {
        return Err("checkpoint has an unexpected Scene schema".into());
    }
    if old.version() == version {
        validate_scene(old)?;
        return Ok(old.clone());
    }
    if old.version() != 1 || version != 2 {
        return Err(format!(
            "no migration from state {} to {version}",
            old.version()
        ));
    }
    validate_scene(old)?;
    State::new(
        2,
        schema(2)?,
        Value::Record(vec![
            ("ticks".into(), Value::I32(integer(old.value(), "ticks")?)),
            ("x".into(), Value::I32(integer(old.value(), "x")?)),
            ("speed".into(), Value::I32(2)),
        ]),
    )
}

pub fn integer(value: &Value, name: &str) -> Result<i32, String> {
    value
        .field(name)
        .and_then(Value::as_i32)
        .ok_or_else(|| format!("expected integer Scene field {name}"))
}

fn validate_scene(state: &State) -> Result<(), String> {
    let ticks = integer(state.value(), "ticks")?;
    let x = integer(state.value(), "x")?;
    if !(0..1_000_000).contains(&ticks) || !(0..500).contains(&x) {
        return Err("Scene ticks or x is outside the example's accepted range".into());
    }
    if state.version() == 2 && !(1..=100).contains(&integer(state.value(), "speed")?) {
        return Err("Scene speed must be in 1..100".into());
    }
    Ok(())
}

fn read_bounded(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "example input exceeds 1 MiB",
        ));
    }
    Ok(bytes)
}

fn source(path: &Path) -> Result<(Vec<u8>, u32, rill::ast::Program), String> {
    let bytes = read_bounded(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let text = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
    let version = text
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("// state-version: "))
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or("first source line must be // state-version: 1 or 2")?;
    schema(version)?;
    let program = rill::parser::parse(text)?;
    Ok((bytes, version, program))
}

pub fn run(
    options: Options,
    mut renderer: Option<&mut dyn Renderer>,
    mut observe: impl FnMut(&Frame),
) -> Result<(), String> {
    if options.headless && (options.renderer.is_some() || renderer.is_some()) {
        return Err("--headless runs logic only and cannot select a renderer".into());
    }
    if !options.headless && renderer.is_none() {
        return Err("a windowed run requires a renderer adapter".into());
    }
    let compiler = NativeCompiler::new(
        std::env::var_os("CC").unwrap_or_else(|| "cc".into()),
        options.optimization,
    );
    let (mut observed, version, program) = source(&options.source)?;
    let initial = State::new(
        1,
        schema(1)?,
        Value::Record(vec![
            ("ticks".into(), Value::I32(0)),
            ("x".into(), Value::I32(0)),
        ]),
    )?;
    let mut session = LiveSession::new(
        &program,
        &registrations(),
        migrate(&initial, version)?,
        &compiler,
    )?;
    match read_bounded(&options.checkpoint) {
        Ok(bytes) => session.restore_checkpoint(&bytes, |old| migrate(old, version))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(format!("read checkpoint: {e}")),
    }
    if let Some(parent) = options
        .checkpoint
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut frames = 0_u32;
    loop {
        if let Ok(bytes) = read_bounded(&options.source) {
            if bytes != observed {
                observed = bytes;
                let replacement = source(&options.source).and_then(|(_, version, program)| {
                    let all = registrations();
                    let selected = if version == session.state().version() {
                        &all[..1]
                    } else {
                        &all[..]
                    };
                    session.replace_selected(&program, selected, &compiler, |old| {
                        migrate(old, version)
                    })
                });
                match replacement {
                    Ok(generation) => eprintln!("accepted logic generation {generation}"),
                    Err(error) => eprintln!("candidate rejected; keeping code and state: {error}"),
                }
            }
        }
        session.step("rill_update")?;
        if !(0..1_000_000).contains(&integer(session.state().value(), "ticks")?)
            || !(0..500).contains(&integer(session.state().value(), "x")?)
        {
            return Err("Scene ticks or x is outside the example's accepted range".into());
        }
        if session.state().version() == 2
            && !(1..=100).contains(&integer(session.state().value(), "speed")?)
        {
            return Err("Scene speed must be in 1..100".into());
        }
        let color = session
            .query("rill_color")?
            .as_i32()
            .ok_or("color must return i32")?;
        if !(0..=255).contains(&color) {
            return Err("color must be in 0..255".into());
        }
        let frame = Frame {
            ticks: integer(session.state().value(), "ticks")?,
            x: integer(session.state().value(), "x")?,
            color,
            state_version: session.state().version(),
            generation: session.generation(),
            update_generation: session
                .unit_generation("rill_update")
                .ok_or("missing update")?,
            color_generation: session
                .unit_generation("rill_color")
                .ok_or("missing color")?,
        };
        if let Some(renderer) = renderer.as_deref_mut() {
            if renderer.frame(frame.x, frame.color)? {
                break;
            }
        }
        session.write_checkpoint(&options.checkpoint)?;
        observe(&frame);
        frames = frames.checked_add(1).ok_or("frame counter exhausted")?;
        if options.frames != 0 && frames >= options.frames {
            break;
        }
        std::thread::sleep(options.interval);
    }
    if let Some(renderer) = renderer {
        renderer.close()?;
    }
    Ok(())
}
