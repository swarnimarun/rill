use rill::{parser, reload::Session};

fn main() -> Result<(), String> {
    let mut session = Session::new(
        parser::parse("fn update(state) { state + 1 }")?,
        &["update"],
    )?;
    let state = session.call_i32("update", &[40])?;
    session.replace(parser::parse("fn update(state) { state + 2 }")?)?;
    let state = session.call_i32("update", &[state])?;
    println!("generation {}: state {state}", session.generation());
    Ok(())
}
