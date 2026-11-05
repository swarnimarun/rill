use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Tree(PathBuf);
impl Tree {
    fn new() -> Self {
        loop {
            let path = std::env::temp_dir().join(format!(
                "rill-docs-cli-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(fs::canonicalize(path).unwrap()),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("cannot create docs fixture: {error}"),
            }
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rill"));
        command.current_dir(&self.0);
        command
    }

    fn project(&self, directory: &str) -> PathBuf {
        let root = self.0.join(directory);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir(root.join("docs")).unwrap();
        fs::write(
            root.join("rill.json"),
            r#"{"version":1,"name":"space-tool","entry":"src/main.rill"}"#,
        )
        .unwrap();
        fs::write(
            root.join("src/main.rill"),
            "import math; fn main() { math.double(21) }",
        )
        .unwrap();
        fs::write(
            root.join("src/math.rill"),
            "/// PhotonNotes doubles an integer.\nexport fn double(value: i32) -> i32 { value + value }\n\n/// A named position.\nexport type Point = { x: i32; y: i32 }\n\n/// PRIVATE_SENTINEL\nfn private_only(value) { value }\n",
        )
        .unwrap();
        fs::write(
            root.join("README.md"),
            "# Space tool\n\n[Read the guide](docs/guide.md#read-the-value).\n",
        )
        .unwrap();
        fs::write(
            root.join("docs/guide.md"),
            "# Guide\n\n## Read the value\n\nPhotonGuidance explains the result.\n\n[Project home](../README.md).\n\n```rill\nfn answer() { 42 }\n```\n\n| Input | Output |\n| --- | --- |\n| 21 | 42 |\n",
        )
        .unwrap();
        root
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn page(root: &Path, relative: &str) -> String {
    fs::read_to_string(root.join(relative))
        .unwrap_or_else(|error| panic!("cannot read {relative}: {error}"))
}

fn href_ending<'a>(html: &'a str, suffix: &str) -> &'a str {
    html.split("href=\"")
        .skip(1)
        .filter_map(|part| part.split('"').next())
        .find(|href| href.ends_with(suffix))
        .unwrap_or_else(|| panic!("missing link ending {suffix:?}"))
}

#[test]
fn language_docs_are_bundled_and_work_without_a_project_or_external_tools() {
    let tree = Tree::new();
    fs::write(tree.0.join("README.md"), "# UNRELATED_PRIVATE_README").unwrap();
    let output = tree
        .command()
        .args(["docs", "--language", "-o", "language site"])
        .env("PATH", "")
        .env("CC", "unavailable-docs-compiler")
        .output()
        .unwrap();
    success(&output);
    let site = tree.0.join("language site");
    for file in [
        "index.html",
        "search.html",
        "language/index.html",
        "language/syntax-tour.html",
        "assets/style.css",
        "assets/site.js",
        "assets/search-index.js",
    ] {
        assert!(site.join(file).is_file(), "missing {file}");
    }
    assert!(page(&site, "language/index.html").contains("experimental"));
    assert!(!page(&site, "assets/search-index.js").contains("UNRELATED_PRIVATE_README"));
    assert!(!site.join("project/README.html").exists());

    // With no manifest, the ordinary docs command selects the same language set.
    success(&tree.command().arg("docs").output().unwrap());
    assert!(tree
        .0
        .join("build/docs/language/syntax-tour.html")
        .is_file());
}

#[test]
fn direct_project_and_autodetected_commands_use_project_relative_outputs() {
    let tree = Tree::new();
    let root = tree.project("project with spaces");
    success(
        &tree
            .command()
            .args(["docs", "project with spaces"])
            .output()
            .unwrap(),
    );
    let default = root.join("build/docs");
    assert!(default.join("project/README.html").is_file());
    assert!(default.join("api/math.html").is_file());
    assert!(!tree.0.join("build/docs").exists());

    success(
        &tree
            .command()
            .args([
                "project",
                "docs",
                "project with spaces",
                "-o",
                "public docs",
            ])
            .output()
            .unwrap(),
    );
    assert!(root.join("public docs/api/math.html").is_file());
    assert!(root.join("public docs/language/syntax-tour.html").is_file());

    success(
        &tree
            .command()
            .current_dir(&root)
            .args(["docs", "-o", "automatic docs"])
            .output()
            .unwrap(),
    );
    assert!(root.join("automatic docs/api/main.html").is_file());
    assert!(root
        .join("automatic docs/project/docs/guide.html")
        .is_file());
}

#[test]
fn source_api_comments_symbols_markdown_links_and_search_remain_addressable() {
    let tree = Tree::new();
    let root = tree.project("app");
    success(
        &tree
            .command()
            .args(["docs", "app", "-o", "site"])
            .output()
            .unwrap(),
    );
    let site = root.join("site");
    let api = page(&site, "api/math.html");
    assert!(api.contains("id=\"fn-double\""));
    assert!(api.contains("id=\"type-point\""));
    assert!(api.contains("PhotonNotes doubles an integer."));
    assert!(!api.contains("id=\"fn-private-only\""));
    let search = page(&site, "assets/search-index.js");
    assert!(search.contains("PhotonNotes"));
    assert!(search.contains("PhotonGuidance"));
    assert!(search.contains("api/math.html#fn-double"));
    assert!(!search.contains("PRIVATE_SENTINEL"));

    let readme = page(&site, "project/README.html");
    let href = href_ending(&readme, "guide.html#read-the-value");
    let target = site
        .join("project")
        .join(href.split('#').next().unwrap())
        .canonicalize()
        .unwrap();
    assert_eq!(target, site.join("project/docs/guide.html"));
    let guide = page(&site, "project/docs/guide.html");
    assert!(guide.contains("id=\"read-the-value\""));
    assert!(guide.contains("<table"));
    assert!(guide.contains("<code"));
    let href = href_ending(&guide, "README.html");
    assert_eq!(
        site.join("project/docs").join(href).canonicalize().unwrap(),
        site.join("project/README.html")
    );
}

#[test]
fn native_source_documentation_does_not_invoke_compilers_or_dependency_tools() {
    let tree = Tree::new();
    let root = tree.project("native app");
    fs::write(
        root.join("src/main.rill"),
        "@include(\"not-installed/header.h\");\n@pkg_config(\"not-installed-docs-package\");\nexport extern fn optional(value: i32) -> i32;\nfn main() { optional(42) }\n",
    )
    .unwrap();
    let output = tree
        .command()
        .args(["docs", "native app"])
        .env("PATH", "")
        .env("CC", "unavailable-docs-compiler")
        .output()
        .unwrap();
    success(&output);
    let api = page(&root.join("build/docs"), "api/main.html");
    assert!(api.contains("optional"));
    assert!(api.contains("i32"));
}

#[test]
fn raw_html_unsafe_links_and_shell_like_names_stay_inert() {
    let tree = Tree::new();
    let root = tree.project("app $(touch PWNED)");
    fs::write(
        root.join("README.md"),
        "# Untrusted input\n\n<script>window.RILL_DOCS_PWNED = true</script>\n\n[Unsafe](javascript:alert(1))\n\n[External](https://example.com/reference)\n",
    )
    .unwrap();
    fs::write(
        root.join("docs/guide $(touch PWNED).md"),
        "# Shell-like filename\n\nUNTRUSTED_MARKDOWN_SENTINEL\n",
    )
    .unwrap();
    success(
        &tree
            .command()
            .args(["docs", "app $(touch PWNED)", "-o", "site"])
            .output()
            .unwrap(),
    );
    let site = root.join("site");
    let readme = page(&site, "project/README.html");
    assert!(readme.contains("&lt;script&gt;"));
    assert!(!readme.contains("<script>window.RILL_DOCS_PWNED"));
    assert!(!readme.to_lowercase().contains("href=\"javascript:"));
    assert!(readme.contains("href=\"https://example.com/reference\""));
    assert!(page(&site, "assets/search-index.js").contains("UNTRUSTED_MARKDOWN_SENTINEL"));
    assert!(!tree.0.join("PWNED").exists());
    assert!(!root.join("PWNED").exists());
}

#[test]
fn a_source_parse_failure_preserves_the_previous_generated_site() {
    let tree = Tree::new();
    let root = tree.project("app");
    success(
        &tree
            .command()
            .args(["docs", "app", "-o", "site"])
            .output()
            .unwrap(),
    );
    let site = root.join("site");
    let index = fs::read(site.join("index.html")).unwrap();
    let marker = fs::read(site.join(".rill-docs.json")).unwrap();
    let original_source = fs::read(root.join("src/main.rill")).unwrap();
    fs::write(root.join("src/main.rill"), "fn main() {").unwrap();
    let output = tree
        .command()
        .args(["docs", "app", "-o", "site"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(fs::read(site.join("index.html")).unwrap(), index);
    assert_eq!(fs::read(site.join(".rill-docs.json")).unwrap(), marker);

    // A small valid input must hit the label budget before multiplying a long
    // page title into every search result or replacing the previous output.
    fs::write(root.join("src/main.rill"), original_source).unwrap();
    let large_title = format!("# {}\n\n{}", "A".repeat(4096), "## x\n\n".repeat(1000));
    fs::write(root.join("docs/large-title.md"), large_title).unwrap();
    let output = tree
        .command()
        .args(["docs", "app", "-o", "site"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("E-DOCS-LIMIT"));
    assert_eq!(fs::read(site.join("index.html")).unwrap(), index);
    assert_eq!(fs::read(site.join(".rill-docs.json")).unwrap(), marker);
}

#[test]
fn regeneration_removes_stale_pages_and_preserves_unmanaged_user_files() {
    let tree = Tree::new();
    let root = tree.project("app");
    fs::write(root.join("docs/stale.md"), "# Old guide").unwrap();
    let generate = || {
        tree.command()
            .args(["docs", "app", "-o", "site"])
            .output()
            .unwrap()
    };
    success(&generate());
    let site = root.join("site");
    assert!(site.join("project/docs/stale.html").is_file());
    let marker: serde_json::Value = serde_json::from_str(&page(&site, ".rill-docs.json")).unwrap();
    assert_eq!(marker["version"], 1);
    assert!(marker["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|path| path == "index.html"));
    fs::remove_file(root.join("docs/stale.md")).unwrap();
    success(&generate());
    assert!(!site.join("project/docs/stale.html").exists());
    fs::remove_file(site.join("assets/style.css")).unwrap();
    success(&generate());
    assert!(site.join("assets/style.css").is_file());
    let index = fs::read(site.join("index.html")).unwrap();
    fs::write(site.join("user-note.txt"), "preserve user note").unwrap();
    assert!(!generate().status.success());
    assert_eq!(page(&site, "user-note.txt"), "preserve user note");
    assert_eq!(fs::read(site.join("index.html")).unwrap(), index);
}

#[test]
fn invalid_options_manifests_and_unowned_outputs_never_clobber_inputs() {
    let tree = Tree::new();
    let root = tree.project("app");
    let original = fs::read(root.join("src/main.rill")).unwrap();
    for args in [
        vec!["docs", "--unknown"],
        vec!["docs", "-o"],
        vec!["docs", "--language", "--language"],
        vec!["docs", "--language", "-o", "a", "-o", "b"],
        vec!["docs", "--language", "-o", "a", "--output", "b"],
        vec!["docs", "--language", "--output", "a", "-o", "b"],
        vec!["docs", "--language", "--open", "--open"],
        vec!["docs", "app", "--language"],
        vec!["docs", "app", "another-directory"],
        vec!["docs", "app", "-o", "."],
        vec!["docs", "app", "-o", "src"],
    ] {
        assert!(
            !tree
                .command()
                .args(&args)
                .output()
                .unwrap()
                .status
                .success(),
            "accepted {args:?}"
        );
    }
    assert_eq!(fs::read(root.join("src/main.rill")).unwrap(), original);
    fs::create_dir(tree.0.join("unowned")).unwrap();
    fs::write(tree.0.join("unowned/keep.txt"), "keep directory").unwrap();
    fs::write(tree.0.join("existing-file"), "keep file").unwrap();
    fs::create_dir(tree.0.join("empty-unowned")).unwrap();
    for output in ["unowned", "empty-unowned", "existing-file"] {
        assert!(!tree
            .command()
            .args(["docs", "--language", "-o", output])
            .output()
            .unwrap()
            .status
            .success());
    }
    assert_eq!(page(&tree.0, "unowned/keep.txt"), "keep directory");
    assert_eq!(page(&tree.0, "existing-file"), "keep file");
    fs::write(root.join("rill.json"), "{\"version\":2}").unwrap();
    assert!(!tree
        .command()
        .args(["docs", "app"])
        .output()
        .unwrap()
        .status
        .success());
}

#[cfg(unix)]
#[test]
fn output_and_managed_tree_symlinks_do_not_clobber_external_files() {
    use std::os::unix::fs::symlink;

    let tree = Tree::new();
    let root = tree.project("app");
    let outside = tree.0.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep.txt"), "preserve external file").unwrap();
    symlink(&outside, root.join("linked-output")).unwrap();
    let result = tree
        .command()
        .args(["docs", "app", "-o", "linked-output"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!outside.join("index.html").exists());

    // Parent aliases resolve to an explicit destination before publication.
    symlink(&outside, root.join("through")).unwrap();
    success(
        &tree
            .command()
            .args(["docs", "app", "-o", "through/docs"])
            .output()
            .unwrap(),
    );
    let site = outside.join("docs");
    let index = fs::read(site.join("index.html")).unwrap();
    fs::remove_file(site.join("assets/style.css")).unwrap();
    symlink(outside.join("keep.txt"), site.join("assets/style.css")).unwrap();
    let result = tree
        .command()
        .args(["docs", "app", "-o", "through/docs"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(page(&outside, "keep.txt"), "preserve external file");
    assert_eq!(fs::read(site.join("index.html")).unwrap(), index);
    assert!(fs::symlink_metadata(site.join("assets/style.css"))
        .unwrap()
        .file_type()
        .is_symlink());
}

#[cfg(unix)]
#[test]
fn open_uses_one_literal_path_and_reports_opener_failure_after_generation() {
    use std::os::unix::fs::PermissionsExt;

    let tree = Tree::new();
    let tools = tree.0.join("fake tools");
    fs::create_dir(&tools).unwrap();
    let recorded = tree.0.join("opened arguments");
    for name in ["open", "xdg-open"] {
        let program = tools.join(name);
        fs::write(
            &program,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$RILL_DOCS_OPEN_ARGS\"\n",
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let output = tree
        .command()
        .args([
            "docs",
            "--language",
            "-o",
            "opened site $(literal)",
            "--open",
        ])
        .env("PATH", &tools)
        .env("RILL_DOCS_OPEN_ARGS", &recorded)
        .output()
        .unwrap();
    success(&output);
    let site = tree.0.join("opened site $(literal)");
    let expected = format!("{}\n", site.join("index.html").display());
    assert_eq!(fs::read_to_string(&recorded).unwrap(), expected);
    assert!(site.join("language/syntax-tour.html").is_file());

    for name in ["open", "xdg-open"] {
        fs::write(tools.join(name), "#!/bin/sh\nexit 7\n").unwrap();
    }
    let output = tree
        .command()
        .args([
            "docs",
            "--language",
            "-o",
            "opened site $(literal)",
            "--open",
        ])
        .env("PATH", &tools)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("E-DOCS-OPEN"));
    assert!(site.join("index.html").is_file());
    assert!(site.join(".rill-docs.json").is_file());
}

#[test]
fn help_needs_no_valid_project_and_long_output_option_matches_short_form() {
    let tree = Tree::new();
    fs::write(tree.0.join("rill.json"), "invalid project manifest").unwrap();
    for flag in ["--help", "-h"] {
        let project_help = tree
            .command()
            .args(["project", "docs", flag])
            .output()
            .unwrap();
        success(&project_help);
        assert!(String::from_utf8(project_help.stdout)
            .unwrap()
            .contains("usage: rill docs"));
        let output = tree.command().args(["docs", flag]).output().unwrap();
        success(&output);
        let help = String::from_utf8(output.stdout).unwrap();
        assert!(help.contains("usage: rill docs"));
        assert!(help.contains("--language"));
        assert!(help.contains("--open"));
        assert!(!tree.0.join("build").exists());
    }
    success(
        &tree
            .command()
            .args(["docs", "--language", "--output", "alias site"])
            .output()
            .unwrap(),
    );
    assert!(tree
        .0
        .join("alias site/language/syntax-tour.html")
        .is_file());
}

fn search_entries(site: &Path) -> Vec<serde_json::Value> {
    let script = page(site, "assets/search-index.js");
    let json = script
        .strip_prefix("window.RILL_DOCS_INDEX=")
        .unwrap()
        .trim_end()
        .strip_suffix(';')
        .unwrap();
    serde_json::from_str(json).unwrap()
}

#[test]
fn comment_headings_and_unclosed_fences_cannot_take_another_symbol_address() {
    let tree = Tree::new();
    let root = tree.project("app");
    fs::write(
        root.join("src/main.rill"),
        "import math; fn main() { math.first() }",
    )
    .unwrap();
    fs::write(
        root.join("src/math.rill"),
        "/// ## fn second\n/// FIRST_COMMENT_SENTINEL\n/// ```rill\n/// This fence is deliberately unclosed.\nexport fn first() -> i32 { 1 }\n\n/// SECOND_ACTUAL_SENTINEL\nexport fn second() -> i32 { 2 }\n",
    )
    .unwrap();
    success(
        &tree
            .command()
            .args(["docs", "app", "-o", "site"])
            .output()
            .unwrap(),
    );
    let site = root.join("site");
    let api = page(&site, "api/math.html");
    assert_eq!(api.matches("id=\"fn-first\"").count(), 1);
    assert_eq!(api.matches("id=\"fn-second\"").count(), 1);
    assert!(api.contains("export fn second()"));
    let entries = search_entries(&site);
    let second: Vec<_> = entries
        .iter()
        .filter(|entry| entry["url"] == "api/math.html#fn-second")
        .collect();
    assert!(!second.is_empty(), "second() lost its declaration address");
    assert!(second.iter().any(|entry| entry["text"]
        .as_str()
        .unwrap()
        .contains("export fn second()")));
    for entry in second {
        let text = entry["text"].as_str().unwrap();
        assert!(!text.contains("export fn first()"));
        assert!(!text.contains("FIRST_COMMENT_SENTINEL"));
    }
    assert!(entries.iter().any(|entry| {
        entry["text"]
            .as_str()
            .unwrap()
            .contains("SECOND_ACTUAL_SENTINEL")
            && entry["url"]
                .as_str()
                .unwrap()
                .starts_with("api/math.html#fn-second")
    }));
}

#[test]
fn empty_markdown_headings_have_labeled_addressable_search_results() {
    let tree = Tree::new();
    let root = tree.project("app");
    fs::write(
        root.join("docs/blank.md"),
        "# Blank guide\n\n##\n\nEMPTY_HEADING_SENTINEL\n",
    )
    .unwrap();
    success(
        &tree
            .command()
            .args(["docs", "app", "-o", "site"])
            .output()
            .unwrap(),
    );
    let site = root.join("site");
    let entries = search_entries(&site);
    let matching: Vec<_> = entries
        .iter()
        .filter(|entry| {
            entry["text"]
                .as_str()
                .unwrap()
                .contains("EMPTY_HEADING_SENTINEL")
        })
        .collect();
    assert!(!matching.is_empty());
    let html = page(&site, "project/docs/blank.html");
    for entry in matching {
        assert!(!entry["title"].as_str().unwrap().trim().is_empty());
        let url = entry["url"].as_str().unwrap();
        assert!(url.starts_with("project/docs/blank.html#"));
        let fragment = url.split_once('#').unwrap().1;
        assert!(html.contains(&format!("id=\"{fragment}\"")));
    }
}
