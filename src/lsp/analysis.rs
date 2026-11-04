//! Editor analysis uses compiler snapshots and never resolves native tools.
mod hints;
mod index;
mod types;

pub use hints::{inlay_hints, signature_help};

use self::index::Index;
use super::text;
use crate::{
    check,
    lexer::Kind,
    modules::{self, provenance::SourceProgram},
    parser::ExportKind,
    source::{LexemeKind, Span},
    source_map::{AstNode, AstPath},
    syntax::{SyntaxKind, SyntaxTree},
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, ops::Range, path::PathBuf};

const KEYWORDS: &[&str] = &[
    "fn", "let", "mut", "if", "else", "while", "match", "type", "record", "not", "true", "false",
    "read", "inout", "move", "import", "as", "export", "extern", "const", "in", "then", "do",
];
const BUILTINS: &[&str] = &[
    "byte_len",
    "byte_at",
    "bytes_new",
    "bytes_len",
    "bytes_get",
    "bytes_set",
    "u8x16_load",
    "u8x16_lane",
    "u8x16_and",
    "u8x16_or",
    "u8x16_xor",
    "u8x16_eq",
    "u8x16_any",
    "u8x16_all",
    "system_box_i32_abort",
    "box_read",
    "box_write",
    "system_bytes_zeroed_abort",
    "owned_bytes_len",
    "owned_bytes_get",
    "owned_bytes_set",
    "u8",
    "u32",
    "u64",
    "f32",
    "c_null",
    "is_null",
];

fn paths(
    uri: &str,
    source: &str,
    overlays: &BTreeMap<String, String>,
) -> Option<BTreeMap<PathBuf, String>> {
    let mut result: BTreeMap<_, _> = overlays
        .iter()
        .filter_map(|(uri, source)| Some((text::uri_to_path(uri)?, source.clone())))
        .collect();
    result.insert(text::uri_to_path(uri)?, source.to_owned());
    Some(result)
}

fn program(
    uri: &str,
    source: &str,
    overlays: &BTreeMap<String, String>,
) -> Result<SourceProgram, String> {
    let Context {
        entry, overlays, ..
    } = context(uri, source, overlays)?;
    modules::compile::load_library_with_sources_and_overlays(&entry, &overlays)
        .map_err(|error| error.to_string())
}

struct Context {
    entry: PathBuf,
    overlays: BTreeMap<PathBuf, String>,
    graph: modules::ModuleGraph,
}

fn normalized_path(path: &std::path::Path) -> Option<PathBuf> {
    path.canonicalize()
        .ok()
        .or_else(|| Some(path.parent()?.canonicalize().ok()?.join(path.file_name()?)))
}

fn same_path(a: &std::path::Path, b: &std::path::Path) -> bool {
    a == b
        || normalized_path(a)
            .zip(normalized_path(b))
            .is_some_and(|(a, b)| a == b)
}

fn context(uri: &str, source: &str, opened: &BTreeMap<String, String>) -> Result<Context, String> {
    let target =
        text::uri_to_path(uri).ok_or("E-LSP-URI: semantic analysis requires a file URI")?;
    let overlays = paths(uri, source, opened).ok_or("E-LSP-URI: invalid document URI")?;
    let normalized_target = normalized_path(&target);
    let mut selected: Option<(PathBuf, modules::ModuleGraph)> = None;
    for (entry, text) in &overlays {
        if !normalized_target
            .as_ref()
            .zip(entry.parent().and_then(|parent| parent.canonicalize().ok()))
            .is_some_and(|(target, root)| target.starts_with(root))
        {
            continue;
        }
        if !crate::parser::parse_module(text).is_ok_and(|module| {
            module
                .program
                .functions
                .iter()
                .any(|function| function.name == "main")
        }) {
            continue;
        }
        if selected
            .as_ref()
            .is_some_and(|(old, _)| same_path(old, entry))
        {
            continue;
        }
        let Ok(graph) = modules::load_with_overlays(entry, &overlays) else {
            continue;
        };
        if !graph
            .modules
            .values()
            .any(|module| same_path(&module.path, &target))
        {
            continue;
        }
        if selected.is_some() {
            return Err("E-LSP-CONTEXT: more than one open main entry imports this file; close an entry to select its module context".into());
        }
        selected = Some((entry.clone(), graph));
    }
    let (entry, graph) = match selected {
        Some(selected) => selected,
        None => (
            target.clone(),
            modules::load_with_overlays(&target, &overlays).map_err(|error| error.to_string())?,
        ),
    };
    Ok(Context {
        entry,
        overlays,
        graph,
    })
}

