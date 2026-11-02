//! User-facing names for compiler-generated symbols.

/// Module flattening uses a prefix that cannot occur in source identifiers.
/// Strip it only when the entire prefix and source constructor are valid.
pub(crate) fn constructor_name(name: &str) -> &str {
    let Some((id, source)) = name.strip_prefix("M$").and_then(|s| s.split_once('$')) else {
        return name;
    };
    let mut chars = source.chars();
    if id.is_empty()
        || !id.bytes().all(|b| b.is_ascii_digit())
        || !chars.next().is_some_and(|c| c.is_ascii_uppercase())
        || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return name;
    }
    source
}

#[cfg(test)]
mod tests {
    use super::constructor_name;

    #[test]
    fn strips_only_valid_internal_constructor_names() {
        assert_eq!(constructor_name("M$12$Some_Value"), "Some_Value");
        for name in ["Some", "M$$Some", "M$a$Some", "M$1$some", "M$1$Some$Other"] {
            assert_eq!(constructor_name(name), name);
        }
    }
}
