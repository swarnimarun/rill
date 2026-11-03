//! Owned origins for the exact module-lowered program. Metadata is built only
//! after ordinary loading, resolution and checking succeed, never instead of them.
use super::{
    resolve::{Kind, ModuleId, Symbol, SymbolId},
    ModuleError, ModuleGraph,
};
use crate::{
    ast::{Expr, Pattern, Program},
    parser::ExportKind,
    source::Span,
    source_map::{AstNode, AstNodeKind, AstPath, OriginKind},
    syntax::{AstDeclaration, SyntaxKind, SyntaxTree},
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
};

/// An index local to one `SourceProgram`, not an incremental/cache identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceId(pub(super) usize);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceSnapshot {
    pub id: SourceId,
    pub module: String,
    /// Canonical filesystem path, or the loader's bundled-module diagnostic path.
    pub path: PathBuf,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Contribution {
    /// This declaration supplied the retained output AST.
    Retained,
    /// A separately validated, compatible native declaration. Parameter names
    /// may differ; this is an ABI contribution, not identical source spelling.
    CoalescedNative,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lowering {
    Unchanged,
    /// The lowering pass resolved this node's name to this declaration identity.
    ResolvedName(SymbolId),
    /// A qualified source Field expression became a resolved Var expression.
    QualifiedValue(SymbolId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceOrigin {
    pub source: SourceId,
    /// Structural path in this source's original parse, not in the output.
    pub path: AstPath,
    /// A source anchor, not necessarily a complete semantic-subtree extent.
    pub span: Span,
    pub syntax: SyntaxKind,
    pub provenance: OriginKind,
    pub contribution: Contribution,
    pub lowering: Lowering,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedNode {
    /// Structural address in the exact, immutable flattened program.
    pub path: AstPath,
    pub kind: AstNodeKind,
    /// Retained origin first, followed by compatible native contributors.
    pub origins: Vec<SourceOrigin>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedSymbol {
    /// The actual resolver identity, including original name/visibility/owner.
    pub symbol: Symbol,
    pub source: SourceId,
    pub source_path: AstPath,
    /// A declaration root or type-variant path in the flattened program.
    /// Compatible native symbols from different modules can share this target.
    pub target: AstPath,
}

/// Read-only paired program and provenance. Cloning the program does not carry
/// origins to subsequent edits. `into_program` intentionally discards metadata.
/// No typed IR, backend, foreign ABI or incremental identity claim is made.
#[derive(Clone, Debug)]
pub struct SourceProgram {
    program: Program,
    root: PathBuf,
    sources: Vec<SourceSnapshot>,
    nodes: Vec<ResolvedNode>,
    index: BTreeMap<AstPath, usize>,
    symbols: Vec<ResolvedSymbol>,
    symbol_index: HashMap<SymbolId, usize>,
}
impl SourceProgram {
    pub fn program(&self) -> &Program {
        &self.program
    }
    pub fn into_program(self) -> Program {
        self.program
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn sources(&self) -> &[SourceSnapshot] {
        &self.sources
    }
    pub fn source(&self, id: SourceId) -> Option<&SourceSnapshot> {
        self.sources.get(id.0)
    }
    /// Sorted module order, source declaration order, then semantic preorder;
    /// later coalesced declarations add origins to the existing node entries.
    pub fn nodes(&self) -> &[ResolvedNode] {
        &self.nodes
    }
    pub fn origin(&self, path: &AstPath) -> Option<&ResolvedNode> {
        self.index.get(path).map(|&i| &self.nodes[i])
    }
    pub fn node(&self, path: &AstPath) -> Option<AstNode<'_>> {
        self.origin(path)?;
        path.resolve_program(&self.program)
    }
    /// Use an origin from this result, never one from a different load.
    pub fn origin_text(&self, origin: &SourceOrigin) -> Option<&str> {
        self.source(origin.source)?
            .text
            .get(origin.span.byte_range())
    }
    pub fn symbols(&self) -> &[ResolvedSymbol] {
        &self.symbols
    }
    pub fn symbol(&self, id: SymbolId) -> Option<&ResolvedSymbol> {
        self.symbol_index.get(&id).map(|&i| &self.symbols[i])
    }
}

pub(super) struct Transfer {
    source: SourceId,
    from: AstDeclaration,
    to: AstDeclaration,
    contribution: Contribution,
    /// Captured at the actual declaration transfer, independently of the
    /// separately exposed symbol table. Variants inherit this module owner.
    owner: ModuleId,
}
#[derive(Default)]
pub(super) struct Trace {
    transfers: Vec<Transfer>,
    symbols: Vec<ResolvedSymbol>,
    changes: Vec<(SourceId, AstPath, Lowering)>,
}
impl Trace {
    pub(super) fn transfer(
        &mut self,
        source: SourceId,
        from: AstDeclaration,
        to: AstDeclaration,
        contribution: Contribution,
        symbol: &Symbol,
    ) {
        self.transfers.push(Transfer {
            source,
            from,
            to,
            contribution,
            owner: symbol.owner,
        });
        self.symbol(source, AstPath::root(from), AstPath::root(to), symbol);
    }
    pub(super) fn symbol(
        &mut self,
        source: SourceId,
        source_path: AstPath,
        target: AstPath,
        symbol: &Symbol,
    ) {
        self.symbols.push(ResolvedSymbol {
            symbol: symbol.clone(),
            source,
            source_path,
            target,
        });
    }
    pub(super) fn change(&mut self, source: SourceId, path: AstPath, change: Lowering) {
        self.changes.push((source, path, change));
    }
}

fn fail(message: impl Into<String>) -> ModuleError {
    ModuleError {
        code: "E-MODULE-SOURCE-MAP",
        message: message.into(),
    }
}
fn roots(program: &Program) -> impl Iterator<Item = AstDeclaration> + '_ {
    (0..program.functions.len())
        .map(AstDeclaration::Function)
        .chain((0..program.externs.len()).map(AstDeclaration::ExternFunction))
        .chain((0..program.constants.len()).map(AstDeclaration::Constant))
        .chain((0..program.types.len()).map(AstDeclaration::Type))
}
fn node_name(node: AstNode<'_>) -> Option<&str> {
    match node {
        AstNode::Function(n) => Some(&n.name),
        AstNode::ExternFunction(n) => Some(&n.name),
        AstNode::Constant(n) => Some(&n.name),
        AstNode::TypeDeclaration(n) => Some(&n.name),
        AstNode::Variant(n) => Some(&n.name),
        AstNode::TypeExpression(n) => Some(&n.name),
        AstNode::Expression(Expr::Var(n) | Expr::Call(n, _) | Expr::Record(n, _))
        | AstNode::Pattern(Pattern::Constructor(n, _)) => Some(n),
        _ => None,
    }
}
fn definition(
    node: AstNode<'_>,
    path: &AstPath,
    program: &Program,
    exports: &HashSet<(&str, ExportKind)>,
) -> Option<(Kind, bool)> {
    let (kind, export, name) = match node {
        AstNode::Function(f) => (Kind::Function, ExportKind::Function, f.name.as_str()),
        AstNode::ExternFunction(f) => (Kind::Function, ExportKind::Function, f.name.as_str()),
        AstNode::Constant(c) => (Kind::Constant, ExportKind::Constant, c.name.as_str()),
        AstNode::TypeDeclaration(t) => (Kind::Type, ExportKind::Type, t.name.as_str()),
        AstNode::Variant(_) => {
            let AstDeclaration::Type(index) = path.declaration else {
                return None;
            };
            (
                Kind::Constructor,
                ExportKind::Type,
                program.types.get(index)?.name.as_str(),
            )
        }
        _ => return None,
    };
    Some((kind, exports.contains(&(name, export))))
}

fn same_shape(before: AstNode<'_>, after: AstNode<'_>, lowering: Lowering) -> bool {
    if before.kind() != after.kind() || before.child_steps() != after.child_steps() {
        return false;
    }
    if matches!(lowering, Lowering::QualifiedValue(_)) {
        return matches!(
            (before, after),
            (
                AstNode::Expression(Expr::Field(..)),
                AstNode::Expression(Expr::Var(_))
            )
        );
    }
    match (before, after) {
        (AstNode::Expression(a), AstNode::Expression(b)) => {
            std::mem::discriminant(a) == std::mem::discriminant(b)
        }
        (AstNode::Pattern(a), AstNode::Pattern(b)) => {
            std::mem::discriminant(a) == std::mem::discriminant(b)
        }
        (AstNode::ForeignType(a), AstNode::ForeignType(b)) => {
            std::mem::discriminant(a) == std::mem::discriminant(b)
        }
        _ => true,
    }
}

/// Called only after the ordinary lowering/checking pass has succeeded. The
/// shared parser is replayed over owned graph snapshots, with no filesystem or
/// native-tool reads. Matching uses recorded source/destination slots, not names
/// or AST equality. Syntax-tree IDs are consumed here and never escape.
pub(super) fn finish(
    graph: ModuleGraph,
    program: Program,
    trace: Trace,
) -> Result<SourceProgram, ModuleError> {
    let mut transfers = BTreeMap::new();
    for transfer in &trace.transfers {
        if transfers
            .insert((transfer.source, transfer.from), transfer)
            .is_some()
        {
            return Err(fail("source declaration transferred more than once"));
        }
    }
    let mut changes = BTreeMap::new();
    for (source, path, lowering) in trace.changes {
        if changes.insert((source, path), lowering).is_some() {
            return Err(fail("source node rewritten more than once"));
        }
    }
    let mut result = SourceProgram {
        program,
        root: graph.root,
        sources: vec![],
        nodes: vec![],
        index: BTreeMap::new(),
        symbols: trace.symbols,
        symbol_index: HashMap::new(),
    };
    let mut symbol_slots = BTreeMap::new();
    for (index, symbol) in result.symbols.iter().enumerate() {
        if symbol_slots
            .insert((symbol.source, symbol.source_path.clone()), index)
            .is_some()
        {
            return Err(fail("source declaration has multiple symbol identities"));
        }
        if result
            .symbol_index
            .insert(symbol.symbol.id, index)
            .is_some()
        {
            return Err(fail("resolved symbol recorded more than once"));
        }
    }
    let mut seen = HashSet::new();
    for (i, (module, input)) in graph.modules.into_iter().enumerate() {
        let source = SourceId(i);
        let tree = SyntaxTree::parse(&input.source);
        let map = tree
            .ast_source_map()
            .map_err(|error| fail(format!("{}: {error}", input.path.display())))?;
        let parsed = tree
            .parsed_module()
            .ok_or_else(|| fail("checked source has no parsed module"))?;
        let exports: HashSet<_> = parsed
            .exports
            .iter()
            .map(|e| (e.name.as_str(), e.kind))
            .collect();
        for origin in map.entries() {
            let transfer = transfers
                .get(&(source, origin.path.declaration))
                .ok_or_else(|| {
                    fail(format!(
                        "unrecorded source declaration: {module} {:?}",
                        origin.path
                    ))
                })?;
            seen.insert((source, origin.path.declaration));
            let path = AstPath {
                declaration: transfer.to,
                steps: origin.path.steps.clone(),
            };
            let lowering = changes
                .remove(&(source, origin.path.clone()))
                .unwrap_or(Lowering::Unchanged);
            let before = origin
                .path
                .resolve(parsed)
                .ok_or_else(|| fail("invalid original AST path"))?;
            let after = path
                .resolve_program(&result.program)
                .ok_or_else(|| fail("invalid output AST path"))?;
            // Every original declaration/variant, including every coalesced
            // native declaration, needs exactly one actual resolver identity.
            // These are checks at recorded slots, never lookup by spelling.
            if let Some((kind, exported)) =
                definition(before, &origin.path, &parsed.program, &exports)
            {
                let index = symbol_slots
                    .remove(&(source, origin.path.clone()))
                    .ok_or_else(|| fail("source declaration has no symbol identity"))?;
                let symbol = &result.symbols[index];
                if symbol.target != path
                    || symbol.symbol.kind != kind
                    || symbol.symbol.exported != exported
                    || symbol.symbol.owner != transfer.owner
                    || Some(symbol.symbol.name.as_str()) != node_name(before)
                {
                    return Err(fail(
                        "resolved symbol disagrees with its source declaration",
                    ));
                }
            }
            if !same_shape(before, after, lowering) {
                return Err(fail(format!("unrecorded structural rewrite at {path:?}")));
            }
            match lowering {
                Lowering::Unchanged if node_name(before) != node_name(after) => {
                    return Err(fail(format!("unrecorded name rewrite at {path:?}")));
                }
                Lowering::ResolvedName(id) | Lowering::QualifiedValue(id) => {
                    let target = result
                        .symbol(id)
                        .and_then(|symbol| symbol.target.resolve_program(&result.program))
                        .ok_or_else(|| fail("rewrite has no recorded symbol target"))?;
                    if node_name(after).is_none() || node_name(after) != node_name(target) {
                        return Err(fail(format!(
                            "rewrite disagrees with resolved target at {path:?}"
                        )));
                    }
                }
                _ => {}
            }
            let owned = SourceOrigin {
                source,
                path: origin.path.clone(),
                span: origin.span,
                syntax: tree
                    .node(origin.syntax)
                    .ok_or_else(|| fail("invalid grammar origin"))?
                    .kind(),
                provenance: origin.provenance,
                contribution: transfer.contribution,
                lowering,
            };
            if input.source.get(owned.span.byte_range()).is_none() {
                return Err(fail("source anchor is outside its snapshot"));
            }
            if let Some(&index) = result.index.get(&path) {
                let node = &mut result.nodes[index];
                if owned.contribution != Contribution::CoalescedNative
                    || node.kind != after.kind()
                    || !matches!(
                        path.declaration,
                        AstDeclaration::ExternFunction(_) | AstDeclaration::Constant(_)
                    )
                {
                    return Err(fail("unexpected multiple origins for output node"));
                }
                node.origins.push(owned);
            } else {
                if owned.contribution != Contribution::Retained {
                    return Err(fail("coalesced origin precedes retained declaration"));
                }
                result.index.insert(path.clone(), result.nodes.len());
                result.nodes.push(ResolvedNode {
                    path,
                    kind: after.kind(),
                    origins: vec![owned],
                });
            }
        }
        result.sources.push(SourceSnapshot {
            id: source,
            module,
            path: input.path,
            text: input.source,
        });
    }
    if !symbol_slots.is_empty() {
        return Err(fail("resolved symbols lack source declarations"));
    }
    if seen.len() != transfers.len() || !changes.is_empty() {
        return Err(fail(
            "recorded declarations or rewrites lack source origins",
        ));
    }
    // Independently enumerate the exact final program, including all generated
    // nodes, to reject holes/extras rather than returning a partial map.
    let mut work: Vec<_> = roots(&result.program).map(AstPath::root).collect();
    let mut count = 0;
    while let Some(path) = work.pop() {
        let node = path
            .resolve_program(&result.program)
            .ok_or_else(|| fail("invalid final AST path"))?;
        let entry = result
            .origin(&path)
            .ok_or_else(|| fail(format!("unmapped final AST node: {path:?}")))?;
        if entry.kind != node.kind() {
            return Err(fail("final AST category differs"));
        }
        count += 1;
        work.extend(node.child_steps().into_iter().map(|step| path.child(step)));
    }
    if count != result.nodes.len() {
        return Err(fail("extra final AST origins"));
    }
    for symbol in &result.symbols {
        let node = result
            .origin(&symbol.target)
            .ok_or_else(|| fail("symbol has no output origin"))?;
        if !node
            .origins
            .iter()
            .any(|o| o.source == symbol.source && o.path == symbol.source_path)
        {
            return Err(fail("symbol has no contributing source origin"));
        }
        if !matches!(
            (symbol.symbol.kind, node.kind),
            (Kind::Type, AstNodeKind::TypeDeclaration)
                | (Kind::Constructor, AstNodeKind::Variant)
                | (
                    Kind::Function,
                    AstNodeKind::Function | AstNodeKind::ExternFunction
                )
                | (Kind::Constant, AstNodeKind::Constant)
        ) {
            return Err(fail("symbol target has the wrong category"));
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::{
        resolve::{Declaration, Namespace, Resolver},
        Module,
    };

    fn fixture() -> (ModuleGraph, Program, Trace) {
        let text = "fn main() { 1 }";
        let graph = ModuleGraph {
            root: ".".into(),
            modules: BTreeMap::from([(
                "@entry".into(),
                Module {
                    name: "@entry".into(),
                    path: "<test>".into(),
                    source: text.into(),
                    imports: vec![],
                },
            )]),
        };
        let program = crate::parser::parse_module(text).unwrap().program;
        crate::check::analyze(&program).unwrap();
        let resolver = Resolver::new(
            &graph,
            &BTreeMap::from([(
                "@entry".into(),
                vec![Declaration {
                    name: "main".into(),
                    kind: Kind::Function,
                    exported: false,
                }],
            )]),
        )
        .unwrap();
        let symbol = resolver
            .resolve("@entry", None, "main", Namespace::Value)
            .unwrap();
        let mut trace = Trace::default();
        trace.transfer(
            SourceId(0),
            AstDeclaration::Function(0),
            AstDeclaration::Function(0),
            Contribution::Retained,
            symbol,
        );
        (graph, program, trace)
    }
    fn rejects(change: impl FnOnce(&mut ModuleGraph, &mut Program, &mut Trace)) {
        let (mut graph, mut program, mut trace) = fixture();
        change(&mut graph, &mut program, &mut trace);
        assert_eq!(
            finish(graph, program, trace).unwrap_err().code,
            "E-MODULE-SOURCE-MAP"
        );
        // Optional metadata failures do not mutate even the original strict parse.
        let (graph, program, trace) = fixture();
        assert_eq!(
            finish(graph, program.clone(), trace).unwrap().program(),
            &program
        );
    }

    #[test]
    fn mapping_rejects_missing_duplicate_and_wrong_transfer_slots() {
        rejects(|_, _, trace| trace.transfers.clear());
        rejects(|_, _, trace| trace.transfers[0].to = AstDeclaration::Function(8));
        rejects(|_, _, trace| trace.transfers[0].source = SourceId(3));
        rejects(|_, _, trace| {
            let symbol = trace.symbols[0].symbol.clone();
            trace.transfer(
                SourceId(0),
                AstDeclaration::Function(0),
                AstDeclaration::Function(0),
                Contribution::Retained,
                &symbol,
            );
        });
        rejects(|_, _, trace| trace.transfers[0].contribution = Contribution::CoalescedNative);
    }

    #[test]
    fn mapping_rejects_unrecorded_rewrites_and_invented_qualified_changes() {
        rejects(|_, program, _| program.functions[0].name = "invented".into());
        rejects(|_, program, _| program.functions[0].body = Expr::Bool(true));
        rejects(|_, _, trace| {
            let id = trace.symbols[0].symbol.id;
            trace.change(
                SourceId(0),
                AstPath::root(AstDeclaration::Function(0))
                    .child(crate::source_map::AstStep::FunctionBody),
                Lowering::QualifiedValue(id),
            );
        });
        rejects(|_, _, trace| {
            let id = trace.symbols[0].symbol.id;
            trace.change(
                SourceId(0),
                AstPath::root(AstDeclaration::Function(2)),
                Lowering::ResolvedName(id),
            );
        });
        rejects(|_, _, trace| {
            let id = trace.symbols[0].symbol.id;
            for _ in 0..2 {
                trace.change(
                    SourceId(0),
                    AstPath::root(AstDeclaration::Function(0)),
                    Lowering::ResolvedName(id),
                );
            }
        });
    }

    #[test]
    fn mapping_rejects_stale_snapshots_missing_symbols_and_extra_output() {
        rejects(|graph, _, _| graph.modules.get_mut("@entry").unwrap().source = "fn main(".into());
        rejects(|_, _, trace| trace.symbols[0].target = AstPath::root(AstDeclaration::Type(0)));
        rejects(|_, _, trace| {
            trace.symbols[0].source_path = AstPath::root(AstDeclaration::Function(7))
        });
        rejects(|_, program, _| program.functions.push(program.functions[0].clone()));
    }

    #[test]
    fn mapping_requires_complete_and_consistent_source_symbol_identities() {
        rejects(|_, _, trace| trace.symbols.clear());
        rejects(|_, _, trace| trace.symbols[0].symbol.name = "not_the_source_declaration".into());
        rejects(|_, _, trace| trace.symbols[0].symbol.kind = Kind::Constant);
        rejects(|_, _, trace| trace.symbols[0].symbol.exported = true);
        let other_symbol = || {
            let (mut graph, _, _) = fixture();
            graph.modules.insert(
                "other".into(),
                Module {
                    name: "other".into(),
                    path: "<other>".into(),
                    source: "fn other() { 1 }".into(),
                    imports: vec![],
                },
            );
            let resolver = Resolver::new(
                &graph,
                &BTreeMap::from([
                    (
                        "@entry".into(),
                        vec![Declaration {
                            name: "main".into(),
                            kind: Kind::Function,
                            exported: false,
                        }],
                    ),
                    (
                        "other".into(),
                        vec![Declaration {
                            name: "other".into(),
                            kind: Kind::Function,
                            exported: false,
                        }],
                    ),
                ]),
            )
            .unwrap();
            resolver
                .resolve("other", None, "other", Namespace::Value)
                .unwrap()
                .clone()
        };
        rejects(|_, _, trace| trace.symbols[0].symbol.owner = other_symbol().owner);
        rejects(|_, _, trace| {
            let mut duplicate = trace.symbols[0].clone();
            duplicate.symbol.id = other_symbol().id;
            trace.symbols.push(duplicate);
        });
    }
}
