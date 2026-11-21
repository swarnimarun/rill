//! Hints come from checked snapshots. Token recovery identifies call context
//! only; it never assigns a type to an incomplete expression.
mod call_context;
pub(super) use call_context::recover_caller;
use call_context::{active_call, recover_callee};

use super::{index::Index, name_range, program, same_path, text, types};
use crate::{
    ast::{Expr, ParamMode},
    check::{self, CheckedProgram, Type},
    lexer::Kind,
    modules::provenance::SourceProgram,
    source::{LexemeKind, Span},
    source_map::{AstNode, AstPath, AstSourceMap, AstStep},
    syntax::{AstDeclaration, SyntaxKind, SyntaxTree},
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, ops::Range};

#[derive(Clone)]
struct Signature {
    label: String,
    parameters: Vec<String>,
    names: Vec<Option<String>>,
}

impl Signature {
    fn callable(name: &str, ty: &Type, mut names: Vec<Option<String>>) -> Option<Self> {
        let Type::Callable { params, .. } = ty else {
            return None;
        };
        let arity = if names.is_empty() {
            params.len()
        } else {
            names.len()
        };
        let rendered = callable_texts(ty, arity)?;
        names.resize(rendered.len() - 1, None);
        let parameters = rendered[..rendered.len() - 1]
            .iter()
            .zip(&names)
            .map(|(ty, name)| {
                name.as_ref()
                    .map_or_else(|| ty.clone(), |name| format!("{name}: {ty}"))
            })
            .collect();
        Some(Self::new(name, parameters, names, rendered.last()?))
    }

    fn named(name: &str, checked: &CheckedProgram<'_>) -> Option<Self> {
        if let Some(function) = checked.program.functions.iter().find(|f| f.name == name) {
            let signature = checked.functions.get(name)?;
            let rendered = types::type_texts(
                &signature
                    .params
                    .iter()
                    .chain([&signature.result])
                    .collect::<Vec<_>>(),
            );
            let parameters = function
                .params
                .iter()
                .zip(&rendered)
                .map(|(parameter, ty)| {
                    let mode = match parameter.mode {
                        ParamMode::Value => "",
                        ParamMode::Read => "read ",
                        ParamMode::Inout => "inout ",
                    };
                    format!("{mode}{}: {ty}", parameter.name)
                })
                .collect();
            return Some(Self::new(
                display_name(name),
                parameters,
                function
                    .params
                    .iter()
                    .map(|p| Some(p.name.clone()))
                    .collect(),
                rendered.last()?,
            ));
        }
        if let Some(function) = checked.program.externs.iter().find(|f| f.name == name) {
            let parameters = function
                .params
                .iter()
                .map(|p| {
                    format!(
                        "{}: {}",
                        p.name,
                        types::type_text(&check::extern_type(&p.ty))
                    )
                })
                .collect();
            let mut signature = Self::new(
                display_name(name),
                parameters,
                function
                    .params
                    .iter()
                    .map(|p| Some(p.name.clone()))
                    .collect(),
                &types::type_text(&check::extern_type(&function.result)),
            );
            signature.label.insert_str(0, "extern ");
            return Some(signature);
        }
        for ty in &checked.program.types {
            if ty.record_fields.is_some() {
                continue;
            }
            if let Some(variant) = ty.variants.iter().find(|v| v.name == name) {
                let result = if ty.params.is_empty() {
                    display_name(&ty.name).to_owned()
                } else {
                    format!("{}<{}>", display_name(&ty.name), ty.params.join(", "))
                };
                return Some(Self::new(
                    display_name(name),
                    variant.fields.iter().map(types::annotation).collect(),
                    vec![None; variant.fields.len()],
                    &result,
                ));
            }
        }
        None
    }

    fn new(name: &str, parameters: Vec<String>, names: Vec<Option<String>>, result: &str) -> Self {
        Self {
            label: format!("fn {name}({}) -> {result}", parameters.join(", ")),
            parameters,
            names,
        }
    }

