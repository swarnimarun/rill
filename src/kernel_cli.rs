use rill::kernel_ir::{Kernel, Target, MAX_BYTES};
use std::{
    ffi::OsString,
    fs::File,
    io::{self, Read, Write},
    path::Path,
};

pub const USAGE: &str = "rill kernel lower <file.rill> <function>\n       rill kernel eval <file.rir> [signed-i32 ...]\n       rill kernel emit-llvm <nvptx|vulkan> <file.rir>";

fn read_kernel(file: &Path) -> Result<Kernel, String> {
    let mut text = String::new();
    File::open(file)
        .and_then(|source| source.take(MAX_BYTES + 1).read_to_string(&mut text))
        .map_err(|e| format!("{}: {e}", file.display()))?;
    Kernel::parse(&text).map_err(|e| format!("{}: {e}", file.display()))
}

pub fn run(mut args: impl Iterator<Item = OsString>) -> Result<(), String> {
    let command = args.next().ok_or(USAGE)?;
    let mut out = io::BufWriter::new(io::stdout().lock());
    match command.to_str() {
        Some("lower") => {
            let file = args.next().ok_or(USAGE)?;
            let name = args.next().ok_or(USAGE)?;
            if args.next().is_some() {
                return Err(USAGE.into());
            }
            let text = rill::kernel_source::lower(Path::new(&file), name.to_str().ok_or(USAGE)?)?;
            out.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        }
        Some("eval") => {
            let file = args.next().ok_or(USAGE)?;
            let kernel = read_kernel(Path::new(&file))?;
            for arg in args {
                let n = arg
                    .to_str()
                    .and_then(|s| s.parse().ok())
                    .ok_or("expected signed i32 input")?;
                writeln!(out, "{}", kernel.eval(n)).map_err(|e| e.to_string())?;
            }
        }
        Some("emit-llvm") => {
            let target = match args.next().as_deref().and_then(|s| s.to_str()) {
                Some("nvptx") => Target::Nvptx,
                Some("vulkan") => Target::Vulkan,
                _ => return Err(USAGE.into()),
            };
            let file = args.next().ok_or(USAGE)?;
            if args.next().is_some() {
                return Err(USAGE.into());
            }
            let module = read_kernel(Path::new(&file))?.emit_llvm(target);
            out.write_all(module.as_bytes())
                .map_err(|e| e.to_string())?;
        }
        _ => return Err(USAGE.into()),
    }
    out.flush().map_err(|e| e.to_string())
}
