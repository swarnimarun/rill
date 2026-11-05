//! Documentation inputs bundled with the compiler or read from a local project.
use super::{ApiItem, Page, Site};
use rill::{
    lexer::Kind,
    parser::{ExportKind, ParsedModule},
    source::{LexemeKind, Position},
    syntax::{AstDeclaration, SyntaxTree},
};
use std::{
    collections::{BTreeSet, HashSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_MARKDOWN_FILES: usize = 256;
const MAX_MARKDOWN_BYTES: usize = 16 * 1024 * 1024;
const MAX_MARKDOWN_DEPTH: usize = 16;
const MAX_DIRECTORY_ENTRIES: usize = 8192;

// Explicit embedding keeps installed compilers independent of a source checkout.
// Historical verification reports remain linked source material, not chapters.
macro_rules! chapter {
    ($route:literal, $title:literal, $source:literal) => {
        (
            concat!("language/", $route, ".html"),
            $title,
            $source,
            include_str!(concat!("../../", $source)),
        )
    };
}

const LANGUAGE: &[(&str, &str, &str, &str)] = &[
    chapter!("index", "Rill", "README.md"),
    chapter!("syntax-tour", "Syntax tour", "docs/syntax-tour.md"),
    chapter!(
        "language-features",
        "Language features",
        "docs/language-features.md"
    ),
    chapter!("functions", "Functions", "docs/functions.md"),
    chapter!("projects", "Projects", "docs/projects.md"),
    chapter!(
        "docs-command",
        "Documentation command",
        "docs/docs-command.md"
    ),
    chapter!("stdlib", "Standard library", "stdlib/README.md"),
    chapter!("native-builds", "Native builds", "docs/native-builds.md"),
    chapter!(
        "build-performance",
        "Build scaling",
        "docs/build-performance.md"
    ),
    chapter!("hot-reload", "Reload workflows", "docs/hot-reload.md"),
    chapter!("c-interop", "C interoperability", "docs/c-interop.md"),
    chapter!("c-exports", "Scalar C exports", "docs/c-exports.md"),
    chapter!(
        "source-format",
        "Source formatting",
        "docs/source-format.md"
    ),
    chapter!("editor-support", "Editor support", "docs/editor-support.md"),
    chapter!(
        "phase-status",
        "Implementation status",
        "docs/phase-status.md"
    ),
    chapter!(
        "recursive-signatures",
        "Recursive signatures",
        "docs/recursive-signatures.md"
    ),
    chapter!(
        "deferred-projections",
        "Record projection inference",
        "docs/deferred-projections.md"
    ),
    chapter!("modules", "Module boundaries", "docs/blockers/modules.md"),
    chapter!(
        "ownership",
        "Ownership boundaries",
        "docs/blockers/ownership.md"
    ),
    chapter!(
        "owned-allocation",
        "Owned allocation",
        "docs/blockers/owned-allocation.md"
    ),
    chapter!(
        "function-resources",
        "Function resource boundaries",
        "docs/blockers/function-resources.md"
    ),
    chapter!(
        "remaining-phases",
        "Remaining language work",
        "docs/blockers/remaining-phases.md"
    ),
    chapter!("plan", "Language blueprint", "PLAN.md"),
];

pub(super) fn collect(project: Option<&crate::project::Project>) -> Result<Site, String> {
    let mut pages: Vec<_> = LANGUAGE
        .iter()
        .map(|(path, title, source, markdown)| Page {
            path: (*path).into(),
            title: (*title).into(),
            section: "Language".into(),
            source: (*source).into(),
            markdown: (*markdown).into(),
            items: Vec::new(),
        })
        .collect();
    let title = if let Some(project) = project {
        collect_markdown(project, &mut pages)?;
        collect_api(project, &mut pages)?;
        project.name.clone()
    } else {
        "Rill".into()
    };
    let mut markdown = format!("# {title} documentation\n\n");
    markdown.push_str("Rill is an experimental language. Browse the language manual, project guides and parsed source declarations.\n\n");
    for section in ["Language", "Project", "API"] {
        let entries: Vec<_> = pages
            .iter()
            .filter(|page| page.section == section)
            .collect();
        if entries.is_empty() {
            continue;
        }
        markdown.push_str(&format!("## {section}\n\n"));
        for page in entries {
            markdown.push_str(&format!(
                "- [{}]({})\n",
                escape_label(&page.title),
                page.path
            ));
        }
        markdown.push('\n');
    }
    pages.insert(
        0,
        Page {
            path: "index.html".into(),
            title: format!("{title} documentation"),
            section: "Overview".into(),
            source: "__index__.md".into(),
            markdown,
            items: Vec::new(),
        },
    );
    let total = pages.iter().try_fold(0_usize, |total, page| {
        [
            &page.path,
            &page.title,
            &page.section,
            &page.source,
            &page.markdown,
        ]
        .into_iter()
        .chain(
            page.items
                .iter()
                .flat_map(|item| [&item.kind, &item.name, &item.signature, &item.markdown]),
        )
        .try_fold(total, |total, text| total.checked_add(text.len()))
    });
    if total.is_none_or(|total| total > MAX_MARKDOWN_BYTES) {
        return Err("E-DOCS-LIMIT: documentation content exceeds 16 MiB".into());
    }
    ensure_unique_paths(&pages)?;
    Ok(Site { title, pages })
}

fn collect_markdown(
    project: &crate::project::Project,
    pages: &mut Vec<Page>,
) -> Result<(), String> {
    let mut inputs = Vec::new();
    let readme = project.root.join("README.md");
    match fs::symlink_metadata(&readme) {
        Ok(_) => markdown_input(&readme, &mut inputs)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => return Err(input_error(&readme, error)),
    }
    let docs = project.root.join("docs");
    match fs::symlink_metadata(&docs) {
        Ok(metadata) => {
            reject_symlink(&docs, &metadata)?;
            if !metadata.is_dir() {
                return Err(format!(
                    "E-DOCS-INPUT: {} must be a directory",
                    docs.display()
                ));
            }
            walk_markdown(&docs, 0, &mut inputs, &mut 0)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => return Err(input_error(&docs, error)),
    }
    inputs.sort();
    let mut total = 0;
    for path in inputs {
        let source = relative_source(&project.root, &path)?;
        let markdown = read_markdown(&path)?;
        total += markdown.len();
        if total > MAX_MARKDOWN_BYTES {
            return Err("E-DOCS-LIMIT: project Markdown exceeds 16 MiB".into());
        }
        let title = markdown_title(&markdown).unwrap_or_else(|| source.clone());
        let mut route = PathBuf::from(&source);
        route.set_extension("html");
        let path = format!("project/{}", route_path(&route)?);
        pages.push(Page {
            path,
            title,
            section: "Project".into(),
            source,
            markdown,
            items: Vec::new(),
        });
    }
    Ok(())
}

fn walk_markdown(
    directory: &Path,
    depth: usize,
    paths: &mut Vec<PathBuf>,
    work: &mut usize,
) -> Result<(), String> {
    if depth > MAX_MARKDOWN_DEPTH {
        return Err("E-DOCS-LIMIT: Markdown directory depth exceeds 16".into());
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(directory).map_err(|error| input_error(directory, error))? {
        *work += 1;
        if *work > MAX_DIRECTORY_ENTRIES {
            return Err("E-DOCS-LIMIT: Markdown tree exceeds 8192 directory entries".into());
        }
        entries.push(entry.map_err(|error| input_error(directory, error))?);
    }
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| input_error(&path, error))?;
        reject_symlink(&path, &metadata)?;
        if metadata.is_dir() {
            walk_markdown(&path, depth + 1, paths, work)?;
        } else if path.extension().is_some_and(|extension| extension == "md") {
            markdown_input(&path, paths)?;
        }
    }
    Ok(())
}

fn markdown_input(path: &Path, paths: &mut Vec<PathBuf>) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| input_error(path, error))?;
    reject_symlink(path, &metadata)?;
    if !metadata.is_file() {
        return Err(format!(
            "E-DOCS-INPUT: {} must be a regular Markdown file",
            path.display()
        ));
    }
    if paths.len() >= MAX_MARKDOWN_FILES {
        return Err("E-DOCS-LIMIT: more than 256 project Markdown files".into());
    }
    paths.push(path.into());
    Ok(())
}