fn diagnostic(source: &str, range: Range<usize>, code: &str, message: &str) -> Value {
    json!({"range": text::range(source, range), "severity": 1, "code": code,
        "source": "rill", "message": message})
}

pub fn diagnostics(uri: &str, source: &str, overlays: &BTreeMap<String, String>) -> Vec<Value> {
    let tree = SyntaxTree::parse(source);
    if !tree.diagnostics().is_empty() {
        return tree
            .diagnostics()
            .iter()
            .map(|error| {
                diagnostic(
                    source,
                    error.span.byte_range(),
                    error.code.as_str(),
                    &error.message,
                )
            })
            .collect();
    }
    let result = if text::uri_to_path(uri).is_none() {
        if tree
            .nodes()
            .any(|(_, node)| node.kind() == SyntaxKind::Import)
        {
            Err("E-LSP-URI: save this document to resolve local or bundled imports".into())
        } else {
            check::analyze_library(&tree.parsed_module().unwrap().program)
                .map(|_| ())
                .map_err(|error| error.to_string())
        }
    } else {
        program(uri, source, overlays).map(|_| ())
    };
    match result {
        Ok(_) => vec![],
        Err(message) => {
            let code = message.split(':').next().unwrap_or("E-CHECK");
            vec![diagnostic(source, 0..0, code, &message)]
        }
    }
}

fn name_range(tree: &SyntaxTree<'_>, span: Span, name: &str) -> Option<Range<usize>> {
    let lexemes = tree.source().lexemes();
    let start = lexemes.partition_point(|piece| piece.span().end.byte <= span.start.byte);
    let export_modifier = matches!(
        lexemes.get(start).map(|piece| piece.kind()),
        Some(LexemeKind::Token(Kind::Ident(word))) if word == "export"
    ) && matches!(
        lexemes[start + 1..]
            .iter()
            .take_while(|piece| piece.span().start.byte < span.end.byte)
            .find_map(|piece| match piece.kind() {
                LexemeKind::Token(kind) => Some(kind),
                _ => None,
            }),
        Some(Kind::Fn | Kind::Let | Kind::Type | Kind::Extern)
    );
    lexemes[start..]
        .iter()
        .skip(usize::from(export_modifier))
        .take_while(|piece| piece.span().start.byte < span.end.byte)
        .find_map(|piece| {
            let range = piece.span().byte_range();
            (range.start >= span.start.byte
                && range.end <= span.end.byte
                && matches!(piece.kind(), LexemeKind::Token(Kind::Ident(found)) if found == name))
            .then_some(range)
        })
}

fn member_range(tree: &SyntaxTree<'_>, span: Span, name: &str) -> Option<Range<usize>> {
    let lexemes = tree.source().lexemes();
    let start = lexemes.partition_point(|piece| piece.span().end.byte <= span.start.byte);
    let mut tokens = lexemes[start..]
        .iter()
        .take_while(|piece| piece.span().start.byte < span.end.byte)
        .filter(|piece| matches!(piece.kind(), LexemeKind::Token(_)));
    let mut alias = tokens.next()?;
    if matches!(alias.kind(), LexemeKind::Token(Kind::Record)) {
        alias = tokens.next()?;
    }
    if !matches!(alias.kind(), LexemeKind::Token(Kind::Ident(_)))
        || !matches!(tokens.next()?.kind(), LexemeKind::Token(Kind::Dot))
    {
        return None;
    }
    let member = tokens.next()?;
    matches!(member.kind(), LexemeKind::Token(Kind::Ident(found)) if found == name)
        .then(|| member.span().byte_range())
}

