//! Bounded, namespace-aware help attached only after an operation is rejected.

const MAX_NAME_BYTES: usize = 64;
const MAX_CANDIDATES: usize = 4096;

pub(crate) fn unknown_name<'a>(
    message: String,
    name: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> String {
    if !(3..=MAX_NAME_BYTES).contains(&name.len()) || !source_name(name) {
        return message;
    }
    let limit = if name.len() < 5 { 1 } else { 2 };
    let mut nearest = None;
    let mut best = limit + 1;
    let mut tied = false;
    for (index, candidate) in candidates.into_iter().enumerate() {
        // Returning the original message keeps the answer independent of hash
        // iteration order even when a scope exceeds the suggestion budget.
        if index == MAX_CANDIDATES {
            return message;
        }
        if candidate.len() > MAX_NAME_BYTES || !source_name(candidate) {
            continue;
        }
        let distance = distance(name.as_bytes(), candidate.as_bytes());
        if distance == 0 {
            return message;
        }
        if distance < best {
            best = distance;
            nearest = Some(candidate);
            tied = false;
        } else if distance == best && nearest != Some(candidate) {
            tied = true;
        }
    }
    match nearest {
        Some(candidate) if best <= limit && !tied => {
            format!("{message}\nhelp: did you mean '{candidate}'?")
        }
        _ => message,
    }
}

pub(crate) fn unknown_value<'a>(
    message: String,
    name: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> String {
    let help = match name {
        "break" => Some(
            "`break` is not built-in control flow. End a `while` loop using a mutable boolean in its condition.",
        ),
        "continue" => Some(
            "`continue` is not built-in control flow. Guard the remaining loop body with `if`, keeping loop progress outside that guard.",
        ),
        "return" => Some(
            "`return` is not built-in control flow. Use the final expression of the function, with `if` or `match` branches for alternative results.",
        ),
        _ => None,
    };
    if let Some(help) = help {
        format!("{message}\nhelp: {help} See docs/rust-to-rill.md#control-flow.")
    } else {
        unknown_name(message, name, candidates)
    }
}

fn source_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

// Adjacent transpositions count as one typo. Fixed rows bound memory even for
// user-supplied names; callers admit at most MAX_NAME_BYTES ASCII bytes.
fn distance(left: &[u8], right: &[u8]) -> usize {
    let mut previous_previous = [0; MAX_NAME_BYTES + 1];
    let mut previous = [0; MAX_NAME_BYTES + 1];
    let mut current = [0; MAX_NAME_BYTES + 1];
    for (index, value) in previous.iter_mut().enumerate() {
        *value = index;
    }
    for (row, &a) in left.iter().enumerate() {
        current[0] = row + 1;
        for (column, &b) in right.iter().enumerate() {
            current[column + 1] = (current[column] + 1)
                .min(previous[column + 1] + 1)
                .min(previous[column] + usize::from(a != b));
            if row > 0 && column > 0 && a == right[column - 1] && left[row - 1] == b {
                current[column + 1] = current[column + 1].min(previous_previous[column - 1] + 1);
            }
        }
        previous_previous = previous;
        previous = current;
    }
    previous[right.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_nearest_typo_is_deterministic_and_deduplicated() {
        let expected = "unknown vaule\nhelp: did you mean 'value'?";
        for candidates in [["other", "value", "value"], ["value", "other", "value"]] {
            assert_eq!(
                unknown_name("unknown vaule".into(), "vaule", candidates),
                expected
            );
        }
        assert_eq!(distance(b"kitten", b"sitting"), 3);
        assert_eq!(distance(b"vaule", b"value"), 1);
    }

    #[test]
    fn ties_short_names_internal_names_and_large_scopes_have_no_suggestion() {
        for candidates in [["cat", "car"], ["car", "cat"]] {
            assert_eq!(unknown_name("unknown".into(), "cap", candidates), "unknown");
        }
        assert_eq!(unknown_name("unknown".into(), "xy", ["xyz"]), "unknown");
        assert_eq!(
            unknown_name("unknown".into(), "secret", ["M$secret"]),
            "unknown"
        );
        assert_eq!(
            unknown_name(
                "unknown".into(),
                "vaule",
                std::iter::repeat_n("value", MAX_CANDIDATES + 1)
            ),
            "unknown"
        );
        assert_eq!(
            unknown_name("unknown".into(), &"a".repeat(MAX_NAME_BYTES + 1), ["value"]),
            "unknown"
        );
    }
}
