//! Construction-only observer. Optional failures latch; no metadata helper can
//! return through semantic lowering. Source addresses live only for its borrow.
use super::shapes::{CaptureReceipt, ShapeBindingReceipt, ShapeReceipt, ShapeUseReceipt};
use super::structure::{fingerprint, Fingerprint};
use super::*;
use crate::{
    ast::Function,
    source_map::AstStep,
    typed::{LocalId, Type as IrType},
};

pub(in crate::typed) type Edges = Vec<(TypedChildRole, usize)>;
pub(in crate::typed) struct LoweredExpr {
    pub value: ExprNode,
    pub trace: Option<usize>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::typed) enum RawTarget {
    Source(usize),
    Fragment(usize),
    Shape(usize),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::typed) struct RawRelation {
    pub role: RelationRole,
    pub target: RawTarget,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::typed) enum RawOrigin {
    Source {
        source: usize,
        rule: DirectRule,
    },
    Generated {
        reason: GenerationReason,
        relations: Vec<RawRelation>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::typed) enum RawDisposition {
    Output(usize),
    OperandOnly(usize),
    EmbeddedOperand(usize),
    SignatureOnly,
    StaticShapeOnly { shape: usize, continuation: usize },
    RepresentedByShape(usize),
    AliasOfShape { shape: usize, binding: usize },
}
#[derive(Clone)]
pub(super) struct Fragment {
    pub instance: usize,
    pub origin: RawOrigin,
    pub fingerprint: Fingerprint,
    pub children: Edges,
}
pub(super) struct BindingReceipt {
    pub instance: usize,
    pub source: usize,
    pub local: LocalId,
}
pub(super) struct DispositionReceipt {
    pub instance: usize,
    pub source: usize,
    pub disposition: RawDisposition,
}

pub(super) struct SourceIndex<'a> {
    sources: &'a SourceProgram,
    nodes: Vec<AstNode<'a>>,
    addresses: HashMap<(u8, usize), usize>,
    children: HashMap<(usize, AstStep), usize>,
    roots: HashMap<AstDeclaration, usize>,
    function_ranges: HashMap<usize, std::ops::Range<usize>>,
}
impl<'a> SourceIndex<'a> {
    fn empty(sources: &'a SourceProgram) -> Self {
        Self {
            sources,
            nodes: Vec::new(),
            addresses: HashMap::new(),
            children: HashMap::new(),
            roots: HashMap::new(),
            function_ranges: HashMap::new(),
        }
    }
    fn new(sources: &'a SourceProgram, budget: &mut BodyBudget) -> Result<Self, String> {
        let mut result = Self::empty(sources);
        let mut ancestors: Vec<(usize, AstNode<'a>)> = Vec::new();
        let mut declaration = None;
        for (slot, record) in sources.nodes().iter().enumerate() {
            budget.records(1)?;
            budget.work(1 + record.path.steps.len())?;
            let depth = record.path.steps.len();
            let node = if depth == 0 {
                if result.roots.insert(record.path.declaration, slot).is_some() {
                    return Err(fail("duplicate body source root"));
                }
                declaration = Some(record.path.declaration);
                ancestors.clear();
                record
                    .path
                    .resolve_program(sources.program())
                    .ok_or_else(|| fail("missing source declaration"))?
            } else {
                if declaration != Some(record.path.declaration) || depth > ancestors.len() {
                    return Err(fail("non-preorder source index"));
                }
                ancestors.truncate(depth);
                let (parent, parent_node) = ancestors[depth - 1];
                let step = record.path.steps[depth - 1];
                let parent_path = &sources.nodes()[parent].path;
                if record.path.steps[..depth - 1] != parent_path.steps {
                    return Err(fail("source parent prefix mismatch"));
                }
                budget.edges(1)?;
                if result.children.insert((parent, step), slot).is_some() {
                    return Err(fail("duplicate source child"));
                }
                parent_node
                    .child(step)
                    .ok_or_else(|| fail("invalid indexed source child"))?
            };
            if let AstDeclaration::Function(function) = record.path.declaration {
                if depth == 0 {
                    budget.records(1)?;
                }
                result
                    .function_ranges
                    .entry(function)
                    .or_insert(slot..slot)
                    .end = slot + 1;
            }
            budget.work(1)?;
            if result.addresses.insert(address(node), slot).is_some() {
                return Err(fail("duplicate source occurrence address"));
            }
            result.nodes.push(node);
            budget.records(1)?;
            ancestors.push((slot, node));
        }
        Ok(result)
    }
    pub(super) fn function_range(&self, function: usize) -> Option<std::ops::Range<usize>> {
        self.function_ranges.get(&function).cloned()
    }
    pub(super) fn node(&self, slot: usize) -> Option<AstNode<'a>> {
        self.nodes.get(slot).copied()
    }
    pub(super) fn child(&self, slot: usize, step: AstStep) -> Option<usize> {
        self.children.get(&(slot, step)).copied()
    }
    pub(super) fn is_signature_annotation(
        &self,
        slot: usize,
        budget: &mut BodyBudget,
    ) -> Result<bool, String> {
        let Some(record) = self.sources.nodes().get(slot) else {
            return Ok(false);
        };
        budget.work(record.path.steps.len())?;
        let tail = match record.path.steps.as_slice() {
            [AstStep::Parameter(_), AstStep::Annotation, tail @ ..]
            | [AstStep::FunctionResult, tail @ ..] => tail,
            _ => return Ok(false),
        };
        Ok(tail
            .iter()
            .all(|step| matches!(step, AstStep::TypeArgument(_))))
    }
    pub(super) fn function(&self, slot: usize) -> Option<usize> {
        match self.sources.nodes().get(slot)?.path.declaration {
            AstDeclaration::Function(i) => Some(i),
            _ => None,
        }
    }
    #[cfg(test)]
    pub(super) fn sources(&self) -> &'a SourceProgram {
        self.sources
    }
    pub(super) fn root(&self, declaration: AstDeclaration) -> Option<usize> {
        self.roots.get(&declaration).copied()
    }
    fn find(&self, node: AstNode<'_>) -> Option<usize> {
        self.addresses.get(&address(node)).copied()
    }
}
fn address(node: AstNode<'_>) -> (u8, usize) {
    let pointer = match node {
        AstNode::Function(v) => v as *const _ as usize,
        AstNode::ExternFunction(v) => v as *const _ as usize,
        AstNode::Constant(v) => v as *const _ as usize,
        AstNode::TypeDeclaration(v) => v as *const _ as usize,
        AstNode::Variant(v) => v as *const _ as usize,
        AstNode::Parameter(v) => v as *const _ as usize,
        AstNode::ForeignParameter(v) => v as *const _ as usize,
        AstNode::ForeignType(v) => v as *const _ as usize,
        AstNode::TypeExpression(v) => v as *const _ as usize,
        AstNode::Expression(v) => v as *const _ as usize,
        AstNode::Pattern(v) => v as *const _ as usize,
        AstNode::MatchArm(v) => v as *const _ as usize,
    };
    (node.kind() as u8, pointer)
}

