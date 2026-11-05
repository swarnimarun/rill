use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};

use super::{
    error,
    manifest::{confined, Entry},
    read_bounded, MAX_SOURCE,
};

pub(super) type Sources = BTreeMap<PathBuf, Vec<u8>>;

pub(super) fn dependency_path(root: &Path, cwd: &Path, input: &Path) -> Result<PathBuf, String> {
    let input = if input.is_absolute() {
        input.to_path_buf()
    } else {
        cwd.join(input)
    };
    let mut normalized = PathBuf::new();
    for component in input.components() {
        match component {
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(error("INCLUDE", "dependency escapes the shader project"));
                }
            }
            Component::CurDir => {}
            component => normalized.push(component),
        }
    }
    let relative = normalized.strip_prefix(root).map_err(|_| error("INCLUDE", format!("dependency {} is outside the shader project; copy it into the project and use a local include/import", normalized.display())))?;
    confined(root, relative, false)?;
    Ok(relative.to_path_buf())
}

pub(super) fn depfile(path: &Path) -> Result<Vec<PathBuf>, String> {
    let bytes = read_bounded(path, super::MAX_METADATA)?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| error("INCLUDE", "compiler dependency file must be UTF-8"))?;
    let text = text.replace("\\\r\n", "").replace("\\\n", "");
    let (_, text) = text.split_once(':').ok_or_else(|| {
        error(
            "INCLUDE",
            "compiler did not produce a Make-style dependency file",
        )
    })?;
    let mut words = Vec::new();
    let mut word = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => word.push(
                chars
                    .next()
                    .ok_or_else(|| error("INCLUDE", "unterminated dependency escape"))?,
            ),
            '$' if chars.peek() == Some(&'$') => {
                chars.next();
                word.push('$');
            }
            '#' => while chars.next().is_some_and(|c| c != '\n') {},
            c if c.is_whitespace() => {
                if !word.is_empty() {
                    words.push(PathBuf::from(std::mem::take(&mut word)));
                }
            }
            c => word.push(c),
        }
        if words.len() > 4096 {
            return Err(error(
                "LIMIT",
                "compiler reported more than 4096 shader dependencies",
            ));
        }
    }
    if !word.is_empty() {
        words.push(PathBuf::from(word));
    }
    if words.is_empty() {
        return Err(error("INCLUDE", "compiler reported no shader dependencies"));
    }
    Ok(words)
}

pub(super) fn capture(
    root: &Path,
    cwd: &Path,
    dependencies: Vec<PathBuf>,
) -> Result<Sources, String> {
    let mut sources = Sources::new();
    let mut total = 0;
    for path in dependencies {
        let relative = dependency_path(root, cwd, &path)?;
        if sources.contains_key(&relative) {
            continue;
        }
        let bytes = read_bounded(&root.join(&relative), MAX_SOURCE)?;
        total += bytes.len();
        if total > 16 * 1024 * 1024 || sources.len() >= 4096 {
            return Err(error(
                "LIMIT",
                "shader dependencies exceed 4096 files or 16 MiB total",
            ));
        }
        sources.insert(relative, bytes);
    }
    Ok(sources)
}

pub(super) fn unchanged(root: &Path, sources: &Sources) -> Result<(), String> {
    for (relative, bytes) in sources {
        let path = confined(root, relative, false)?;
        if read_bounded(&path, MAX_SOURCE)? != *bytes {
            return Err(error(
                "CHANGED",
                format!(
                    "{} changed during compilation; retry the build",
                    path.display()
                ),
            ));
        }
    }
    Ok(())
}

pub(super) fn snapshot(path: &Path, entry: &Entry, sources: &Sources) -> Result<(), String> {
    fs::create_dir(path).map_err(|e| error("OUTPUT", e.to_string()))?;
    for dir in &entry.include_dirs {
        fs::create_dir_all(path.join(dir)).map_err(|e| error("OUTPUT", e.to_string()))?;
    }
    for (relative, bytes) in sources {
        let destination = path.join(relative);
        fs::create_dir_all(destination.parent().unwrap())
            .map_err(|e| error("OUTPUT", e.to_string()))?;
        fs::write(destination, bytes).map_err(|e| error("OUTPUT", e.to_string()))?;
    }
    Ok(())
}

