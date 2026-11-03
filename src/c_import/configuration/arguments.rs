//! The opt-in profile grammar. Legacy argument handling is intentionally separate.
use super::{error, MAX_ARGUMENT, MAX_ITEMS};
use std::{
    collections::HashSet,
    ffi::{OsStr, OsString},
    os::unix::ffi::OsStrExt,
    path::Path,
};

pub(super) fn bytes(value: &OsStr, field: &str) -> Result<(), String> {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > MAX_ARGUMENT || bytes.contains(&0) {
        return Err(error(format!(
            "invalid {field}: expected 1..={MAX_ARGUMENT} bytes without NUL"
        )));
    }
    Ok(())
}
pub(super) fn operand(value: &OsStr, field: &str) -> Result<(), String> {
    bytes(value, field)?;
    if matches!(value.as_bytes()[0], b'-' | b'@') {
        return Err(error(format!(
            "ambiguous {field} operand {value:?}; use an absolute path"
        )));
    }
    Ok(())
}
fn macro_operand(value: &OsStr, define: bool) -> Result<(), String> {
    bytes(value, "macro")?;
    let value = value
        .to_str()
        .ok_or_else(|| error("macro names/values must be UTF-8"))?;
    let name = if define {
        value.split_once('=').map_or(value, |(n, _)| n)
    } else {
        value
    };
    if !super::super::selected::identifier(name) || name.len() > 256 {
        return Err(error(format!(
            "unsupported macro name {name:?}; function-like macros are not allowed"
        )));
    }
    if name.starts_with("__")
        || matches!(
            name,
            "_Static_assert"
                | "_Bool"
                | "_Generic"
                | "_Alignof"
                | "sizeof"
                | "int32_t"
                | "uint8_t"
                | "uint32_t"
                | "uint64_t"
                | "int64_t"
                | "CHAR_BIT"
                | "INT32_MIN"
                | "INT32_MAX"
                | "UINT8_MAX"
                | "UINT32_MAX"
                | "UINT64_MAX"
                | "INT64_MIN"
                | "INT64_MAX"
                | "INT32_C"
                | "INT64_C"
                | "UINT8_C"
                | "UINT32_C"
                | "UINT64_C"
        )
    {
        return Err(error(format!(
            "macro {name:?} is reserved for native validation"
        )));
    }
    Ok(())
}
fn unique(seen: &mut HashSet<&'static str>, category: &'static str) -> Result<(), String> {
    if !seen.insert(category) {
        return Err(error(format!("repeated or conflicting {category} option")));
    }
    Ok(())
}

/// Validate the complete ordered package-plus-explicit argv before libclang or C.
pub fn validate_c_args(flags: &[OsString]) -> Result<(), String> {
    if flags.len() > MAX_ITEMS {
        return Err(error("more than 1024 C arguments"));
    }
    let mut args = flags.iter();
    let mut seen = HashSet::new();
    while let Some(arg) = args.next() {
        bytes(arg, "C argument")?;
        let text = arg.to_str().ok_or_else(|| {
            error("C option must be UTF-8; separate path operands may be non-UTF-8")
        })?;
        if [
            "-I",
            "-D",
            "-U",
            "-isystem",
            "-iquote",
            "-idirafter",
            "-include",
            "-imacros",
            "-isysroot",
            "--sysroot",
            "-target",
            "--target",
            "-arch",
        ]
        .contains(&text)
        {
            let value = args
                .next()
                .ok_or_else(|| error(format!("missing operand for {text}")))?;
            match text {
                "-D" => macro_operand(value, true)?,
                "-U" => macro_operand(value, false)?,
                _ => operand(value, text)?,
            }
            match text {
                "-target" | "--target" => unique(&mut seen, "target")?,
                "-arch" => unique(&mut seen, "architecture")?,
                "-isysroot" | "--sysroot" => unique(&mut seen, "sysroot")?,
                _ => {}
            }
        } else if let Some(value) = text.strip_prefix("-D").filter(|v| !v.is_empty()) {
            macro_operand(OsStr::new(value), true)?;
        } else if let Some(value) = text.strip_prefix("-U").filter(|v| !v.is_empty()) {
            macro_operand(OsStr::new(value), false)?;
        } else if let Some(value) = text.strip_prefix("-I").filter(|v| !v.is_empty()) {
            operand(OsStr::new(value), "-I")?;
        } else if let Some((prefix, category)) = [
            ("--sysroot=", "sysroot"),
            ("--target=", "target"),
            ("-march=", "architecture"),
            ("-mcpu=", "cpu"),
            ("-mabi=", "ABI"),
            ("-mfpu=", "FPU"),
            ("-mfloat-abi=", "float ABI"),
        ]
        .iter()
        .find(|(p, _)| text.starts_with(p))
        {
            operand(OsStr::new(&text[prefix.len()..]), prefix)?;
            unique(&mut seen, category)?;
        } else {
            match text {
                "-O0" | "-O2" => unique(&mut seen, "optimization")?,
                "-m32" | "-m64" => unique(&mut seen, "pointer ABI")?,
                "-fsigned-char" | "-funsigned-char" => unique(&mut seen, "char signedness")?,
                "-fshort-wchar" | "-fshort-enums" | "-fms-extensions" | "-nostdinc"
                | "-pthread" | "-std=c11" => {}
                _ => return Err(error(format!("unsupported configured C argument {arg:?}"))),
            }
        }
    }
    Ok(())
}

