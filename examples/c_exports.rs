//! Emit the checked scalar library used by examples/c-exports/host.c.

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let output = PathBuf::from(arguments.next().ok_or("usage: c_exports <new-directory>")?);
    if arguments.next().is_some() {
        return Err("usage: c_exports <new-directory>".into());
    }
    let program = rill::parser::parse(include_str!("c-exports/update.rill"))?;
    let artifact = rill::abi::emit_i32_library(
        &program,
        &[rill::abi::Export {
            source: "update".into(),
            c_name: "rill_update".into(),
        }],
    )?;
    // Require a new directory, so this example cannot replace existing files.
    std::fs::create_dir(&output)?;
    std::fs::write(output.join("update.c"), artifact.c)?;
    std::fs::write(output.join("update.h"), artifact.header)?;
    std::fs::write(output.join("update.abi.json"), artifact.manifest)?;
    println!("{}", output.display());
    Ok(())
}