    fn response(&self, active: usize) -> Value {
        let parameters: Vec<_> = self
            .parameters
            .iter()
            .map(|label| json!({"label":label}))
            .collect();
        let active = active.min(self.parameters.len().saturating_sub(1));
        json!({"signatures":[{"label":self.label,"parameters":parameters,"activeParameter":active}],
            "activeSignature":0,"activeParameter":active})
    }
}

fn callable_texts(ty: &Type, arity: usize) -> Option<Vec<String>> {
    let mut parameters = Vec::new();
    let mut result = ty;
    loop {
        let Type::Callable {
            params,
            result: next,
            ..
        } = result
        else {
            return None;
        };
        parameters.extend(params);
        result = next;
        // The canonical unary chain represents natural source parameters, but
        // returned functions remain result types. A nullary layer is always a
        // separate invocation, even when more parameters were requested.
        if params.is_empty() || parameters.len() >= arity {
            break;
        }
    }
    Some(types::type_texts(
        &parameters.into_iter().chain([result]).collect::<Vec<_>>(),
    ))
}

fn display_name(name: &str) -> &str {
    name.strip_prefix("M$")
        .and_then(|rest| rest.split_once('$'))
        .map_or(name, |(_, name)| name)
}

#[derive(Default)]
struct Details {
    hints: Vec<(usize, String, u32)>,
    calls: Vec<(usize, Signature)>,
    values: Vec<(Range<usize>, Signature)>,
    available: BTreeMap<String, Signature>,
    scopes: Vec<(String, Range<usize>, usize, usize)>,
    local_names: BTreeMap<usize, Vec<Option<String>>>,
    local_signatures: BTreeMap<usize, Signature>,
}

impl Details {
    fn new(uri: &str, source: &str, overlays: &BTreeMap<String, String>) -> Self {
        let tree = SyntaxTree::parse(source);
        let Some(parsed) = tree.parsed_module() else {
            return Self::default();
        };
        let Ok(map) = tree.ast_source_map() else {
            return Self::default();
        };
        let scopes = Index::syntax_only(uri, source)
            .locals
            .into_iter()
            .map(|(definition, visible, depth)| {
                (definition.name, visible, depth, definition.range.start)
            })
            .collect();
        let mut details = Self {
            scopes,
            ..Self::default()
        };
        if let Ok(program) = program(uri, source, overlays) {
            details.resolved(uri, source, overlays, &program);
        } else if text::uri_to_path(uri).is_none()
            && !tree
                .nodes()
                .any(|(_, node)| node.kind() == SyntaxKind::Import)
        {
            if let Ok(checked) = check::analyze_library(&parsed.program) {
                details.available(&checked, |name| Some(name.to_owned()));
                for origin in map.entries() {
                    if let Some(node) = origin.path.resolve(parsed) {
                        details.node(&tree, &map, &origin.path, origin.span, node, &checked);
                    }
                }
            }
        }
        details
    }

    fn available(&mut self, checked: &CheckedProgram<'_>, name: impl Fn(&str) -> Option<String>) {
        for raw in checked
            .program
            .functions
            .iter()
            .map(|f| f.name.as_str())
            .chain(checked.program.externs.iter().map(|f| f.name.as_str()))
            .chain(
                checked
                    .program
                    .types
                    .iter()
                    .filter(|t| t.record_fields.is_none())
                    .flat_map(|t| t.variants.iter().map(|v| v.name.as_str())),
            )
        {
            if let (Some(name), Some(signature)) = (name(raw), Signature::named(raw, checked)) {
                self.available.insert(name, signature);
            }
        }
    }

