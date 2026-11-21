use serde_json::{json, Value};
use std::collections::BTreeMap;

const SOURCES: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/playground_sources.rs"));

pub(super) fn catalogue() -> Result<Value, String> {
    let mut available: BTreeMap<_, _> = SOURCES.iter().copied().collect();
    let mut lessons: Vec<Value> =
        serde_json::from_str(include_str!("../../examples/playground/lessons.json"))
            .map_err(|e| e.to_string())?;
    if lessons.is_empty() {
        return Err("empty playground curriculum".into());
    }
    for lesson in &mut lessons {
        let get = |key: &str| {
            lesson[key]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| format!("lesson requires {key}"))
        };
        for key in [
            "title",
            "topic",
            "description",
            "view",
            "expected",
            "exercise",
        ] {
            get(key)?;
        }
        let id = get("id")?;
        let file = get("file")?;
        if !id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
            || file != format!("{id}.rill")
        {
            return Err(format!("lesson {id} requires a matching .rill filename"));
        }
        let source = available
            .remove(file)
            .ok_or_else(|| format!("missing or repeated lesson source {file}"))?;
        lesson["source"] = Value::String(source.into());
    }
    if let Some((extra, _)) = available.first_key_value() {
        return Err(format!("unlisted lesson source {extra}"));
    }
    Ok(json!({"lessons": lessons}))
}

pub(super) fn check() -> Result<(), String> {
    let catalogue = catalogue()?;
    for lesson in catalogue["lessons"].as_array().unwrap() {
        let mut request = json!({"source": lesson["source"], "view": lesson["view"]});
        for option in ["entry", "inputs"] {
            if let Some(v) = lesson.get(option) {
                request[option] = v.clone();
            }
        }
        let bytes = super::worker::execute(request.to_string().as_bytes())?;
        let actual: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        let ok = lesson
            .get("ok")
            .map_or(Some(true), Value::as_bool)
            .ok_or("invalid expected lesson status")?;
        let observed = if ok {
            &actual["output"]
        } else {
            &actual["diagnostics"]
        };
        let expected = lesson["expected"].as_str().unwrap();
        let compare_result =
            ok && matches!(request["view"].as_str(), Some("run" | "ir-eval" | "check"));
        let matches = observed.as_str().is_some_and(|text| {
            if compare_result {
                text.trim() == expected.trim()
            } else {
                text.contains(expected)
            }
        });
        if actual["ok"].as_bool() != Some(ok) || !matches {
            return Err(format!(
                "lesson {} disagrees with its expected result {}\n{}",
                lesson["id"], lesson["expected"], actual
            ));
        }
        println!("ok  {}", lesson["id"].as_str().unwrap());
    }
    Ok(())
}