fn reject_symlink(path: &Path, metadata: &fs::Metadata) -> Result<(), String> {
    if metadata.file_type().is_symlink() {
        Err(format!(
            "E-DOCS-INPUT: symlink documentation input {}",
            path.display()
        ))
    } else {
        Ok(())
    }
}

fn read_markdown(path: &Path) -> Result<String, String> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .and_then(|file| file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|error| input_error(path, error))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(format!("E-DOCS-LIMIT: {} exceeds 1 MiB", path.display()));
    }
    String::from_utf8(bytes).map_err(|_| format!("E-DOCS-INPUT: {} is not UTF-8", path.display()))
}

fn collect_api(project: &crate::project::Project, pages: &mut Vec<Page>) -> Result<(), String> {
    let entry = project.root.join(&project.entry);
    let graph = rill::modules::load(&entry).map_err(|error| format!("E-DOCS-SOURCE: {error}"))?;
    let total_source = graph.modules.values().try_fold(0_usize, |total, module| {
        total.checked_add(module.source.len())
    });
    if total_source.is_none_or(|total| total > MAX_MARKDOWN_BYTES) {
        return Err("E-DOCS-LIMIT: API source graph exceeds 16 MiB".into());
    }
    for module in graph.modules.values() {
        let entry = module.name == "@entry";
        let name = if entry { "main" } else { &module.name };
        // Import identifiers cannot contain '~', which reserves the entry's
        // route without excluding a valid source module named main.
        let route = if !entry && name.eq_ignore_ascii_case("main") {
            format!("~{name}")
        } else {
            name.replace('.', "/")
        };
        let path = format!("api/{route}.html");
        let source = if module.name.starts_with("std.") {
            format!(
                "stdlib/{}.rill",
                module.name.trim_start_matches("std.").replace('.', "/")
            )
        } else {
            // Keep the project's written source address, including a contained
            // symlink alias, rather than exposing only its canonical target.
            let lexical = if entry {
                project.root.join(&project.entry)
            } else {
                project
                    .root
                    .join(&project.entry)
                    .parent()
                    .expect("entry has a filename")
                    .join(module.name.replace('.', "/"))
                    .with_extension("rill")
            };
            relative_source(&project.root, &lexical)?
        };
        let (markdown, items) = api_content(name, &source, &module.source, entry)?;
        pages.push(Page {
            path,
            title: format!("Module {name}"),
            section: "API".into(),
            source,
            markdown,
            items,
        });
    }
    Ok(())
}