    fn resolved(
        &mut self,
        uri: &str,
        source: &str,
        overlays: &BTreeMap<String, String>,
        program: &SourceProgram,
    ) {
        let Some(path) = text::uri_to_path(uri) else {
            return;
        };
        let Ok(checked) = check::analyze_library(program.program()) else {
            return;
        };
        let tree = SyntaxTree::parse(source);
        let Ok(map) = tree.ast_source_map() else {
            return;
        };
        let Ok(context) = super::context(uri, source, overlays) else {
            return;
        };
        let Some(current) = context
            .graph
            .modules
            .values()
            .find(|m| same_path(&m.path, &path))
        else {
            return;
        };
        for symbol in program.symbols() {
            let Some(snapshot) = program.source(symbol.source) else {
                continue;
            };
            let Some(target) = symbol.target.resolve_program(program.program()) else {
                continue;
            };
            let raw_name = match target {
                AstNode::Function(f) => &f.name,
                AstNode::ExternFunction(f) => &f.name,
                AstNode::Variant(v) => &v.name,
                _ => continue,
            };
            let Some(signature) = Signature::named(raw_name, &checked) else {
                continue;
            };
            if same_path(&snapshot.path, &path) {
                self.available.insert(symbol.symbol.name.clone(), signature);
            } else if symbol.symbol.exported {
                for import in current
                    .imports
                    .iter()
                    .filter(|i| i.module == snapshot.module)
                {
                    self.available.insert(
                        format!("{}.{}", import.alias, symbol.symbol.name),
                        signature.clone(),
                    );
                }
            }
        }
        for node in program.nodes() {
            for origin in &node.origins {
                if program.source(origin.source).is_none_or(|s| {
                    !same_path(&s.path, &path) || origin.span.end.byte > s.original_len
                }) {
                    continue;
                }
                if let Some(node) = program.node(&node.path) {
                    self.node(&tree, &map, &origin.path, origin.span, node, &checked);
                }
            }
        }
    }