fn declaration(node: AstNode<'_>) -> Option<(&str, u32, u32)> {
    Some(match node {
        AstNode::Function(function) => (&function.name, 12, 3),
        AstNode::ExternFunction(function) => (&function.name, 12, 3),
        AstNode::Constant(constant) => (&constant.name, 14, 21),
        AstNode::TypeDeclaration(ty) => (
            &ty.name,
            if ty.record_fields.is_some() { 23 } else { 10 },
            7,
        ),
        AstNode::Variant(variant) => (&variant.name, 22, 20),
        _ => return None,
    })
}

pub fn document_symbols(source: &str) -> Vec<Value> {
    let tree = SyntaxTree::parse(source);
    let Some(parsed) = tree.parsed_module() else {
        return vec![];
    };
    let Ok(map) = tree.ast_source_map() else {
        return vec![];
    };
    tree.declarations()
        .iter()
        .filter_map(|item| {
            let path = AstPath::root(item.declaration);
            let (name, kind, _) = declaration(path.resolve(parsed)?)?;
            let span = map.origin(&path)?.span;
            Some(
                json!({"name": name, "kind": kind, "range": text::range(source, span.byte_range()),
                "selectionRange": text::range(source, name_range(&tree, span, name)?)}),
            )
        })
        .collect()
}

pub fn hover(
    uri: &str,
    source: &str,
    byte: usize,
    overlays: &BTreeMap<String, String>,
) -> Option<Value> {
    let lexical = crate::source::SourceFile::lex(source).ok()?;
    if !lexical.lexemes().iter().any(|piece| {
        piece.span().byte_range().contains(&byte) && matches!(piece.kind(), LexemeKind::Token(_))
    }) {
        return None;
    }
    let index = Index::new(uri, source, overlays);
    let (range, detail) = index
        .hovers
        .iter()
        .rev()
        .filter(|(range, _)| range.contains(&byte))
        .min_by_key(|(range, _)| range.len())?;
    Some(
        json!({"contents": {"kind": "markdown", "value": format!("```rill\n{detail}\n```")}, "range": text::range(source, range.clone())}),
    )
}

pub fn definition(
    uri: &str,
    source: &str,
    byte: usize,
    overlays: &BTreeMap<String, String>,
) -> Option<Value> {
    Index::new(uri, source, overlays)
        .references
        .into_iter()
        .filter(|(range, _)| range.contains(&byte))
        .min_by_key(|(range, _)| range.len())
        .map(|(_, target)| target)
}

fn qualified_call(source: &str, dot: usize) -> bool {
    let Ok(source) = crate::source::SourceFile::lex(source) else {
        return false;
    };
    let mut after = source.lexemes().iter().filter(|piece| {
        piece.span().start.byte > dot && matches!(piece.kind(), LexemeKind::Token(_))
    });
    matches!(
        after.next().map(|piece| piece.kind()),
        Some(LexemeKind::Token(Kind::Ident(_)))
    ) && matches!(
        after.next().map(|piece| piece.kind()),
        Some(LexemeKind::Token(Kind::LParen))
    )
}