pub(in crate::typed) struct Trace<'a> {
    pub(super) budget: BodyBudget,
    pub(super) failure: Option<String>,
    pub(super) instance: usize,
    pub(super) source_index: SourceIndex<'a>,
    pub(super) fragments: Vec<Fragment>,
    pub(super) roots: Vec<usize>,
    pub(super) bindings: Vec<BindingReceipt>,
    pub(super) dispositions: Vec<DispositionReceipt>,
    pub(super) shapes: Vec<ShapeReceipt>,
    pub(super) shape_bindings: Vec<ShapeBindingReceipt>,
    pub(super) uses: Vec<ShapeUseReceipt>,
    pub(super) captures: Vec<CaptureReceipt>,
    pub(super) binding_sources: HashMap<(usize, usize), usize>,
    pub(super) shape_binding_index: HashMap<(usize, usize), usize>,
    frames: Vec<Edges>,
}
impl<'a> Trace<'a> {
    pub(super) fn new(sources: &'a SourceProgram, mut budget: BodyBudget) -> Self {
        let (source_index, failure) = match SourceIndex::new(sources, &mut budget) {
            Ok(index) => (index, None),
            Err(error) => (SourceIndex::empty(sources), Some(error)),
        };
        if failure.is_some() {
            budget.failed.set(true);
        }
        Self {
            budget,
            failure,
            instance: 0,
            source_index,
            fragments: Vec::new(),
            roots: Vec::new(),
            bindings: Vec::new(),
            dispositions: Vec::new(),
            shapes: Vec::new(),
            shape_bindings: Vec::new(),
            uses: Vec::new(),
            captures: Vec::new(),
            binding_sources: HashMap::new(),
            shape_binding_index: HashMap::new(),
            frames: Vec::new(),
        }
    }
    pub(in crate::typed) fn observe<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Option<T> {
        if self.failure.is_some() || self.budget.failed.get() {
            return None;
        }
        match f(self) {
            Ok(value) => Some(value),
            Err(error) => {
                self.budget.failed.set(true);
                self.failure = Some(error);
                None
            }
        }
    }
    pub(in crate::typed) fn source(&mut self, node: AstNode<'_>) -> Option<usize> {
        self.observe(|trace| {
            trace.budget.work(1)?;
            trace
                .source_index
                .find(node)
                .ok_or_else(|| fail("construction source has no indexed occurrence"))
        })
    }
    pub(in crate::typed) fn check(&self) -> Result<(), String> {
        match &self.failure {
            Some(error) => Err(error.clone()),
            None if self.budget.failed.get() => {
                Err("E-TYPED-SOURCE-MAP-LIMIT: combined origin trace disabled".into())
            }
            None => Ok(()),
        }
    }
    pub(in crate::typed) fn set_instance(&mut self, instance: usize) {
        self.instance = instance;
    }
    pub(in crate::typed) fn edge(
        &mut self,
        edges: &mut Edges,
        role: TypedChildRole,
        child: Option<usize>,
    ) {
        self.observe(|trace| {
            let child = child.ok_or_else(|| fail("missing child fragment"))?;
            trace.budget.edges(1)?;
            trace.budget.work(1)?;
            edges.push((role, child));
            Ok(())
        });
    }
    pub(in crate::typed) fn relation(
        &mut self,
        edges: &mut Vec<RawRelation>,
        role: RelationRole,
        target: RawTarget,
    ) {
        self.observe(|trace| {
            trace.budget.work(1 + edges.len())?;
            let relation = RawRelation { role, target };
            if !edges.contains(&relation) {
                trace.budget.edges(1)?;
                edges.push(relation);
            }
            Ok(())
        });
    }
    pub(in crate::typed) fn disposition(&mut self, source: usize, disposition: RawDisposition) {
        self.observe(|trace| {
            trace.budget.records(1)?;
            trace.budget.work(1)?;
            trace.dispositions.push(DispositionReceipt {
                instance: trace.instance,
                source,
                disposition,
            });
            Ok(())
        });
    }
    fn record(
        &mut self,
        origin: RawOrigin,
        node: TypedNodeRef<'_>,
        children: Edges,
    ) -> Option<usize> {
        self.observe(|trace| {
            trace.budget.records(1)?;
            trace.budget.work(1)?;
            let fingerprint = fingerprint(node, &mut trace.budget)?;
            let slot = trace.fragments.len();
            trace.fragments.push(Fragment {
                instance: trace.instance,
                origin,
                fingerprint,
                children,
            });
            Ok(slot)
        })
    }
    pub(in crate::typed) fn direct(
        &mut self,
        source: AstNode<'_>,
        rule: DirectRule,
        node: TypedNodeRef<'_>,
        children: Edges,
    ) -> Option<usize> {
        let source = self.source(source)?;
        let fragment = self.record(RawOrigin::Source { source, rule }, node, children)?;
        let disposition = match rule {
            DirectRule::Argument => RawDisposition::OperandOnly(fragment),
            DirectRule::EmbeddedCapacity => RawDisposition::EmbeddedOperand(fragment),
            _ => RawDisposition::Output(fragment),
        };
        self.disposition(source, disposition);
        Some(fragment)
    }
    pub(in crate::typed) fn generated(
        &mut self,
        reason: GenerationReason,
        relations: Vec<RawRelation>,
        node: TypedNodeRef<'_>,
        children: Edges,
    ) -> Option<usize> {
        self.record(RawOrigin::Generated { reason, relations }, node, children)
    }
    pub(in crate::typed) fn bind(&mut self, node: AstNode<'_>, local: LocalId) {
        if let Some(source) = self.source(node) {
            self.observe(|trace| {
                trace.budget.records(1)?;
                trace.budget.work(1)?;
                trace.bindings.push(BindingReceipt {
                    instance: trace.instance,
                    source,
                    local,
                });
                Ok(())
            });
        }
    }
    pub(in crate::typed) fn begin(&mut self) -> Option<usize> {
        self.observe(|trace| {
            trace.budget.records(1)?;
            trace.budget.work(1)?;
            let depth = trace.frames.len();
            trace.frames.push(Vec::new());
            Ok(depth)
        })
    }
    pub(in crate::typed) fn attach(&mut self, role: TypedChildRole, child: Option<usize>) {
        self.observe(|trace| {
            trace.budget.edges(1)?;
            trace.budget.work(1)?;
            let child = child.ok_or_else(|| fail("missing scoped child fragment"))?;
            trace
                .frames
                .last_mut()
                .ok_or_else(|| fail("child outside construction frame"))?
                .push((role, child));
            Ok(())
        });
    }
    pub(in crate::typed) fn end(&mut self, depth: Option<usize>) -> Edges {
        self.observe(|trace| {
            let depth = depth.ok_or_else(|| fail("missing construction frame"))?;
            if trace.frames.len() != depth + 1 {
                return Err(fail("unbalanced construction frames"));
            }
            trace.budget.records += 1;
            Ok(trace.frames.pop().unwrap())
        })
        .unwrap_or_default()
    }
    pub(in crate::typed) fn finish_expression(
        &mut self,
        depth: Option<usize>,
        expr: &Expr,
        node: &ExprNode,
    ) -> Option<usize> {
        let mut children = self.end(depth);
        if let (Expr::Call(_, args), crate::typed::ExprKind::BytesNew(capacity)) =
            (expr, &node.kind)
        {
            if let Some(arg) = args.first() {
                let fragment = self.direct(
                    AstNode::Expression(arg),
                    DirectRule::EmbeddedCapacity,
                    TypedNodeRef::Capacity(capacity),
                    Vec::new(),
                );
                self.edge(&mut children, TypedChildRole::Capacity, fragment);
            }
        }
        self.direct(
            AstNode::Expression(expr),
            DirectRule::Expression,
            TypedNodeRef::Expression(node),
            children,
        )
    }
    pub(in crate::typed) fn signature(
        &mut self,
        function: &Function,
        params: &[(LocalId, IrType, ParamMode)],
        result: &IrType,
        body: &ExprNode,
        body_fragment: Option<usize>,
    ) {
        let mut children = Vec::new();
        for (index, (param, value)) in function.params.iter().zip(params).enumerate() {
            let fragment = self.direct(
                AstNode::Parameter(param),
                DirectRule::Parameter,
                TypedNodeRef::Parameter(value),
                Vec::new(),
            );
            self.edge(&mut children, TypedChildRole::Parameter(index), fragment);
            if let Some(slot) = self.source(AstNode::Parameter(param)) {
                self.signature_annotations(slot, AstStep::Annotation);
            }
        }
        let mut relations = Vec::new();
        if let Some(slot) = self.source(AstNode::Function(function)) {
            self.relation(
                &mut relations,
                RelationRole::Definition,
                RawTarget::Source(slot),
            );
        }
        if let Some(slot) = self.source(AstNode::Expression(&function.body)) {
            self.relation(&mut relations, RelationRole::Body, RawTarget::Source(slot));
        }
        let reason = if function.result_annotation.is_some() {
            if let Some(root) = self.source(AstNode::Function(function)) {
                self.signature_annotations(root, AstStep::FunctionResult);
                if let Some(annotation) = self.source_index.child(root, AstStep::FunctionResult) {
                    self.relation(
                        &mut relations,
                        RelationRole::Annotation,
                        RawTarget::Source(annotation),
                    );
                }
            }
            GenerationReason::AnnotatedResultType
        } else {
            GenerationReason::InferredResultType
        };
        let result_fragment =
            self.generated(reason, relations, TypedNodeRef::Result(result), Vec::new());
        self.edge(&mut children, TypedChildRole::Result, result_fragment);
        self.edge(&mut children, TypedChildRole::FunctionBody, body_fragment);
        let root = self.direct(
            AstNode::Function(function),
            DirectRule::Function,
            TypedNodeRef::Function {
                params,
                result,
                body,
            },
            children,
        );
        self.observe(|trace| {
            trace.budget.records(1)?;
            trace
                .roots
                .push(root.ok_or_else(|| fail("missing function body fragment"))?);
            Ok(())
        });
    }
    fn signature_annotations(&mut self, root: usize, role: AstStep) {
        self.observe(|trace| {
            let mut pending = Vec::new();
            if let Some(annotation) = trace.source_index.child(root, role) {
                trace.budget.records(1)?;
                pending.push(annotation);
            }
            while let Some(source) = pending.pop() {
                trace.budget.work(1)?;
                trace.disposition(source, RawDisposition::SignatureOnly);
                trace.check()?;
                let Some(AstNode::TypeExpression(ty)) = trace.source_index.node(source) else {
                    return Err(fail("invalid signature annotation"));
                };
                trace.budget.records(ty.args.len())?;
                for i in 0..ty.args.len() {
                    pending.push(
                        trace
                            .source_index
                            .child(source, AstStep::TypeArgument(i))
                            .ok_or_else(|| fail("missing annotation child"))?,
                    );
                }
            }
            Ok(())
        });
    }
}