    fn node(
        &mut self,
        tree: &SyntaxTree<'_>,
        map: &AstSourceMap,
        path: &AstPath,
        span: Span,
        node: AstNode<'_>,
        checked: &CheckedProgram<'_>,
    ) {
        if let AstNode::Expression(expression) = node {
            let names = match expression {
                Expr::Var(name) | Expr::Call(name, _)
                    if self.scopes.iter().any(|(local, visible, _, _)| {
                        local == name && visible.contains(&span.start.byte)
                    }) =>
                {
                    vec![]
                }
                _ => expression_names(expression, checked),
            };
            if let Some(signature) = checked
                .expression_type(expression)
                .and_then(|ty| Signature::callable("", ty, names))
            {
                self.values.push((span.byte_range(), signature));
            }
        }
        match node {
            AstNode::Expression(Expr::Let(name, value, _) | Expr::LetMut(name, value, _))
                if !name.starts_with('$') =>
            {
                if let (Some(range), Some(ty)) =
                    (name_range(tree, span, name), checked.expression_type(value))
                {
                    self.hints
                        .push((range.end, format!(": {}", types::type_text(ty)), 1));
                    self.local_names
                        .insert(range.start, expression_names(value, checked));
                    if let Some(signature) =
                        Signature::callable(name, ty, expression_names(value, checked))
                    {
                        self.local_signatures.insert(range.start, signature);
                    }
                }
            }
            AstNode::Expression(expression @ Expr::Lambda(parameters, _)) => {
                let Some(ty) = checked.expression_type(expression) else {
                    return;
                };
                let Some(rendered) = callable_texts(ty, parameters.len()) else {
                    return;
                };
                for (number, parameter) in parameters.iter().enumerate() {
                    if parameter.annotation.is_some() {
                        continue;
                    }
                    if let (Some(range), Some(ty)) = (
                        map.origin(&path.child(AstStep::LambdaParameter(number)))
                            .and_then(|o| name_range(tree, o.span, &parameter.name)),
                        rendered.get(number),
                    ) {
                        self.hints.push((range.end, format!(": {ty}"), 1));
                    }
                }
                if let Some(end) = parameter_end(tree, span) {
                    self.hints
                        .push((end, format!(" -> {}", rendered.last().unwrap()), 1));
                }
            }
            AstNode::Function(function) => {
                let Some(signature) = checked.functions.get(&function.name) else {
                    return;
                };
                let rendered = types::type_texts(
                    &signature
                        .params
                        .iter()
                        .chain([&signature.result])
                        .collect::<Vec<_>>(),
                );
                for (number, (parameter, ty)) in function.params.iter().zip(&rendered).enumerate() {
                    if parameter.annotation.is_some() {
                        continue;
                    }
                    if let Some(range) = map
                        .origin(&path.child(AstStep::Parameter(number)))
                        .and_then(|o| name_range(tree, o.span, &parameter.name))
                    {
                        self.hints.push((range.end, format!(": {ty}"), 1));
                    }
                }
                if function.result_annotation.is_none() {
                    if let Some(end) = parameter_end(tree, span) {
                        self.hints
                            .push((end, format!(" -> {}", rendered.last().unwrap()), 1));
                    }
                }
            }
            AstNode::Expression(expression @ Expr::Call(name, args)) => {
                let Some(range) = callee_range(tree, span) else {
                    return;
                };
                let local = self
                    .scopes
                    .iter()
                    .filter(|(n, visible, _, _)| n == name && visible.contains(&range.start))
                    .max_by_key(|(_, _, depth, _)| depth)
                    .map(|(_, _, _, start)| *start);
                let signature = if let Some(local) = local {
                    checked.application_type(expression).and_then(|ty| {
                        Signature::callable(
                            name,
                            ty,
                            self.local_names.get(&local).cloned().unwrap_or_default(),
                        )
                    })
                } else {
                    Signature::named(name, checked)
                };
                let Some(signature) = signature else {
                    return;
                };
                self.call(tree, map, path, args, range, signature);
            }
            AstNode::Expression(Expr::Apply(callee, args)) => {
                let Some(ty) = checked.expression_type(callee) else {
                    return;
                };
                let Some(signature) =
                    Signature::callable("", ty, expression_names(callee, checked))
                else {
                    return;
                };
                // The opening parenthesis identifies arbitrary callee
                // expressions independently of their printable spelling.
                let Some(origin) = map.origin(&path.child(AstStep::ApplyCallee)) else {
                    return;
                };
                self.call(tree, map, path, args, origin.span.byte_range(), signature);
            }
            _ => {}
        }
    }

    fn call(
        &mut self,
        tree: &SyntaxTree<'_>,
        map: &AstSourceMap,
        path: &AstPath,
        args: &[Expr],
        range: Range<usize>,
        signature: Signature,
    ) {
        if let Some(open) = tree
            .source()
            .lexemes()
            .iter()
            .filter(|piece| {
                piece.span().start.byte >= range.end && matches!(piece.kind(), LexemeKind::Token(_))
            })
            .find(|piece| !matches!(piece.kind(), LexemeKind::Token(Kind::RParen)))
            .filter(|piece| matches!(piece.kind(), LexemeKind::Token(Kind::LParen)))
        {
            self.calls.push((open.span().start.byte, signature.clone()));
        }
        for (number, name) in signature.names.iter().enumerate() {
            let Some(name) = name else { continue };
            // Repeating a matching argument name adds no information.
            if matches!(args.get(number), Some(Expr::Var(argument) | Expr::Inout(argument)) if argument == name)
            {
                continue;
            }
            if let Some(origin) = map.origin(&path.child(AstStep::Argument(number))) {
                self.hints
                    .push((origin.span.start.byte, format!("{name}:"), 2));
            }
        }
    }
}

