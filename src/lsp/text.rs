//! LSP uses zero-based UTF-16 positions, independently of compiler columns.
use serde_json::{json, Value};
use std::ops::Range;
use std::path::{Path, PathBuf};

fn line(text: &str, wanted: usize) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let (mut start, mut row, mut cursor) = (0, 0, 0);
    while cursor < bytes.len() {
        if matches!(bytes[cursor], b'\r' | b'\n') {
            if row == wanted {
                return Some((start, cursor));
            }
            cursor += if bytes[cursor] == b'\r' && bytes.get(cursor + 1) == Some(&b'\n') {
                2
            } else {
                1
            };
            start = cursor;
            row += 1;
        } else {
            cursor += 1;
        }
    }
    (row == wanted).then_some((start, text.len()))
}

pub fn offset(text: &str, position: &Value) -> Option<usize> {
    let row = usize::try_from(position.get("line")?.as_u64()?).ok()?;
    let wanted = usize::try_from(position.get("character")?.as_u64()?).ok()?;
    let (start, end) = line(text, row)?;
    let mut units = 0;
    for (index, character) in text[start..end].char_indices() {
        if units == wanted {
            return Some(start + index);
        }
        units += character.len_utf16();
        if units > wanted {
            return None;
        }
    }
    (wanted >= units).then_some(end)
}

pub fn position(text: &str, mut byte: usize) -> Value {
    byte = byte.min(text.len());
    while !text.is_char_boundary(byte) {
        byte -= 1;
    }
    let bytes = text.as_bytes();
    let (mut start, mut row, mut cursor) = (0, 0, 0);
    while cursor < byte {
        if matches!(bytes[cursor], b'\r' | b'\n') {
            let next = cursor
                + if bytes[cursor] == b'\r' && bytes.get(cursor + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
            if next > byte {
                byte = cursor;
                break;
            }
            cursor = next;
            start = cursor;
            row += 1;
        } else {
            cursor += 1;
        }
    }
    json!({"line": row, "character": text[start..byte].encode_utf16().count()})
}

pub fn range(text: &str, bytes: Range<usize>) -> Value {
    json!({"start": position(text, bytes.start), "end": position(text, bytes.end)})
}

pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let path = if rest.starts_with('/') {
        rest
    } else {
        rest.strip_prefix("localhost")
            .filter(|path| path.starts_with('/'))?
    };
    if path.contains(['?', '#']) {
        return None;
    }
    let mut decoded = Vec::with_capacity(path.len());
    let mut bytes = path.bytes();
    while let Some(byte) = bytes.next() {
        decoded.push(if byte == b'%' {
            let high = (bytes.next()? as char).to_digit(16)?;
            let low = (bytes.next()? as char).to_digit(16)?;
            (high * 16 + low) as u8
        } else {
            byte
        });
    }
    if decoded.contains(&0) {
        return None;
    }
    #[cfg(unix)]
    let path = {
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(std::ffi::OsString::from_vec(decoded))
    };
    #[cfg(not(unix))]
    let path = {
        let path = String::from_utf8(decoded).ok()?;
        let path = if path.as_bytes().get(2) == Some(&b':') {
            path.strip_prefix('/')?
        } else {
            &path
        };
        PathBuf::from(path)
    };
    path.is_absolute().then_some(path)
}

pub fn path_to_uri(path: &Path) -> Option<String> {
    if !path.is_absolute() {
        return None;
    }
    #[cfg(unix)]
    let bytes = {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes()
    };
    #[cfg(not(unix))]
    let portable = format!("/{}", path.to_str()?.replace('\\', "/"));
    #[cfg(not(unix))]
    let bytes = portable.as_bytes();
    use std::fmt::Write;
    let mut uri = String::from("file://");
    for byte in bytes {
        if byte.is_ascii_alphanumeric() || b"/-._~:".contains(byte) {
            uri.push(*byte as char);
        } else {
            write!(uri, "%{byte:02X}").unwrap();
        }
    }
    Some(uri)
}

pub(super) fn apply_changes(text: &str, changes: &Value) -> Result<String, &'static str> {
    let changes = changes
        .as_array()
        .ok_or("contentChanges must be an array")?;
    if changes.len() > 256 {
        return Err("too many content changes");
    }
    let mut result = text.to_owned();
    for change in changes {
        let replacement = change["text"]
            .as_str()
            .ok_or("change text must be a string")?;
        if let Some(range) = change.get("range") {
            let start = offset(&result, &range["start"]).ok_or("invalid range start")?;
            let end = offset(&result, &range["end"]).ok_or("invalid range end")?;
            if start > end {
                return Err("reversed edit range");
            }
            if result.len() - (end - start) + replacement.len() > super::MAX_DOCUMENT {
                return Err("document exceeds 1 MiB");
            }
            result.replace_range(start..end, replacement);
        } else {
            if replacement.len() > super::MAX_DOCUMENT {
                return Err("document exceeds 1 MiB");
            }
            result = replacement.to_owned();
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_positions_round_trip_all_line_endings() {
        let text = "é🦀x\r\nnext\rfinal\n";
        for (byte, row, column) in [
            (0, 0, 0),
            (2, 0, 1),
            (6, 0, 3),
            (9, 1, 0),
            (14, 2, 0),
            (20, 3, 0),
        ] {
            let pos = json!({"line": row, "character": column});
            assert_eq!(offset(text, &pos), Some(byte));
            assert_eq!(position(text, byte), pos);
        }
        assert_eq!(offset(text, &json!({"line":0,"character":2})), None);
        assert_eq!(offset(text, &json!({"line":0,"character":100})), Some(7));
        assert_eq!(offset(text, &json!({"line":4,"character":0})), None);
        assert_eq!(position(text, 8), json!({"line":0,"character":4}));
    }

    #[test]
    fn file_uri_round_trip_preserves_spaces_percent_and_unicode() {
        let path = std::env::temp_dir().join("rill a%#?é🦀.rill");
        let uri = path_to_uri(&path).unwrap();
        assert_eq!(uri_to_path(&uri), Some(path));
        assert_eq!(
            uri_to_path("file://localhost/tmp/a.rill"),
            Some(PathBuf::from("/tmp/a.rill"))
        );
        for invalid in [
            "file://remote/tmp/a",
            "file:///tmp/%QQ",
            "file:///tmp/%00",
            "file:///tmp/a#fragment",
            "https://example.com/a",
        ] {
            assert_eq!(uri_to_path(invalid), None, "{invalid}");
        }
    }
}
