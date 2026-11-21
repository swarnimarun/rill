use super::{escape, Page, Targets};
use std::collections::BTreeMap;

pub(super) fn root_prefix(path: &str) -> String {
    "../".repeat(path.matches('/').count())
}

pub(super) fn encode(value: &str, slash: bool) -> String {
    let mut output = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_' | b'.' | b'~')
            || (slash && byte == b'/')
        {
            output.push(char::from(byte));
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}

fn decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut output = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = std::str::from_utf8(bytes.get(index + 1..index + 3)?).ok()?;
            output.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).ok()
}

fn normalize(base: &str, target: &str) -> Option<String> {
    let mut parts: Vec<_> = base.split('/').collect();
    parts.pop();
    for part in target.split('/') {
        match part {
            "" | "." => (),
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

pub(super) fn href(
    page: &Page,
    url: &str,
    pages: &[Page],
    anchors: &[Targets],
    local_aliases: &BTreeMap<String, String>,
) -> Option<String> {
    let url = url.trim();
    if url.chars().any(char::is_control) || url.contains('\\') {
        return None;
    }
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:")
    {
        return Some(url.into());
    }
    if url.starts_with('/') || url.contains(':') {
        return None;
    }
    let (path, fragment) = url
        .split_once('#')
        .map_or((url, None), |(p, f)| (p, Some(f)));
    let fragment = match fragment {
        Some(value) => Some(decode(value)?),
        None => None,
    };
    let path = decode(path)?;
    let path = path.split_once('?').map_or(path.as_str(), |(p, _)| p);
    let index = if path.is_empty() {
        pages
            .iter()
            .position(|candidate| candidate.path == page.path)?
    } else {
        let source = normalize(&page.source, path);
        let output = normalize(&page.path, path);
        pages
            .iter()
            .position(|candidate| {
                candidate.section == page.section && Some(&candidate.source) == source.as_ref()
            })
            .or_else(|| {
                pages
                    .iter()
                    .position(|candidate| Some(&candidate.path) == output.as_ref())
            })
            .or_else(|| {
                pages.iter().position(|candidate| {
                    page.section != "Language"
                        && candidate.section != "Language"
                        && Some(&candidate.source) == source.as_ref()
                })
            })?
    };
    let published = path.ends_with(".html");
    let fragment = match fragment.filter(|id| !id.is_empty()) {
        Some(id) => Some(if published {
            anchors[index].published.get(&id)?
        } else if path.is_empty() {
            local_aliases
                .get(&id)
                .or_else(|| anchors[index].source.get(&id))?
        } else {
            anchors[index].source.get(&id)?
        }),
        None => None,
    };
    let mut result = root_prefix(&page.path) + &encode(&pages[index].path, true);
    if let Some(fragment) = fragment {
        result.push('#');
        result.push_str(&encode(fragment, false));
    }
    Some(result)
}

pub(super) fn link(path: &str, label: &str, current: bool) -> String {
    format!(
        "<a href=\"{}\"{}>{}</a>",
        escape(path),
        if current {
            " aria-current=\"page\""
        } else {
            ""
        },
        escape(label)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn page(section: &str, source: &str, path: &str) -> Page {
        Page {
            path: path.into(),
            title: path.into(),
            section: section.into(),
            source: source.into(),
            markdown: String::new(),
            items: Vec::new(),
        }
    }
    #[test]
    fn resolves_nested_encoded_links_without_crossing_document_scopes() {
        let pages = vec![
            page("Language", "docs/guide.md", "language/guide.html"),
            page("Project", "docs/guide.md", "project/docs/guide.html"),
            page("Project", "README.md", "project/README.html"),
            page("API", "src/math.rill", "api/math.html"),
        ];
        let anchors = vec![
            Targets {
                source: BTreeMap::from([("hello".into(), "hello".into())]),
                published: BTreeMap::from([("hello".into(), "hello".into())])
            };
            4
        ];
        assert_eq!(
            href(
                &pages[1],
                "../README.md#hello",
                &pages,
                &anchors,
                &BTreeMap::new()
            )
            .unwrap(),
            "../../project/README.html#hello"
        );
        assert_eq!(
            href(
                &pages[0],
                "guide.md#hello",
                &pages,
                &anchors,
                &BTreeMap::new()
            )
            .unwrap(),
            "../language/guide.html#hello"
        );
        assert_eq!(
            href(
                &pages[1],
                "../src/math.rill#hello",
                &pages,
                &anchors,
                &BTreeMap::new()
            )
            .unwrap(),
            "../../api/math.html#hello"
        );
        assert!(href(
            &pages[0],
            "guide.md#missing",
            &pages,
            &anchors,
            &BTreeMap::new()
        )
        .is_none());
        for unsafe_url in [
            "javascript:alert(1)",
            "JaVaScRiPt:alert(1)",
            "data:text/html,test",
            "//example.test",
            "file:///tmp/test",
            "https:\\example.test",
            "../../outside.md",
            "%zz.md",
        ] {
            assert!(
                href(&pages[1], unsafe_url, &pages, &anchors, &BTreeMap::new()).is_none(),
                "{unsafe_url}"
            );
        }
    }
}
