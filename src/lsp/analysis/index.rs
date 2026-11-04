use super::{
    declaration, member_range, name_range, program, same_path, text,
    types::{annotation, function_detail, type_text, type_texts},
};
use crate::{
    ast::{Expr, Pattern},
    check::{self, CheckedProgram, Type},
    lexer::Kind,
    modules::provenance::{Lowering, SourceProgram},
    source::LexemeKind,
    source_map::{AstNode, AstPath, AstSourceMap, AstStep, OriginKind},
    syntax::{AstDeclaration, NodeId, SyntaxElement, SyntaxKind, SyntaxTree},
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    ops::Range,
};

#[derive(Clone)]
pub(super) struct Definition {
    pub(super) name: String,
    pub(super) range: Range<usize>,
    pub(super) uri: String,
    pub(super) detail: String,
    pub(super) kind: u32,
}

impl Definition {
    fn location(&self, source: &str) -> Value {
        json!({"uri": self.uri, "range": text::range(source, self.range.clone())})
    }
}

struct Syntax<'a, 's> {
    tree: &'a SyntaxTree<'s>,
    map: &'a AstSourceMap,
}

pub(super) struct Index {
    pub(super) parsed: bool,
    pub(super) checked: bool,
    uri: String,
    block_ends: HashMap<NodeId, usize>,
    pub(super) references: Vec<(Range<usize>, Value)>,
    pub(super) hovers: Vec<(Range<usize>, String)>,
    pub(super) locals: Vec<(Definition, Range<usize>, usize)>,
    pub(super) globals: Vec<Definition>,
    pub(super) header_completions: Vec<Value>,
    local_types: BTreeMap<usize, Type>,
    record_fields: HashMap<Type, Vec<(String, Type)>>,
}

impl Index {
    pub(super) fn new(uri: &str, source: &str, overlays: &BTreeMap<String, String>) -> Self {
        Self::build(uri, source, overlays, true)
    }

    pub(super) fn syntax_only(uri: &str, source: &str) -> Self {
        Self::build(uri, source, &BTreeMap::new(), false)
    }