fn api_content(
    name: &str,
    path: &str,
    source: &str,
    entry: bool,
) -> Result<(String, Vec<ApiItem>), String> {
    let tree = SyntaxTree::parse(source);
    let module = tree.parsed_module().ok_or_else(|| {
        let diagnostic = tree.diagnostics().first();
        format!(
            "E-DOCS-SOURCE: {path}: {}",
            diagnostic.map_or("invalid source".into(), |diagnostic| format!(
                "{}:{}: {}",
                diagnostic.span.start.line, diagnostic.span.start.column, diagnostic.message
            ))
        )
    })?;
    let mut markdown = format!("# Module `{name}`\n\nSource: `{path}`\n\nSignatures preserve written annotations. This command does not infer types or import generated C bindings. Run `rill check` for compiler validation.\n\n");
    let module_docs = module_comments(source);
    if !module_docs.is_empty() {
        markdown.push_str(&module_docs);
        markdown.push_str("\n\n");
    }
    let mut offset = 0;
    let lines: Vec<_> = source
        .split_inclusive('\n')
        .map(|line| {
            let start = offset;
            offset += line.len();
            (start, line.trim_end_matches(['\r', '\n']))
        })
        .collect();
    let exports: HashSet<_> = module
        .exports
        .iter()
        .map(|export| (export.kind, export.name.as_str()))
        .collect();
    let mut items = Vec::new();
    for declaration in tree.declarations() {
        let (kind, symbol, export_kind) = declaration_info(module, declaration.declaration);
        if !exports.contains(&(export_kind, symbol)) && !(entry && symbol == "main") {
            continue;
        }
        let node = tree
            .node(declaration.node)
            .expect("declaration belongs to its parsed tree");
        let start = node.span().start.byte;
        let end = node.span().end.byte;
        let signature_end = if matches!(declaration.declaration, AstDeclaration::Function(_)) {
            let lexemes = tree.source().lexemes();
            let first = lexemes.partition_point(|lexeme| lexeme.span().start.byte < start);
            lexemes[first..]
                .iter()
                .take_while(|lexeme| lexeme.span().start.byte < end)
                .find_map(|lexeme| {
                    (lexeme.span().start.byte >= start
                        && lexeme.span().end.byte <= end
                        && matches!(lexeme.kind(), LexemeKind::Token(Kind::LBrace | Kind::Equal)))
                    .then_some(lexeme.span().start.byte)
                })
                .unwrap_or(end)
        } else {
            end
        };
        items.push(ApiItem {
            kind: kind.into(),
            name: symbol.into(),
            signature: source[start..signature_end].trim_end().into(),
            markdown: adjacent_comments(source, node.span().start, &lines),
        });
    }
    if items.is_empty() {
        markdown.push_str("No explicit exported declarations are present in this source module.\n");
    }
    Ok((markdown, items))
}