fn expression_names(expression: &Expr, checked: &CheckedProgram<'_>) -> Vec<Option<String>> {
    match expression {
        Expr::Lambda(parameters, _) => parameters.iter().map(|p| Some(p.name.clone())).collect(),
        Expr::Var(name) => checked
            .program
            .functions
            .iter()
            .find(|f| f.name == *name)
            .map(|f| f.params.iter().map(|p| Some(p.name.clone())).collect())
            .unwrap_or_default(),
        Expr::Call(name, args) => checked
            .program
            .functions
            .iter()
            .find(|f| f.name == *name)
            .map(|f| {
                f.params
                    .iter()
                    .skip(args.len())
                    .map(|p| Some(p.name.clone()))
                    .collect()
            })
            .unwrap_or_default(),
        _ => vec![],
    }
}

fn parameter_end(tree: &SyntaxTree<'_>, span: Span) -> Option<usize> {
    let mut tokens = tree
        .source()
        .lexemes()
        .iter()
        .filter(|p| p.span().start.byte >= span.start.byte && p.span().start.byte < span.end.byte)
        .filter_map(|p| match p.kind() {
            LexemeKind::Token(kind) => Some((kind, p.span())),
            _ => None,
        });
    if !tokens.by_ref().any(|(k, _)| matches!(k, Kind::Fn)) {
        return None;
    }
    let mut depth = 0usize;
    for (kind, span) in tokens {
        match kind {
            Kind::LParen => depth += 1,
            Kind::RParen if depth == 1 => return Some(span.end.byte),
            Kind::RParen => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    None
}

fn callee_range(tree: &SyntaxTree<'_>, span: Span) -> Option<Range<usize>> {
    let tokens: Vec<_> = tree
        .source()
        .lexemes()
        .iter()
        .filter(|p| {
            p.span().start.byte >= span.start.byte
                && p.span().start.byte < span.end.byte
                && matches!(p.kind(), LexemeKind::Token(_))
        })
        .collect();
    let first = tokens.first()?;
    if !matches!(first.kind(), LexemeKind::Token(Kind::Ident(_))) {
        return None;
    }
    let end = if matches!(tokens.get(1)?.kind(), LexemeKind::Token(Kind::Dot)) {
        tokens.get(2)?.span().end.byte
    } else {
        first.span().end.byte
    };
    Some(first.span().start.byte..end)
}

pub fn inlay_hints(
    uri: &str,
    source: &str,
    range: Range<usize>,
    overlays: &BTreeMap<String, String>,
) -> Vec<Value> {
    let mut hints = Details::new(uri, source, overlays).hints;
    hints.sort();
    hints.dedup();
    hints
        .into_iter()
        .filter(|(byte, _, _)| range.contains(byte))
        .map(|(byte, label, kind)| {
            json!({"position":text::position(source, byte),"label":label,"kind":kind,
            "paddingRight":kind == 2})
        })
        .collect()
}

pub fn signature_help(
    uri: &str,
    source: &str,
    byte: usize,
    overlays: &BTreeMap<String, String>,
) -> Option<Value> {
    let call = active_call(source, byte)?;
    let details = Details::new(uri, source, overlays);
    if let Some((_, signature)) = details.calls.iter().find(|(open, _)| *open == call.open) {
        return Some(signature.response(call.argument));
    }
    // Removing an unfinished caller lets the compiler check declarations and
    // imported overlays without giving fabricated arguments inferred types.
    if call.name.is_empty() {
        let scoped = recover_callee(source, call.open)?;
        let mut recovered_overlays = overlays.clone();
        recovered_overlays.insert(uri.to_owned(), scoped.clone());
        let details = Details::new(uri, &scoped, &recovered_overlays);
        // Grouping parentheses can sit outside an expression's source origin.
        // Require that nothing except those closers separates it from the
        // unfinished invocation, so an unrelated callable cannot be selected.
        return details
            .values
            .iter()
            .filter(|(range, _)| {
                range.end <= call.open
                    && source
                        .get(range.end..call.open)
                        .is_some_and(|gap| gap.chars().all(|c| c.is_whitespace() || c == ')'))
            })
            .max_by_key(|(range, _)| (range.end, range.len()))
            .map(|(_, signature)| signature.response(call.argument));
    }
    let (declarations, scoped) = recover_caller(source, call.open)?;
    if let Some((root, _)) = call.name.split_once('.') {
        // Qualified calls prefer a real import alias. A record callback has
        // the checked field type of its lexical binding, never a guessed ABI.
        if call.name.split('.').count() > 2
            || super::module_completions(uri, source, root, call.open, overlays).is_none()
        {
            let mut recovered_overlays = overlays.clone();
            recovered_overlays.insert(uri.to_owned(), scoped.clone());
            let index = Index::new(uri, &scoped, &recovered_overlays);
            let path: Vec<_> = call.name.split('.').collect();
            return index
                .member_type(&path, call.callee.start)
                .and_then(|ty| Signature::callable(&call.name, ty, vec![]))
                .map(|signature| signature.response(call.argument));
        }
    }
    if !call.name.contains('.') {
        let scope = Index::syntax_only(uri, &scoped);
        if !scope.parsed {
            return None;
        }
        if let Some((definition, _, _)) = scope
            .locals
            .iter()
            .filter(|(definition, visible, _)| {
                definition.name == call.name && visible.contains(&call.callee.start)
            })
            .max_by_key(|(_, _, depth)| depth)
        {
            let mut recovered_overlays = overlays.clone();
            recovered_overlays.insert(uri.to_owned(), scoped.clone());
            return Details::new(uri, &scoped, &recovered_overlays)
                .local_signatures
                .get(&definition.range.start)
                .map(|signature| signature.response(call.argument));
        }
    }
    let mut overlays = overlays.clone();
    if let Some(target) = text::uri_to_path(uri) {
        let context = super::context(uri, source, &overlays).ok()?;
        if !same_path(&context.entry, &target) {
            // An open entry may call the unfinished exported function. Keep
            // that root for nested imports, but exclude its call sites from
            // this temporary declaration-only check.
            let entry = context
                .graph
                .modules
                .values()
                .find(|m| same_path(&m.path, &context.entry))?;
            let entry_uri = overlays
                .keys()
                .find(|uri| text::uri_to_path(uri).is_some_and(|p| same_path(&p, &context.entry)))
                .cloned()
                .or_else(|| text::path_to_uri(&context.entry))?;
            overlays.insert(entry_uri, without_function_bodies(&entry.source)?);
        }
    }
    overlays.insert(uri.to_owned(), declarations.clone());
    Details::new(uri, &declarations, &overlays)
        .available
        .get(&call.name)
        .map(|s| s.response(call.argument))
}

fn without_function_bodies(source: &str) -> Option<String> {
    let tree = SyntaxTree::parse(source);
    let map = tree.ast_source_map().ok()?;
    let mut bodies = Vec::new();
    for declaration in tree
        .declarations()
        .iter()
        .filter(|d| matches!(d.declaration, AstDeclaration::Function(_)))
    {
        let span = map.origin(&AstPath::root(declaration.declaration))?.span;
        let tokens: Vec<_> = tree
            .source()
            .lexemes()
            .iter()
            .filter(|p| {
                p.span().start.byte >= span.start.byte && p.span().end.byte <= span.end.byte
            })
            .collect();
        let start = tokens
            .iter()
            .find(|p| matches!(p.kind(), LexemeKind::Token(Kind::LBrace)))?
            .span()
            .start
            .byte;
        let end = tokens
            .iter()
            .rfind(|p| matches!(p.kind(), LexemeKind::Token(Kind::RBrace)))?
            .span()
            .end
            .byte;
        bodies.push(start..end);
    }
    let mut result = source.to_owned();
    for body in bodies.into_iter().rev() {
        result.replace_range(body, "{}");
    }
    Some(result)
}