fn module_completions(
    uri: &str,
    source: &str,
    prefix: &str,
    byte: usize,
    overlays: &BTreeMap<String, String>,
) -> Option<Vec<Value>> {
    let path = text::uri_to_path(uri)?;
    let recovered;
    let source = if SyntaxTree::parse(source).parsed_module().is_none() {
        recovered = hints::recover_caller(source, byte)?.1;
        recovered.as_str()
    } else {
        source
    };
    let Context { mut graph, .. } = context(uri, source, overlays).ok()?;
    modules::headers::apply_cached(&mut graph).ok()?;
    let module = graph
        .modules
        .values()
        .find(|module| same_path(&module.path, &path))
        .and_then(|current| current.imports.iter().find(|import| import.alias == prefix))
        .and_then(|import| graph.modules.get(&import.module))?;
    let tree = SyntaxTree::parse(&module.source);
    let parsed = tree.parsed_module()?;
    let mut items = BTreeMap::new();
    for export in &parsed.exports {
        let kind = match export.kind {
            ExportKind::Function => 3,
            ExportKind::Constant => 21,
            ExportKind::Type => 7,
        };
        let header = parsed
            .program
            .externs
            .iter()
            .find(|function| function.name == export.name)
            .and_then(|function| index::header_completion(AstNode::ExternFunction(function)))
            .or_else(|| {
                parsed
                    .program
                    .constants
                    .iter()
                    .find(|constant| constant.name == export.name)
                    .and_then(|constant| index::header_completion(AstNode::Constant(constant)))
            });
        items.insert(export.name.clone(), header.unwrap_or_else(|| json!({"label": export.name, "kind": kind, "detail": format!("{} export", module.name)})));
        if export.kind == ExportKind::Type {
            if let Some(ty) = parsed
                .program
                .types
                .iter()
                .find(|ty| ty.name == export.name)
                .filter(|ty| ty.record_fields.is_none())
            {
                for variant in &ty.variants {
                    items.insert(
                        variant.name.clone(),
                        json!({"label": variant.name, "kind": 20}),
                    );
                }
            }
        }
    }
    Some(items.into_values().collect())
}

pub fn completion(
    uri: &str,
    source: &str,
    byte: usize,
    overlays: &BTreeMap<String, String>,
) -> Vec<Value> {
    if let Ok(lexical) = crate::source::SourceFile::lex(source) {
        if lexical.lexemes().iter().any(|piece| {
            piece.span().start.byte <= byte
                && byte <= piece.span().end.byte
                && matches!(
                    piece.kind(),
                    LexemeKind::LineComment | LexemeKind::Token(Kind::Str(_) | Kind::CStr(_))
                )
        }) {
            return vec![];
        }
    }
    let mut index = Index::new(uri, source, overlays);
    if !index.checked {
        if let Some((_, scoped)) = hints::recover_caller(source, byte) {
            let mut recovered_overlays = overlays.clone();
            recovered_overlays.insert(uri.to_owned(), scoped.clone());
            index = Index::new(uri, &scoped, &recovered_overlays);
        }
    }
    let mut items = BTreeMap::new();
    if let Some((before, after)) = source
        .get(..byte)
        .and_then(|before| before.rsplit_once('.'))
    {
        if after.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            let prefix = before
                .rsplit(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '.')
                .next()
                .unwrap_or("");
            let path: Vec<_> = prefix.split('.').collect();
            let dot = before.len();
            let module = (path.len() == 1)
                .then(|| module_completions(uri, source, prefix, byte, overlays))
                .flatten();
            if qualified_call(source, dot) {
                if let Some(module) = module {
                    return module;
                }
            }
            if let Some(members) = index.members(&path, dot.saturating_sub(1)) {
                return members;
            }
            return module.unwrap_or_default();
        }
    }
    for keyword in KEYWORDS {
        items.insert((*keyword).into(), json!({"label": keyword, "kind": 14}));
    }
    for primitive in [
        "i32",
        "bool",
        "unit",
        "str",
        "cstr",
        "u8",
        "u32",
        "u64",
        "f32",
        "u8x16",
        "SystemBoxI32",
        "SystemBytes",
        "ptr",
        "const_ptr",
    ] {
        items.insert(primitive.into(), json!({"label": primitive, "kind": 7}));
    }
    for builtin in BUILTINS {
        items.insert((*builtin).into(), json!({"label": builtin, "kind": 3}));
    }
    for definition in index.globals {
        items.insert(
            definition.name.clone(),
            json!({"label": definition.name, "kind": definition.kind, "detail": definition.detail}),
        );
    }
    for completion in index.header_completions {
        if let Some(name) = completion["label"].as_str() {
            items.insert(name.to_owned(), completion);
        }
    }
    let mut locals = index.locals;
    locals.sort_by_key(|(_, _, depth)| *depth);
    for (definition, visible, _) in locals {
        if visible.contains(&byte) {
            items.insert(definition.name.clone(), json!({"label": definition.name, "kind": definition.kind, "detail": definition.detail}));
        }
    }
    items.into_values().collect()
}