fn declaration_info(
    module: &ParsedModule,
    declaration: AstDeclaration,
) -> (&'static str, &str, ExportKind) {
    let (kind, name, export_kind) = match declaration {
        AstDeclaration::Function(index) => (
            "fn",
            module.program.functions[index].name.as_str(),
            ExportKind::Function,
        ),
        AstDeclaration::ExternFunction(index) => (
            "fn",
            module.program.externs[index].name.as_str(),
            ExportKind::Function,
        ),
        AstDeclaration::Constant(index) => (
            "const",
            module.program.constants[index].name.as_str(),
            ExportKind::Constant,
        ),
        AstDeclaration::Type(index) => (
            "type",
            module.program.types[index].name.as_str(),
            ExportKind::Type,
        ),
    };
    (kind, name, export_kind)
}

fn adjacent_comments(source: &str, start: Position, lines: &[(usize, &str)]) -> String {
    let line = start.line - 1;
    let line_start = lines[line].0;
    // Reverse scanning stops at preceding same-line code, rather than visiting
    // a possibly large indentation prefix once for every declaration.
    if source[line_start..start.byte]
        .chars()
        .rev()
        .any(|character| !character.is_whitespace())
    {
        return String::new();
    }
    let mut docs = Vec::new();
    for (_, text) in lines[..line].iter().rev() {
        let Some(comment) = text.trim_start().strip_prefix("///") else {
            break;
        };
        if comment.starts_with('/') {
            break;
        }
        docs.push(comment.strip_prefix(' ').unwrap_or(comment));
    }
    docs.reverse();
    docs.join("\n")
}

fn module_comments(source: &str) -> String {
    let mut lines = Vec::new();
    for line in source.lines() {
        if line.trim().is_empty() && lines.is_empty() {
            continue;
        }
        let Some(comment) = line.trim_start().strip_prefix("//!") else {
            break;
        };
        lines.push(comment.strip_prefix(' ').unwrap_or(comment));
    }
    lines.join("\n")
}

fn relative_source(root: &Path, path: &Path) -> Result<String, String> {
    relative_parts(
        path.strip_prefix(root)
            .map_err(|_| "E-DOCS-INPUT: source escapes project root".to_owned())?,
    )
    .map(|parts| parts.join("/"))
}

fn relative_parts(path: &Path) -> Result<Vec<&str>, String> {
    path.components()
        .map(|component| match component {
            std::path::Component::Normal(part) => part
                .to_str()
                .ok_or("E-DOCS-INPUT: documentation source paths must be UTF-8".into()),
            _ => Err("E-DOCS-INPUT: documentation source paths must be relative".into()),
        })
        .collect()
}

