//! Run with `cargo run --example native_reload_host` on macOS or Linux.
use rill::abi::Export;
use rill::native_reload::{NativeCandidate, NativeCompiler, NativeSession};

fn main() -> Result<(), String> {
    let roots = [Export {
        source: "update".into(),
        c_name: "rill_update".into(),
    }];
    let compiler = NativeCompiler::default();
    let first = rill::parser::parse("fn update(state: i32) -> i32 { state + 1 }")?;
    let mut session = NativeSession::new(NativeCandidate::build(&first, &roots, &compiler)?)?;
    let mut state = session.call_i32("rill_update", &[10])?;
    println!("generation {}: {state}", session.generation());

    // Build while keeping the old generation available, then replace between
    // calls. The host retains state; no native frame or allocation is migrated.
    let second = rill::parser::parse("fn update(state: i32) -> i32 { state + 7 }")?;
    let candidate = NativeCandidate::build(&second, &roots, &compiler)?;
    session.replace(candidate)?;
    state = session.call_i32("rill_update", &[state])?;
    println!("generation {}: {state}", session.generation());
    Ok(())
}