    fn build(uri: &str, source: &str, overlays: &BTreeMap<String, String>, semantic: bool) -> Self {
        let mut index = Self {
            parsed: false,
            checked: false,
            uri: uri.into(),
            block_ends: HashMap::new(),
            references: vec![],
            hovers: vec![],
            locals: vec![],
            globals: vec![],
            header_completions: vec![],
            local_types: BTreeMap::new(),
            record_fields: HashMap::new(),
        };
        let tree = SyntaxTree::parse(source);
        let Some(parsed) = tree.parsed_module() else {
            return index;
        };
        let Ok(map) = tree.ast_source_map() else {
            return index;
        };
        index.parsed = true;
        let mut work = vec![(tree.root(), source.len())];
        while let Some((id, mut end)) = work.pop() {
            let Some(node) = tree.node(id) else { continue };
            if node.kind() == SyntaxKind::Block {
                end = node.span().end.byte;
            }
            index.block_ends.insert(id, end);
            work.extend(node.children().iter().filter_map(|child| match child {
                SyntaxElement::Node(child) => Some((*child, end)),
                _ => None,
            }));
        }
        for item in tree.declarations() {
            let path = AstPath::root(item.declaration);
            let Some((name, _, kind)) = path.resolve(parsed).and_then(declaration) else {
                continue;
            };
            let Some(span) = map.origin(&path).map(|origin| origin.span) else {
                continue;
            };
            let Some(range) = name_range(&tree, span, name) else {
                continue;
            };
            let definition = Definition {
                name: name.into(),
                range: range.clone(),
                uri: uri.into(),
                detail: name.into(),
                kind,
            };
            index.references.push((range, definition.location(source)));
            index.globals.push(definition);
        }
        for (number, ty) in parsed
            .program
            .types
            .iter()
            .enumerate()
            .filter(|(_, ty)| ty.record_fields.is_none())
        {
            for (variant, value) in ty.variants.iter().enumerate() {
                let path =
                    AstPath::root(AstDeclaration::Type(number)).child(AstStep::Variant(variant));
                let Some(origin) = map.origin(&path) else {
                    continue;
                };
                let Some(range) = name_range(&tree, origin.span, &value.name) else {
                    continue;
                };
                index.globals.push(Definition {
                    name: value.name.clone(),
                    range,
                    uri: uri.into(),
                    detail: format!("{} constructor", ty.name),
                    kind: 20,
                });
            }
        }
        for (number, function) in parsed.program.functions.iter().enumerate() {
            let root = AstPath::root(AstDeclaration::Function(number));
            let Some(function_span) = map.origin(&root).map(|origin| origin.span) else {
                continue;
            };
            let mut scope = Vec::new();
            for (number, parameter) in function.params.iter().enumerate() {
                let Some(span) = map
                    .origin(&root.child(AstStep::Parameter(number)))
                    .map(|origin| origin.span)
                else {
                    continue;
                };
                let Some(range) = name_range(&tree, span, &parameter.name) else {
                    continue;
                };
                let detail = parameter.annotation.as_ref().map_or_else(
                    || parameter.name.clone(),
                    |ty| format!("{}: {}", parameter.name, annotation(ty)),
                );
                let definition = Definition {
                    name: parameter.name.clone(),
                    range: range.clone(),
                    uri: uri.into(),
                    detail,
                    kind: 6,
                };
                index.references.push((range, definition.location(source)));
                index
                    .locals
                    .push((definition.clone(), function_span.byte_range(), 0));
                scope.push(definition);
            }
            index.walk(
                &Syntax {
                    tree: &tree,
                    map: &map,
                },
                &root.child(AstStep::FunctionBody),
                &function.body,
                &scope,
                function_span.end.byte,
                1,
            );
        }
        if !semantic {
            return index;
        }
        if let Ok(program) = program(uri, source, overlays) {
            index.resolved(uri, overlays, &program);
        } else if !tree
            .nodes()
            .any(|(_, node)| node.kind() == SyntaxKind::Import)
        {
            if let Ok(checked) = check::analyze_library(&parsed.program) {
                index.checked = true;
                for origin in map.entries() {
                    if let Some(node) = origin.path.resolve(parsed) {
                        index.checked_node(&tree, &map, &origin.path, origin.span, node, &checked);
                    }
                    if let Some(AstNode::Expression(expression)) = origin.path.resolve(parsed) {
                        if origin.provenance == OriginKind::Source {
                            if let Some(ty) = checked.expression_type(expression) {
                                index.hovers.push((origin.span.byte_range(), type_text(ty)));
                            }
                        }
                        if let Expr::Call(name, _) | Expr::Var(name) = expression {
                            if let Some(definition) = index
                                .globals
                                .iter()
                                .find(|definition| definition.name == *name && definition.kind == 3)
                            {
                                if let Some(range) = name_range(&tree, origin.span, name) {
                                    if !index.references.iter().any(|(found, _)| *found == range) {
                                        index
                                            .references
                                            .push((range.clone(), definition.location(source)));
                                    }
                                    if let Some(function) = parsed
                                        .program
                                        .functions
                                        .iter()
                                        .find(|function| function.name == *name)
                                    {
                                        if let Some(signature) = checked.functions.get(name) {
                                            index.hovers.push((
                                                range,
                                                function_detail(name, function, signature),
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        // A call origin spans its arguments, so its result type is not the
        // type of the local callee's name. Attach checked binding details to
        // the exact lexical reference range.
        for (range, target) in &index.references {
            if target["uri"] != uri {
                continue;
            }
            if let Some((definition, _, _)) = index.locals.iter().find(|(definition, _, _)| {
                target["range"] == text::range(source, definition.range.clone())
                    && index.local_types.contains_key(&definition.range.start)
            }) {
                index
                    .hovers
                    .push((range.clone(), definition.detail.clone()));
            }
        }
        index
    }

    pub(super) fn member_type(&self, path: &[&str], byte: usize) -> Option<&Type> {
        let (root, path) = path.split_first()?;
        let (definition, _, _) = self
            .locals
            .iter()
            .filter(|(definition, visible, _)| definition.name == *root && visible.contains(&byte))
            .max_by_key(|(_, _, depth)| depth)?;
        let mut ty = self.local_types.get(&definition.range.start)?;
        for field in path {
            ty = &self
                .record_fields
                .get(ty)?
                .iter()
                .find(|(name, _)| name == field)?
                .1;
        }
        Some(ty)
    }

    pub(super) fn members(&self, path: &[&str], byte: usize) -> Option<Vec<Value>> {
        let root = path.first()?;
        // An untyped or nonrecord local still shadows an import alias.
        if !self
            .locals
            .iter()
            .any(|(definition, visible, _)| definition.name == *root && visible.contains(&byte))
        {
            return None;
        }
        let fields = self
            .member_type(path, byte)
            .and_then(|ty| self.record_fields.get(ty));
        Some(
            fields
                .map(|fields| {
                    fields.iter().map(|(name, ty)| {
            json!({"label":name,"kind":5,"detail":format!("{name}: {}", type_text(ty))})
        }).collect()
                })
                .unwrap_or_default(),
        )
    }

    fn typed_binding(
        &mut self,
        range: Range<usize>,
        detail: String,
        ty: &Type,
        checked: &CheckedProgram<'_>,
    ) {
        self.hovers.push((range.clone(), detail.clone()));
        if let Some((definition, _, _)) = self
            .locals
            .iter_mut()
            .find(|(definition, _, _)| definition.range == range)
        {
            definition.detail = detail;
            self.local_types.insert(range.start, ty.clone());
        }
        let mut pending = vec![ty.clone()];
        // Metadata is optional, bounded, and comes from the compiler's exact
        // nominal instantiation, including generic field substitution.
        for _ in 0..128 {
            let Some(ty) = pending.pop() else { break };
            if self.record_fields.contains_key(&ty) || !checked.is_record_type(&ty) {
                continue;
            }
            let Type::Nominal(name, _) = &ty else {
                continue;
            };
            let Some(names) = checked
                .program
                .types
                .iter()
                .find(|decl| decl.name == *name)
                .and_then(|decl| decl.record_fields.as_ref())
            else {
                continue;
            };
            let Ok(variants) = checked.adt_variants(&ty) else {
                continue;
            };
            let Some(variant) = variants.first() else {
                continue;
            };
            let fields: Vec<_> = names
                .iter()
                .cloned()
                .zip(variant.fields.iter().cloned())
                .collect();
            pending.extend(fields.iter().map(|(_, ty)| ty.clone()));
            self.record_fields.insert(ty, fields);
        }
    }

    fn checked_node(
        &mut self,
        tree: &SyntaxTree<'_>,
        map: &AstSourceMap,
        path: &AstPath,
        span: crate::source::Span,
        node: AstNode<'_>,
        checked: &CheckedProgram<'_>,
    ) {
        match node {
            AstNode::Expression(Expr::Let(name, value, _) | Expr::LetMut(name, value, _))
                if !name.starts_with('$') =>
            {
                if let (Some(range), Some(ty)) =
                    (name_range(tree, span, name), checked.expression_type(value))
                {
                    self.typed_binding(
                        range,
                        format!("let {name}: {}", type_text(ty)),
                        ty,
                        checked,
                    );
                }
            }
            AstNode::Function(function) => {
                let Some(signature) = checked.functions.get(&function.name) else {
                    return;
                };
                let name = tree
                    .parsed_module()
                    .and_then(|parsed| path.resolve(parsed))
                    .and_then(|node| match node {
                        AstNode::Function(source) => Some(source.name.as_str()),
                        _ => None,
                    })
                    .unwrap_or(&function.name);
                if let Some(range) = name_range(tree, span, name) {
                    let detail = function_detail(name, function, signature);
                    if let Some(definition) = self
                        .globals
                        .iter_mut()
                        .find(|definition| definition.range == range)
                    {
                        definition.detail = detail.clone();
                    }
                    self.hovers.push((range, detail));
                }
                let rendered = type_texts(
                    &signature
                        .params
                        .iter()
                        .chain([&signature.result])
                        .collect::<Vec<_>>(),
                );
                for (number, ((parameter, ty), rendered)) in function
                    .params
                    .iter()
                    .zip(&signature.params)
                    .zip(rendered)
                    .enumerate()
                {
                    if let Some(range) = map
                        .origin(&path.child(AstStep::Parameter(number)))
                        .and_then(|origin| name_range(tree, origin.span, &parameter.name))
                    {
                        self.typed_binding(
                            range,
                            format!("{}: {rendered}", parameter.name),
                            ty,
                            checked,
                        );
                    }
                }
            }
            AstNode::Expression(expression @ Expr::Lambda(parameters, _)) => {
                let mut callable = checked.expression_type(expression);
                let mut parameter_types = Vec::new();
                for _ in parameters {
                    let Some(Type::Callable { params, result, .. }) = callable else {
                        break;
                    };
                    let Some(ty) = params.first() else { break };
                    parameter_types.push(ty);
                    callable = Some(result);
                }
                let rendered = type_texts(&parameter_types);
                for (number, ((parameter, ty), rendered)) in parameters
                    .iter()
                    .zip(parameter_types)
                    .zip(rendered)
                    .enumerate()
                {
                    if let Some(range) = map
                        .origin(&path.child(AstStep::LambdaParameter(number)))
                        .and_then(|origin| name_range(tree, origin.span, &parameter.name))
                    {
                        self.typed_binding(
                            range,
                            format!("{}: {rendered}", parameter.name),
                            ty,
                            checked,
                        );
                    }
                }
            }
            AstNode::Expression(expression @ Expr::Record(_, fields)) => {
                // A polymorphic record initializer still has checked field
                // types even when concrete-layout metadata is unavailable.
                if let Some(ty) = checked.expression_type(expression) {
                    let fields: Option<Vec<_>> = fields
                        .iter()
                        .map(|(name, value)| {
                            Some((name.clone(), checked.expression_type(value)?.clone()))
                        })
                        .collect();
                    if let Some(fields) = fields {
                        self.record_fields.insert(ty.clone(), fields);
                    }
                }
            }
            _ => {}
        }
    }

    fn walk(
        &mut self,
        syntax: &Syntax<'_, '_>,
        path: &AstPath,
        expression: &Expr,
        scope: &[Definition],
        scope_end: usize,
        depth: usize,
    ) {
        let (tree, map) = (syntax.tree, syntax.map);
        let Some(origin) = map.origin(path) else {
            return;
        };
        match expression {
            Expr::Var(name)
            | Expr::Call(name, _)
            | Expr::Move(name)
            | Expr::Inout(name)
            | Expr::Assign(name, _)
            | Expr::Field(name, _)
            | Expr::InoutField(name, _)
            | Expr::FieldAssign(name, _, _) => {
                if let Some(definition) = scope
                    .iter()
                    .rev()
                    .find(|definition| definition.name == *name)
                {
                    if let Some(range) = name_range(tree, origin.span, name) {
                        self.references
                            .push((range.clone(), definition.location(tree.source().text())));
                        self.hovers.push((range, definition.detail.clone()));
                    }
                }
            }
            _ => {}
        }
        match expression {
            Expr::Lambda(parameters, body) => {
                let mut scope = scope.to_vec();
                let body_path = path.child(AstStep::LambdaBody);
                let body_extent = extent(map, &body_path, body);
                let body_start = tree
                    .source()
                    .lexemes()
                    .iter()
                    .find(|piece| {
                        piece.span().start.byte >= origin.span.start.byte
                            && piece.span().start.byte < body_extent.start
                            && matches!(piece.kind(), LexemeKind::Token(Kind::LBrace))
                    })
                    .map_or(body_extent.start, |piece| piece.span().end.byte);
                let visible = body_start..origin.span.end.byte;
                for (number, parameter) in parameters.iter().enumerate() {
                    let Some(span) = map
                        .origin(&path.child(AstStep::LambdaParameter(number)))
                        .map(|origin| origin.span)
                    else {
                        continue;
                    };
                    let Some(range) = name_range(tree, span, &parameter.name) else {
                        continue;
                    };
                    let detail = parameter.annotation.as_ref().map_or_else(
                        || parameter.name.clone(),
                        |ty| format!("{}: {}", parameter.name, annotation(ty)),
                    );
                    let definition = Definition {
                        name: parameter.name.clone(),
                        range: range.clone(),
                        uri: self.uri.clone(),
                        detail,
                        kind: 6,
                    };
                    self.references
                        .push((range, definition.location(tree.source().text())));
                    self.locals
                        .push((definition.clone(), visible.clone(), depth + 1));
                    scope.push(definition);
                }
                self.walk(
                    syntax,
                    &body_path,
                    body,
                    &scope,
                    origin.span.end.byte.min(scope_end),
                    depth + 1,
                );
            }
            Expr::Let(name, value, body) | Expr::LetMut(name, value, body) => {
                self.walk(
                    syntax,
                    &path.child(AstStep::LetValue),
                    value,
                    scope,
                    scope_end,
                    depth,
                );
                let mut scope = scope.to_vec();
                if !name.starts_with('$') {
                    if let Some(range) = name_range(tree, origin.span, name) {
                        let end = if origin.provenance == OriginKind::BlockBinding {
                            self.block_ends
                                .get(&origin.syntax)
                                .copied()
                                .unwrap_or(scope_end)
                        } else {
                            extent(map, &path.child(AstStep::LetBody), body).end
                        }
                        .min(scope_end);
                        let start = if origin.provenance == OriginKind::BlockBinding {
                            origin.span.end.byte
                        } else {
                            let value_end = extent(map, &path.child(AstStep::LetValue), value).end;
                            tree.source()
                                .lexemes()
                                .iter()
                                .find(|piece| {
                                    piece.span().start.byte >= value_end
                                        && matches!(piece.kind(), LexemeKind::Token(Kind::In))
                                })
                                .map_or(range.end, |piece| piece.span().end.byte)
                        };
                        let definition = Definition {
                            name: name.clone(),
                            range: range.clone(),
                            uri: self.uri.clone(),
                            detail: format!("let {name}"),
                            kind: 6,
                        };
                        self.hovers.push((range.clone(), definition.detail.clone()));
                        self.references
                            .push((range, definition.location(tree.source().text())));
                        self.locals.push((definition.clone(), start..end, depth));
                        scope.push(definition);
                    }
                }
                self.walk(
                    syntax,
                    &path.child(AstStep::LetBody),
                    body,
                    &scope,
                    scope_end,
                    depth + 1,
                );
            }
            Expr::Match(value, arms) => {
                self.walk(
                    syntax,
                    &path.child(AstStep::MatchValue),
                    value,
                    scope,
                    scope_end,
                    depth,
                );
                for (number, arm) in arms.iter().enumerate() {
                    let arm_path = path.child(AstStep::Arm(number));
                    let mut scope = scope.to_vec();
                    let body_path = arm_path.child(AstStep::ArmBody);
                    let visible = extent(map, &body_path, &arm.body);
                    self.pattern(
                        syntax,
                        &arm_path.child(AstStep::ArmPattern),
                        &arm.pattern,
                        &mut scope,
                        visible,
                        depth + 1,
                    );
                    self.walk(syntax, &body_path, &arm.body, &scope, scope_end, depth + 1);
                }
            }
            _ => {
                for step in AstNode::Expression(expression).child_steps() {
                    if let Some(AstNode::Expression(child)) =
                        AstNode::Expression(expression).child(step)
                    {
                        self.walk(syntax, &path.child(step), child, scope, scope_end, depth);
                    }
                }
            }
        }
    }

    fn pattern(
        &mut self,
        syntax: &Syntax<'_, '_>,
        path: &AstPath,
        pattern: &Pattern,
        scope: &mut Vec<Definition>,
        visible: Range<usize>,
        depth: usize,
    ) {
        let (tree, map) = (syntax.tree, syntax.map);
        match pattern {
            Pattern::Binding(name) => {
                if let Some(span) = map.origin(path).map(|origin| origin.span) {
                    if let Some(range) = name_range(tree, span, name) {
                        let definition = Definition {
                            name: name.clone(),
                            range: range.clone(),
                            uri: self.uri.clone(),
                            detail: name.clone(),
                            kind: 6,
                        };
                        self.references
                            .push((range, definition.location(tree.source().text())));
                        self.locals.push((definition.clone(), visible, depth));
                        scope.push(definition);
                    }
                }
            }
            Pattern::Constructor(_, fields) => {
                for (number, field) in fields.iter().enumerate() {
                    self.pattern(
                        syntax,
                        &path.child(AstStep::PatternField(number)),
                        field,
                        scope,
                        visible.clone(),
                        depth,
                    );
                }
            }
            _ => {}
        }
    }

    fn resolved(
        &mut self,
        uri: &str,
        overlays: &BTreeMap<String, String>,
        program: &SourceProgram,
    ) {
        let Some(path) = text::uri_to_path(uri) else {
            return;
        };
        let Ok(checked) = check::analyze_library(program.program()) else {
            return;
        };
        self.checked = true;
        let views: BTreeMap<_, _> = program
            .sources()
            .iter()
            .filter_map(|snapshot| {
                let tree = SyntaxTree::parse(&snapshot.text);
                let map = tree.ast_source_map().ok()?;
                Some((snapshot.id, (tree, map)))
            })
            .collect();
        for node in program.nodes() {
            for origin in &node.origins {
                let Some(snapshot) = program.source(origin.source) else {
                    continue;
                };
                if !same_path(&snapshot.path, &path) {
                    continue;
                }
                if origin.span.end.byte > snapshot.original_len {
                    if let Some(completion) = program.node(&node.path).and_then(header_completion) {
                        self.header_completions.push(completion);
                    }
                    continue;
                }
                let Some((tree, _)) = views.get(&origin.source) else {
                    continue;
                };
                if let Some(checked_node) = program.node(&node.path) {
                    self.checked_node(
                        tree,
                        &views[&origin.source].1,
                        &origin.path,
                        origin.span,
                        checked_node,
                        &checked,
                    );
                }
                if let Some(AstNode::Expression(expression)) = program.node(&node.path) {
                    if matches!(
                        origin.provenance,
                        OriginKind::Source
                            | OriginKind::BooleanNegation
                            | OriginKind::UnaryNegation
                    ) {
                        if let Some(ty) = checked.expression_type(expression) {
                            self.hovers.push((origin.span.byte_range(), type_text(ty)));
                        }
                    }
                }
                let id = match origin.lowering {
                    Lowering::ResolvedName(id) | Lowering::QualifiedValue(id) => id,
                    Lowering::Unchanged => continue,
                };
                let Some(symbol) = program.symbol(id) else {
                    continue;
                };
                let qualified = match tree
                    .parsed_module()
                    .and_then(|parsed| origin.path.resolve(parsed))
                {
                    Some(AstNode::Expression(Expr::Call(name, _) | Expr::Record(name, _))) => {
                        name.contains('.')
                    }
                    Some(AstNode::Expression(Expr::Field(..))) => {
                        matches!(origin.lowering, Lowering::QualifiedValue(_))
                    }
                    Some(AstNode::TypeExpression(ty)) => ty.name.contains('.'),
                    Some(AstNode::Pattern(Pattern::Constructor(name, _))) => name.contains('.'),
                    _ => false,
                };
                let selected = if qualified {
                    member_range(tree, origin.span, &symbol.symbol.name)
                } else {
                    name_range(tree, origin.span, &symbol.symbol.name)
                };
                let Some(range) = selected else {
                    continue;
                };
                let Some(target_source) = program.source(symbol.source) else {
                    continue;
                };
                let Some((target_tree, target_map)) = views.get(&symbol.source) else {
                    continue;
                };
                let Some(target_origin) = target_map.origin(&symbol.source_path) else {
                    continue;
                };
                let Some(target_range) =
                    name_range(target_tree, target_origin.span, &symbol.symbol.name)
                else {
                    continue;
                };
                let target_uri = std::iter::once(uri)
                    .chain(overlays.keys().map(String::as_str))
                    .find(|uri| {
                        text::uri_to_path(uri)
                            .is_some_and(|path| same_path(&path, &target_source.path))
                    })
                    .map(str::to_owned)
                    .or_else(|| text::path_to_uri(&target_source.path));
                if let Some(target_uri) =
                    target_uri.filter(|_| target_range.end <= target_source.original_len)
                {
                    self.references.push((range.clone(), json!({"uri": target_uri, "range": text::range(&target_source.text, target_range)})));
                }
                if let Some(AstNode::Function(function)) =
                    symbol.target.resolve_program(program.program())
                {
                    if let Some(signature) = checked.functions.get(&function.name) {
                        self.hovers.push((
                            range,
                            function_detail(&symbol.symbol.name, function, signature),
                        ));
                    }
                } else if let Some(AstNode::ExternFunction(function)) =
                    symbol.target.resolve_program(program.program())
                {
                    let arguments = function
                        .params
                        .iter()
                        .map(|parameter| {
                            format!(
                                "{}: {}",
                                parameter.name,
                                type_text(&check::extern_type(&parameter.ty))
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    self.hovers.push((
                        range,
                        format!(
                            "extern fn {}({arguments}) -> {}",
                            symbol.symbol.name,
                            type_text(&check::extern_type(&function.result))
                        ),
                    ));
                }
            }
        }
    }
}

pub(super) fn header_completion(node: AstNode<'_>) -> Option<Value> {
    let (name, kind, detail) = match node {
        AstNode::ExternFunction(function) => {
            let parameters = function
                .params
                .iter()
                .map(|parameter| {
                    format!(
                        "{}: {}",
                        parameter.name,
                        type_text(&check::extern_type(&parameter.ty))
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            (
                &function.name,
                3,
                format!(
                    "extern fn {}({parameters}) -> {}",
                    function.name,
                    type_text(&check::extern_type(&function.result))
                ),
            )
        }
        AstNode::Constant(constant) => (
            &constant.name,
            21,
            format!(
                "extern const {}: {}",
                constant.name,
                type_text(&check::extern_type(&constant.ty))
            ),
        ),
        _ => return None,
    };
    Some(json!({"label":name,"kind":kind,"detail":detail}))
}

fn extent(map: &AstSourceMap, path: &AstPath, expression: &Expr) -> Range<usize> {
    let (mut start, mut end) = (usize::MAX, 0);
    let mut work = vec![(path.clone(), AstNode::Expression(expression))];
    while let Some((path, node)) = work.pop() {
        if let Some(origin) = map.origin(&path) {
            start = start.min(origin.span.start.byte);
            end = end.max(origin.span.end.byte);
        }
        for step in node.child_steps() {
            if let Some(child) = node.child(step) {
                work.push((path.child(step), child));
            }
        }
    }
    if start == usize::MAX {
        0..0
    } else {
        start..end
    }
}
