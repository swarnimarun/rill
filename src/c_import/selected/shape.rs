//! Pure bounded shape guard for generated selected-C modules.
use super::{Automatic, Selection};
use crate::c_import::Config;
use std::collections::HashSet;

fn protocol_error() -> String {
    "E-C-IMPORT: invalid generated binding module shape".into()
}

// Reject anything beyond the shallow generated subset before invoking the
// general parser/checker in the parent. In particular, arbitrary worker text
// cannot introduce recursive Rill functions/types or expression trees here.
fn parse(module: &str, native: &Config) -> Result<crate::parser::ParsedModule, String> {
    if module.is_empty() || module.len() > 1024 * 1024 {
        return Err(protocol_error());
    }
    let mut declarations = 0;
    for line in module.lines() {
        if line.len() > 64 * 1024 {
            return Err(protocol_error());
        }
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        declarations += 1;
        if declarations > 1024 * 3 + 256 || line.len() > 64 * 1024 {
            return Err(protocol_error());
        }
        if let Some(value) = line
            .strip_prefix("@include(\"")
            .or_else(|| line.strip_prefix("@link(\""))
            .or_else(|| line.strip_prefix("@pkg_config(\""))
        {
            let value = value.strip_suffix("\");").ok_or_else(protocol_error)?;
            if !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_./-+".contains(&b))
            {
                return Err(protocol_error());
            }
            continue;
        }
        if !(line.starts_with("export extern fn ") || line.starts_with("export extern const "))
            || !line.ends_with(';')
            || line.bytes().filter(|b| *b == b';').count() != 1
        {
            return Err(protocol_error());
        }
        let (mut parens, mut angles) = (0i32, 0i32);
        let mut previous = 0;
        for b in line.bytes() {
            if !(b.is_ascii_alphanumeric() || b" _:,();<>-".contains(&b)) {
                return Err(protocol_error());
            }
            match b {
                b'(' => parens += 1,
                b')' => parens -= 1,
                b'<' => angles += 1,
                b'>' if previous != b'-' => angles -= 1,
                _ => {}
            }
            if !(0..=1).contains(&parens) || !(0..=1).contains(&angles) {
                return Err(protocol_error());
            }
            previous = b;
        }
        if parens != 0 || angles != 0 {
            return Err(protocol_error());
        }
    }
    let parsed = crate::parser::parse_module(module).map_err(|_| protocol_error())?;
    let program = &parsed.program;
    if !program.functions.is_empty()
        || !program.types.is_empty()
        || program.allocation_types.is_some()
        || program.externs.len() + program.constants.len() > 256
        || program.externs.iter().any(|f| f.params.len() > 128)
        || program.native != *native
        || parsed.exports.len() != program.externs.len() + program.constants.len()
    {
        return Err(protocol_error());
    }
    Ok(parsed)
}

/// Require the exact selected names in their original order.
pub fn validate(module: &str, selection: &Selection) -> Result<(), String> {
    let parsed = parse(module, &selection.native)?;
    if !parsed
        .program
        .externs
        .iter()
        .map(|f| &f.name)
        .eq(selection.functions.iter())
        || !parsed
            .program
            .constants
            .iter()
            .map(|c| &c.name)
            .eq(selection.constants.iter())
    {
        return Err(protocol_error());
    }
    Ok(())
}

/// Automatic lookup can omit absent names but cannot add unrelated declarations.
pub fn validate_automatic(module: &str, request: &Automatic) -> Result<(), String> {
    let parsed = parse(module, &request.native)?;
    let expected: HashSet<_> = request.names.iter().collect();
    let mut actual = HashSet::new();
    for name in parsed
        .program
        .externs
        .iter()
        .map(|f| &f.name)
        .chain(parsed.program.constants.iter().map(|c| &c.name))
    {
        if !expected.contains(name) || !actual.insert(name) {
            return Err(protocol_error());
        }
    }
    Ok(())
}