fn route_path(path: &Path) -> Result<String, String> {
    let mut result = Vec::new();
    for part in relative_parts(path)? {
        let mut encoded = String::new();
        for byte in part.bytes() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-') {
                encoded.push(byte as char);
            } else {
                encoded.push_str(&format!("~{byte:02x}"));
            }
        }
        result.push(encoded);
    }
    Ok(result.join("/"))
}

fn ensure_unique_paths(pages: &[Page]) -> Result<(), String> {
    let mut paths = BTreeSet::new();
    for page in pages {
        if !paths.insert(page.path.to_ascii_lowercase()) {
            return Err(format!(
                "E-DOCS-INPUT: duplicate documentation route {}",
                page.path
            ));
        }
    }
    Ok(())
}

fn markdown_title(markdown: &str) -> Option<String> {
    let mut fence = None;
    for line in markdown.lines() {
        let line = line.trim_start();
        if let Some(marker) = line
            .chars()
            .next()
            .filter(|marker| *marker == '`' || *marker == '~')
        {
            let count = line
                .chars()
                .take_while(|character| *character == marker)
                .count();
            if count >= 3 {
                if fence.is_some_and(|(open, length)| open == marker && count >= length) {
                    fence = None;
                } else if fence.is_none() {
                    fence = Some((marker, count));
                }
                continue;
            }
        }
        if fence.is_none() {
            if let Some(title) = line.strip_prefix("# ") {
                return Some(title.trim_end_matches('#').trim().into());
            }
        }
    }
    None
}

fn escape_label(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace('\n', " ")
}