pub(super) fn bundle(root: &Path, entry: &Entry) -> Result<(Vec<u8>, Sources), String> {
    let mut sources = Sources::new();
    let mut active = BTreeSet::new();
    let mut total = 0;
    let result = expand(
        root,
        &entry.source,
        &entry.include_dirs,
        &mut sources,
        &mut active,
        &mut total,
    )?;
    Ok((result.into_bytes(), sources))
}
fn expand(
    root: &Path,
    relative: &Path,
    dirs: &[PathBuf],
    sources: &mut Sources,
    active: &mut BTreeSet<PathBuf>,
    total: &mut usize,
) -> Result<String, String> {
    if active.len() >= 32 || !active.insert(relative.to_path_buf()) {
        return Err(error(
            "INCLUDE",
            format!(
                "recursive or deeper than 32 Metal includes: {}",
                relative.display()
            ),
        ));
    }
    let path = confined(root, relative, false)?;
    // Every occurrence must use the same captured bytes. Re-reading an include
    // could mix two versions in the bundle while metadata only described the
    // final one. The caller checks the captured graph against disk afterwards.
    let bytes = if let Some(bytes) = sources.get(relative) {
        bytes.clone()
    } else {
        let bytes = read_bounded(&path, MAX_SOURCE)?;
        *total += bytes.len();
        if sources.len() >= 4096 || *total > 16 * 1024 * 1024 {
            return Err(error(
                "LIMIT",
                "Metal dependency graph exceeds 4096 files or 16 MiB",
            ));
        }
        sources.insert(relative.to_path_buf(), bytes.clone());
        bytes
    };
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| error("INPUT", format!("{} must be UTF-8", path.display())))?;
    let source_name = serde_json::to_string(&relative.to_string_lossy()).unwrap();
    let mut output = format!("#line 1 {source_name}\n");
    let mut block_comment = false;
    for (line_index, line) in text.lines().enumerate() {
        let visible = without_comments(line, &mut block_comment);
        let trimmed = visible.trim_start();
        let include = trimmed
            .strip_prefix('#')
            .map(str::trim_start)
            .and_then(|s| s.strip_prefix("include"))
            .filter(|s| {
                s.starts_with(char::is_whitespace) || s.starts_with('"') || s.starts_with('<')
            });
        if let Some(include) = include {
            let include = include.trim();
            if let Some(name) = include.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
                let requested = Path::new(name);
                if requested.is_absolute()
                    || name.contains('\\')
                    || name.chars().any(char::is_control)
                {
                    return Err(error(
                        "INCLUDE",
                        format!(
                            "{}:{}: include must be a local literal path",
                            relative.display(),
                            line_index + 1
                        ),
                    ));
                }
                let candidates =
                    std::iter::once(relative.parent().unwrap_or(Path::new("")).join(requested))
                        .chain(dirs.iter().map(|dir| dir.join(requested)));
                let mut found = None;
                for candidate in candidates {
                    let candidate = root.join(candidate);
                    if candidate.exists() {
                        found = Some(dependency_path(root, root, &candidate)?);
                        break;
                    }
                }
                let found = found.ok_or_else(|| error("INCLUDE", format!("{}:{}: local include {name:?} not found; add its directory to include_dirs", relative.display(), line_index + 1)))?;
                let directive = visible.find('#').unwrap();
                let literal_end = visible.rfind('"').unwrap() + 1;
                // Preserve comments surrounding the directive, including a block
                // comment that starts on one source line and ends on another.
                output.push_str(&line[..directive]);
                output.push('\n');
                output.push_str(&expand(root, &found, dirs, sources, active, total)?);
                output.push_str(&format!("\n#line {} {source_name}\n", line_index + 1));
                output.push_str(&line[literal_end..]);
                output.push('\n');
            } else if matches!(
                include,
                "<metal_stdlib>" | "<metal_math>" | "<metal_texture>"
            ) {
                output.push_str(line);
                output.push('\n');
            } else {
                return Err(error("INCLUDE", format!("{}:{}: msl-source supports literal local includes and Metal SDK headers; use Slang for compiler-resolved preprocessing", relative.display(), line_index + 1)));
            }
        } else {
            output.push_str(line);
            output.push('\n');
        }
        if output.len() > super::MAX_ARTIFACT {
            return Err(error("LIMIT", "expanded Metal source exceeds 16 MiB"));
        }
    }
    active.remove(relative);
    Ok(output)
}
fn without_comments(line: &str, block: &mut bool) -> String {
    let mut chars = line.chars().peekable();
    let mut output = String::new();
    let mut quote = None;
    while let Some(c) = chars.next() {
        if *block {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                *block = false;
                output.push(' ');
            }
            output.push_str(&" ".repeat(c.len_utf8()));
        } else if let Some(q) = quote {
            output.push(c);
            if c == '\\' {
                if let Some(c) = chars.next() {
                    output.push(c);
                }
            } else if c == q {
                quote = None;
            }
        } else if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            *block = true;
            output.push_str("  ");
        } else if c == '/' && chars.peek() == Some(&'/') {
            break;
        } else {
            if c == '"' || c == '\'' {
                quote = Some(c);
            }
            output.push(c);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_metal_includes_share_one_immutable_capture() {
        let directory = super::super::publish::Temporary::new(&std::env::temp_dir()).unwrap();
        let relative = Path::new("common.inc");
        fs::write(directory.0.join(relative), "// VERSION_A\n").unwrap();
        let mut sources = Sources::new();
        let mut active = BTreeSet::new();
        let mut total = 0;
        let first = expand(
            &directory.0,
            relative,
            &[],
            &mut sources,
            &mut active,
            &mut total,
        )
        .unwrap();
        fs::write(directory.0.join(relative), "// VERSION_B\n").unwrap();
        let second = expand(
            &directory.0,
            relative,
            &[],
            &mut sources,
            &mut active,
            &mut total,
        )
        .unwrap();
        assert_eq!(first, second);
        assert!(first.contains("VERSION_A"));
        assert_eq!(sources[relative], b"// VERSION_A\n");
        assert!(unchanged(&directory.0, &sources)
            .unwrap_err()
            .contains("changed during compilation"));
    }
}