/// A closed link grammar, preserving order and duplicates. No driver escape hatches.
pub fn validate_link_args(flags: &[OsString]) -> Result<(), String> {
    if flags.len() > MAX_ITEMS {
        return Err(error("more than 1024 link arguments"));
    }
    let mut args = flags.iter();
    let mut groups = 0usize;
    while let Some(arg) = args.next() {
        bytes(arg, "link argument")?;
        if Path::new(arg).is_absolute() {
            let raw = arg.as_bytes();
            let file = Path::new(arg)
                .file_name()
                .ok_or_else(|| error("link input must be a file"))?
                .as_bytes();
            let shared = file.windows(4).position(|w| w == b".so.").is_some_and(|p| {
                file[p + 4..]
                    .split(|b| *b == b'.')
                    .all(|part| !part.is_empty() && part.iter().all(u8::is_ascii_digit))
            });
            if !(raw.ends_with(b".o")
                || raw.ends_with(b".a")
                || raw.ends_with(b".so")
                || raw.ends_with(b".dylib")
                || shared)
            {
                return Err(error(format!(
                    "unsupported explicit native link input {arg:?}"
                )));
            }
            continue;
        }
        let text = arg.to_str().ok_or_else(|| {
            error("link options must be UTF-8; absolute native inputs may be non-UTF-8")
        })?;
        if text == "-L" {
            operand(
                args.next().ok_or_else(|| error("missing -L operand"))?,
                "-L",
            )?;
        } else if let Some(path) = text.strip_prefix("-L").filter(|p| !p.is_empty()) {
            operand(OsStr::new(path), "-L")?;
        } else if let Some(name) = text.strip_prefix("-l").filter(|n| !n.is_empty()) {
            super::super::Config {
                links: vec![name.to_owned()],
                ..Default::default()
            }
            .validate()
            .map_err(error)?;
        } else if let Some(path) = text.strip_prefix("-Wl,-rpath,") {
            operand(OsStr::new(path), "rpath")?;
            if path
                .bytes()
                .any(|b| b == b',' || b.is_ascii_control() || b == b'$')
            {
                return Err(error(
                    "rpath cannot contain comma, control characters or variable syntax",
                ));
            }
        } else {
            match text {
                "-pthread" | "-Wl,--as-needed" | "-Wl,--no-as-needed" => {}
                "-Wl,--start-group" => groups += 1,
                "-Wl,--end-group" if groups > 0 => groups -= 1,
                _ => {
                    return Err(error(format!(
                        "unsupported configured link argument {arg:?}"
                    )))
                }
            }
        }
    }
    if groups != 0 {
        return Err(error("unbalanced linker group arguments"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStringExt;
    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }
    #[test]
    fn strict_c_grammar_preserves_safe_operands_and_rejects_bypasses() {
        validate_c_args(&args(&[
            "-I",
            "space ; ' \\",
            "-DVALUE=$(touch marker)",
            "-O2",
            "--target=x86_64-linux-gnu",
        ]))
        .unwrap();
        validate_c_args(&["-I".into(), OsString::from_vec(b"path \xff".to_vec())]).unwrap();
        for values in [
            vec!["-E"],
            vec!["-S"],
            vec!["-c"],
            vec!["-o"],
            vec!["-x"],
            vec!["-xc++"],
            vec!["-std=c++17"],
            vec!["--"],
            vec!["@args"],
            vec!["extra.c"],
            vec!["-Xclang"],
            vec!["-Wp,-P"],
            vec!["-include", "-E"],
            vec!["-I", "@args"],
            vec!["-I"],
            vec!["-I", ""],
            vec!["-resource-dir=/tmp"],
            vec!["-D_Static_assert(x,y)="],
            vec!["-D__builtin_types_compatible_p(a,b)=1"],
            vec!["-UCHAR_BIT"],
            vec!["-DINT32_MAX=1"],
            vec!["-O0", "-O2"],
            vec!["-m32", "-m64"],
            vec!["--target=x", "-target", "x"],
            vec!["-DF(x)=1"],
        ] {
            assert!(validate_c_args(&args(&values)).is_err(), "{values:?}");
        }
    }
    #[test]
    fn ordered_link_inputs_are_closed_and_not_normalized() {
        let flags = args(&[
            "-Wl,--start-group",
            "-la",
            "-lb",
            "-la",
            "-Wl,--end-group",
            "-L",
            "path with spaces",
            "/tmp/a file.o",
            "-Wl,-rpath,/tmp/lib",
        ]);
        let old = flags.clone();
        validate_link_args(&flags).unwrap();
        assert_eq!(flags, old);
        validate_link_args(&[OsString::from_vec(b"/tmp/a \xff.o".to_vec())]).unwrap();
        for value in [
            "-E",
            "-o",
            "@response",
            "-Xlinker",
            "-Wl,-plugin,a",
            "/tmp/extra.c",
            "-Wl,-rpath,/tmp/a,b",
            "-Wl,--end-group",
            "-Wl,--start-group",
        ] {
            assert!(validate_link_args(&args(&[value])).is_err(), "{value}");
        }
    }
}