fn input_error(path: &Path, error: std::io::Error) -> String {
    format!("E-DOCS-INPUT: {}: {error}", path.display())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Tree(PathBuf);
    impl Tree {
        fn new() -> Self {
            let directory = std::env::temp_dir().join(format!(
                "rill-docs-content-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&directory).unwrap();
            fs::create_dir(directory.join("src")).unwrap();
            fs::write(
                directory.join("rill.json"),
                r#"{"version":1,"name":"my-tool","entry":"src/main.rill"}"#,
            )
            .unwrap();
            fs::write(directory.join("src/main.rill"), "fn main() { 42 }").unwrap();
            Self(directory)
        }
        fn project(&self) -> crate::project::Project {
            crate::project::Project::load(&self.0).unwrap()
        }
    }
    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn embedded_language_pages_have_unique_routes_without_a_checkout() {
        let site = collect(None).unwrap();
        assert_eq!(site.title, "Rill");
        assert_eq!(site.pages[0].path, "index.html");
        assert!(site
            .pages
            .iter()
            .any(|page| page.source == "docs/syntax-tour.md" && page.markdown.contains("fn main")));
        assert!(site
            .pages
            .iter()
            .any(|page| page.path == "language/stdlib.html"));
        assert!(site
            .pages
            .iter()
            .all(|page| !Path::new(&page.source).is_absolute()));
        assert!(site
            .pages
            .iter()
            .all(|page| page.section != "Project" && page.section != "API"));
    }

    #[test]
    fn project_guides_and_transitive_public_source_are_deterministic() {
        let tree = Tree::new();
        fs::create_dir(tree.0.join("docs")).unwrap();
        fs::create_dir(tree.0.join("docs/nested")).unwrap();
        fs::write(tree.0.join("README.md"), "# Tool guide\n\nA project guide.").unwrap();
        fs::write(tree.0.join("docs/z.md"), "# Last guide").unwrap();
        fs::write(tree.0.join("docs/nested/first guide.md"), "# First guide\n").unwrap();
        fs::write(tree.0.join("docs/.hidden.md"), "hidden").unwrap();
        fs::write(tree.0.join("docs/data.txt"), "not Markdown").unwrap();
        fs::write(
            tree.0.join("src/main.rill"),
            "import helper; fn hidden() { 0 } fn main() { helper.double(21) }",
        )
        .unwrap();
        fs::write(tree.0.join("src/helper.rill"), "/// Doubles a value.\nexport fn double(value: i32) -> i32 { value + value }\nfn private(value) { value }\n").unwrap();
        let project = tree.project();
        let first = collect(Some(&project)).unwrap();
        let second = collect(Some(&project)).unwrap();
        assert_eq!(first.title, "my-tool");
        assert_eq!(
            first
                .pages
                .iter()
                .map(|page| (&page.path, &page.markdown))
                .collect::<Vec<_>>(),
            second
                .pages
                .iter()
                .map(|page| (&page.path, &page.markdown))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            first
                .pages
                .iter()
                .filter(|page| page.section == "Project")
                .map(|page| page.path.as_str())
                .collect::<Vec<_>>(),
            [
                "project/README.html",
                "project/docs/nested/first~20guide.html",
                "project/docs/z.html"
            ]
        );
        let helper = first
            .pages
            .iter()
            .find(|page| page.path == "api/helper.html")
            .unwrap();
        assert_eq!(helper.source, "src/helper.rill");
        assert_eq!(helper.items.len(), 1);
        assert_eq!(helper.items[0].kind, "fn");
        assert_eq!(helper.items[0].name, "double");
        assert_eq!(helper.items[0].markdown, "Doubles a value.");
        assert_eq!(
            helper.items[0].signature,
            "export fn double(value: i32) -> i32"
        );
        assert!(!helper.markdown.contains("value + value"));
        assert!(!helper.items[0].signature.contains("value + value"));
        for (first, second) in first.pages.iter().zip(&second.pages) {
            assert_eq!(
                first
                    .items
                    .iter()
                    .map(|item| (&item.kind, &item.name, &item.signature, &item.markdown))
                    .collect::<Vec<_>>(),
                second
                    .items
                    .iter()
                    .map(|item| (&item.kind, &item.name, &item.signature, &item.markdown))
                    .collect::<Vec<_>>()
            );
        }
        let entry = first
            .pages
            .iter()
            .find(|page| page.path == "api/main.html")
            .unwrap();
        assert_eq!(entry.items.len(), 1);
        assert_eq!(entry.items[0].name, "main");
    }

    #[test]
    fn api_pages_keep_source_types_constants_modes_and_adjacent_doc_comments() {
        let source = r#"//! A source module.
/// A pair with public fields.
export type Pair = { left: i32; right: bool }
export type Choice<a> = Empty | Value(a)
/// Foreign declaration, without an ABI claim.
export extern fn foreign(value: i32) -> i32;
export extern const FLAG: i32;
/// Preserve the mode and omitted annotation.
export fn update(inout value) { value <- value + 1; value }
/// This block is separated.

export fn bare(value) { value }
/// Legacy signatures remain source signatures.
export let identity value = value
"#;
        let (intro, items) = api_content("sample", "src/sample.rill", source, false).unwrap();
        assert!(intro.contains("A source module."));
        assert_eq!(
            items
                .iter()
                .map(|item| (item.kind.as_str(), item.name.as_str()))
                .collect::<Vec<_>>(),
            [
                ("type", "Pair"),
                ("type", "Choice"),
                ("fn", "foreign"),
                ("const", "FLAG"),
                ("fn", "update"),
                ("fn", "bare"),
                ("fn", "identity")
            ]
        );
        let item = |name: &str| items.iter().find(|item| item.name == name).unwrap();
        assert!(item("Pair").signature.contains("left: i32; right: bool"));
        assert!(item("Choice").signature.contains("Choice<a>"));
        assert_eq!(
            item("foreign").signature,
            "export extern fn foreign(value: i32) -> i32;"
        );
        assert_eq!(item("FLAG").signature, "export extern const FLAG: i32;");
        assert_eq!(item("update").signature, "export fn update(inout value)");
        assert_eq!(item("bare").signature, "export fn bare(value)");
        assert!(item("bare").markdown.is_empty());
        assert_eq!(item("identity").signature, "export let identity value");
        assert_eq!(
            item("identity").markdown,
            "Legacy signatures remain source signatures."
        );
        assert!(items
            .iter()
            .all(|item| !item.signature.contains("value <-")));
    }

    #[test]
    fn api_metadata_stays_separate_from_comment_headings_and_unclosed_fences() {
        let source = r#"//! ## fn second
//! ```markdown
/// ## fn second
/// ```rill
/// Deliberately unclosed comment fence.
export fn first() -> i32 { 1 }
/// The actual second declaration.
export fn second() -> i32 { 2 }
"#;
        let (intro, items) = api_content("sample", "src/sample.rill", source, false).unwrap();
        assert!(intro.ends_with("## fn second\n```markdown\n\n"));
        assert_eq!(items.len(), 2);
        assert_eq!(
            (items[0].kind.as_str(), items[0].name.as_str()),
            ("fn", "first")
        );
        assert_eq!(items[0].signature, "export fn first() -> i32");
        assert_eq!(
            items[0].markdown,
            "## fn second\n```rill\nDeliberately unclosed comment fence."
        );
        assert_eq!(
            (items[1].kind.as_str(), items[1].name.as_str()),
            ("fn", "second")
        );
        assert_eq!(items[1].signature, "export fn second() -> i32");
        assert_eq!(items[1].markdown, "The actual second declaration.");
        assert!(!intro.contains("export fn"));
        assert!(!items[0].markdown.contains("export fn second"));
    }

    #[test]
    fn native_source_is_documented_without_loading_missing_headers() {
        let tree = Tree::new();
        fs::write(tree.0.join("src/main.rill"), "@include(\"missing-for-docs.h\"); export extern fn native() -> i32; fn main() { native() }").unwrap();
        let site = collect(Some(&tree.project())).unwrap();
        let main = site
            .pages
            .iter()
            .find(|page| page.path == "api/main.html")
            .unwrap();
        assert!(main
            .items
            .iter()
            .any(|item| item.signature == "export extern fn native() -> i32;"));
        assert!(main
            .markdown
            .contains("does not infer types or import generated C bindings"));
    }

    #[test]
    fn malformed_source_and_non_utf8_or_oversized_markdown_reject() {
        let tree = Tree::new();
        fs::write(tree.0.join("src/main.rill"), "fn main( {").unwrap();
        assert!(collect(Some(&tree.project()))
            .err()
            .unwrap()
            .contains("E-DOCS-SOURCE"));
        fs::write(tree.0.join("src/main.rill"), "fn main() { 42 }").unwrap();
        let readme = tree.0.join("README.md");
        fs::write(&readme, [0xff]).unwrap();
        assert!(collect(Some(&tree.project()))
            .err()
            .unwrap()
            .contains("not UTF-8"));
        fs::write(&readme, vec![b'x'; MAX_FILE_BYTES as usize + 1]).unwrap();
        assert!(collect(Some(&tree.project()))
            .err()
            .unwrap()
            .contains("exceeds 1 MiB"));
    }

    #[test]
    fn route_encoding_and_titles_are_stable_and_collisions_reject() {
        assert_eq!(
            route_path(Path::new("docs/a b~é.html")).unwrap(),
            "docs/a~20b~7e~c3~a9.html"
        );
        #[cfg(unix)]
        assert_eq!(
            route_path(Path::new(r"docs/a\b.html")).unwrap(),
            "docs/a~5cb.html"
        );
        assert_eq!(
            markdown_title("```sh\n# Fake\n```\n# Real ##\n"),
            Some("Real".into())
        );
        let mut pages = collect(None).unwrap().pages;
        pages.push(Page {
            path: "LANGUAGE/index.html".into(),
            title: "collision".into(),
            section: "Project".into(),
            source: "other.md".into(),
            markdown: String::new(),
            items: Vec::new(),
        });
        assert!(ensure_unique_paths(&pages)
            .unwrap_err()
            .contains("duplicate documentation route"));
    }

    #[test]
    fn entry_and_imported_main_modules_have_distinct_source_routes() {
        let tree = Tree::new();
        fs::write(
            tree.0.join("rill.json"),
            r#"{"version":1,"name":"my-tool","entry":"src/app.rill"}"#,
        )
        .unwrap();
        fs::write(
            tree.0.join("src/app.rill"),
            "import main as operations; fn main() { operations.answer() }",
        )
        .unwrap();
        fs::write(tree.0.join("src/main.rill"), "export fn answer() { 42 }").unwrap();
        let site = collect(Some(&tree.project())).unwrap();
        let entry = site
            .pages
            .iter()
            .find(|page| page.path == "api/main.html")
            .unwrap();
        let imported = site
            .pages
            .iter()
            .find(|page| page.path == "api/~main.html")
            .unwrap();
        assert_eq!(entry.source, "src/app.rill");
        assert_eq!(imported.source, "src/main.rill");
        assert_eq!(entry.items[0].name, "main");
        assert_eq!(imported.items[0].name, "answer");
    }

    #[test]
    fn many_same_line_exports_keep_exact_signatures_without_body_or_comment_leaks() {
        let mut source = String::from("/// Only the first export has this note.\n");
        for index in 0..2048 {
            source.push_str(&format!(
                "export fn item{index}(value: i32) {{ value + 1 }} "
            ));
        }
        let (_, items) = api_content("large", "src/large.rill", &source, false).unwrap();
        assert_eq!(items.len(), 2048);
        assert_eq!(items[0].markdown, "Only the first export has this note.");
        assert!(items[1..].iter().all(|item| item.markdown.is_empty()));
        assert!(items
            .iter()
            .all(|item| !item.signature.contains("value + 1")));
        assert_eq!(items[2047].signature, "export fn item2047(value: i32)");
    }

    #[test]
    fn markdown_file_count_and_directory_depth_have_explicit_limits() {
        let tree = Tree::new();
        fs::create_dir(tree.0.join("docs")).unwrap();
        for index in 0..=MAX_MARKDOWN_FILES {
            fs::write(tree.0.join(format!("docs/{index:03}.md")), "small").unwrap();
        }
        assert!(collect(Some(&tree.project()))
            .err()
            .unwrap()
            .contains("more than 256"));
        fs::remove_dir_all(tree.0.join("docs")).unwrap();
        let mut directory = tree.0.join("docs");
        for _ in 0..=MAX_MARKDOWN_DEPTH + 1 {
            fs::create_dir(&directory).unwrap();
            directory.push("nested");
        }
        assert!(collect(Some(&tree.project()))
            .err()
            .unwrap()
            .contains("depth exceeds 16"));
    }

    #[cfg(unix)]
    #[test]
    fn source_api_uses_project_source_aliases_for_contained_symlinks() {
        use std::os::unix::fs::symlink;
        let tree = Tree::new();
        fs::remove_file(tree.0.join("src/main.rill")).unwrap();
        fs::write(
            tree.0.join("src/entry.txt"),
            "import helper; fn main() { helper.answer() }",
        )
        .unwrap();
        fs::write(
            tree.0.join("src/implementation.txt"),
            "export fn answer() { 42 }",
        )
        .unwrap();
        symlink("entry.txt", tree.0.join("src/main.rill")).unwrap();
        symlink("implementation.txt", tree.0.join("src/helper.rill")).unwrap();
        let site = collect(Some(&tree.project())).unwrap();
        assert_eq!(
            site.pages
                .iter()
                .find(|page| page.path == "api/main.html")
                .unwrap()
                .source,
            "src/main.rill"
        );
        assert_eq!(
            site.pages
                .iter()
                .find(|page| page.path == "api/helper.html")
                .unwrap()
                .source,
            "src/helper.rill"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_readme_docs_directories_and_nested_inputs_reject() {
        use std::os::unix::fs::symlink;
        let tree = Tree::new();
        let outside = tree.0.join("outside.md");
        fs::write(&outside, "# External content").unwrap();
        symlink(&outside, tree.0.join("README.md")).unwrap();
        assert!(collect(Some(&tree.project()))
            .err()
            .unwrap()
            .contains("symlink documentation input"));
        fs::remove_file(tree.0.join("README.md")).unwrap();
        fs::create_dir(tree.0.join("real-docs")).unwrap();
        symlink(tree.0.join("real-docs"), tree.0.join("docs")).unwrap();
        assert!(collect(Some(&tree.project()))
            .err()
            .unwrap()
            .contains("symlink documentation input"));
        fs::remove_file(tree.0.join("docs")).unwrap();
        fs::create_dir(tree.0.join("docs")).unwrap();
        symlink(outside, tree.0.join("docs/guide.md")).unwrap();
        assert!(collect(Some(&tree.project()))
            .err()
            .unwrap()
            .contains("symlink documentation input"));
    }
}
