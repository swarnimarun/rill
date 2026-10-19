//! `.rill-fmt` configuration.
//!
//! The file is line oriented: `key = value`, with `#` and `//` comments.
//! A project puts it at the repository root; the formatter searches the
//! formatted file's directory and then its ancestors, so every source below
//! the root shares the same settings.

use std::path::{Path, PathBuf};

/// The canonical file name.
pub const FILE_NAME: &str = ".rill-fmt";

/// Layout settings for [`crate::format_text_with`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// Longest line the formatter tries to produce.
    pub max_width: usize,
    /// Columns a tab occupies when measuring a line.
    pub tab_width: usize,
    /// An `if`/`else` chain wider than this breaks into one block per line.
    pub single_line_if_width: usize,
    /// A block whose contents are wider than this breaks over several lines.
    pub single_line_block_width: usize,
    /// Empty lines between top-level items and between trait/impl members.
    pub blank_lines_between_items: usize,
    /// Keep up to one blank line the source had between statements.
    pub preserve_blank_lines: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            max_width: 100,
            tab_width: 4,
            single_line_if_width: 80,
            single_line_block_width: 48,
            blank_lines_between_items: 1,
            preserve_blank_lines: true,
        }
    }
}

impl Config {
    /// Parses a `.rill-fmt` file. Unknown keys are errors: a misspelled setting
    /// that silently does nothing is worse than a diagnostic.
    pub fn parse(text: &str) -> Result<Config, String> {
        let mut config = Config::default();
        for (index, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(format!("line {}: expected `key = value`", index + 1));
            };
            let key = key.trim();
            let value = value.trim();
            match key {
                "max_width" => config.max_width = number(key, value, index)?,
                "tab_width" => config.tab_width = number(key, value, index)?,
                "single_line_if_width" => config.single_line_if_width = number(key, value, index)?,
                "single_line_block_width" => {
                    config.single_line_block_width = number(key, value, index)?
                }
                "blank_lines_between_items" => {
                    config.blank_lines_between_items = number(key, value, index)?
                }
                "preserve_blank_lines" => config.preserve_blank_lines = boolean(key, value, index)?,
                _ => return Err(format!("line {}: unknown setting `{key}`", index + 1)),
            }
        }
        if config.tab_width == 0 {
            return Err("tab_width must be at least 1".to_string());
        }
        Ok(config)
    }

    /// The `.rill-fmt` that governs `path`, if any.
    pub fn find_for(path: &Path) -> Option<PathBuf> {
        let start = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or_else(|| Path::new("."))
        };
        for dir in start.ancestors() {
            let candidate = dir.join(FILE_NAME);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    }

    /// Loads the configuration for `path`, falling back to the defaults when no
    /// `.rill-fmt` is found.
    pub fn load_for(path: &Path) -> Result<Config, String> {
        match Config::find_for(path) {
            Some(file) => {
                let text = std::fs::read_to_string(&file)
                    .map_err(|err| format!("{}: {err}", file.display()))?;
                Config::parse(&text).map_err(|err| format!("{}: {err}", file.display()))
            }
            None => Ok(Config::default()),
        }
    }
}

fn number(key: &str, value: &str, index: usize) -> Result<usize, String> {
    value.parse().map_err(|_| {
        format!(
            "line {}: `{key}` needs a whole number, found `{value}`",
            index + 1
        )
    })
}

fn boolean(key: &str, value: &str, index: usize) -> Result<bool, String> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!(
            "line {}: `{key}` needs `true` or `false`, found `{value}`",
            index + 1
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_generous() {
        let config = Config::default();
        assert_eq!(config.max_width, 100);
        assert_eq!(config.tab_width, 4);
        assert_eq!(config.blank_lines_between_items, 1);
    }

    #[test]
    fn parses_every_setting() {
        let config = Config::parse(
            "# comment\n\
             max_width = 88\n\
             tab_width = 2\n\
             single_line_if_width = 60\n\
             single_line_block_width = 32\n\
             blank_lines_between_items = 2\n\
             preserve_blank_lines = false\n",
        )
        .expect("valid configuration");
        assert_eq!(config.max_width, 88);
        assert_eq!(config.tab_width, 2);
        assert_eq!(config.single_line_if_width, 60);
        assert_eq!(config.single_line_block_width, 32);
        assert_eq!(config.blank_lines_between_items, 2);
        assert!(!config.preserve_blank_lines);
    }

    #[test]
    fn rejects_a_misspelled_setting() {
        let error = Config::parse("max_widht = 80\n").expect_err("typo");
        assert!(error.contains("unknown setting"), "{error}");
    }

    #[test]
    fn finds_the_nearest_file() {
        let root = std::env::temp_dir().join(format!("rill-fmt-{}", std::process::id()));
        let nested = root.join("a/b");
        std::fs::create_dir_all(&nested).expect("scratch directory");
        std::fs::write(root.join(FILE_NAME), "max_width = 72\n").expect("write config");
        let found = Config::find_for(&nested.join("main.rill")).expect("config");
        assert_eq!(found, root.join(FILE_NAME));
        let _ = std::fs::remove_dir_all(&root);
    }
}
