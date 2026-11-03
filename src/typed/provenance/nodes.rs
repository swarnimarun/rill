//! Opt-in origins for complete pure Copy function bodies. Layouts, ownership,
//! native operations and generated-C ranges are deliberately outside coverage.
pub(in crate::typed) mod shapes;
mod structure;
#[cfg(test)]
mod tests;
mod trace;
mod validate;

use super::*;
use crate::typed::ExprNode;
pub use shapes::{ShapeChild, ShapeKind, ShapeRecord};
use std::{cell::Cell, rc::Rc};
pub use structure::{TypedChildRole, TypedNodeKind, TypedNodeRef};
pub(in crate::typed) use trace::{
    Edges, LoweredExpr, RawDisposition, RawRelation, RawTarget, Trace,
};

handle!(TypedNodeId);
handle!(ShapeId);
handle!(RelationSetId);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coverage {
    CopyFunctionBodies,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectRule {
    Function,
    Parameter,
    Expression,
    Argument,
    RecordField,
    Arm,
    Pattern,
    EmbeddedCapacity,
    ProjectedUse,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenerationReason {
    InferredResultType,
    ReadArgumentTemporary,
    CaptureEnvironmentBinding,
    CaptureProduct,
    CaptureProductField,
    ShapeMaterialization,
    ShapeRecordField,
    CaptureProjection,
    ProjectedCaptureTemporary,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RelationRole {
    CausedByUse,
    IntroducedForBinding,
    CapturedLeaf,
    SelectedShape,
    ReadsCapture,
    Definition,
    Body,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelationTarget {
    Source(SourceNodeId),
    Typed(TypedNodeId),
    Shape(ShapeId),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Relation {
    pub role: RelationRole,
    pub target: RelationTarget,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeOrigin {
    SourceDerived {
        source: SourceNodeId,
        rule: DirectRule,
    },
    Generated {
        reason: GenerationReason,
        related: RelationSetId,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypedNodeRecord {
    pub id: TypedNodeId,
    pub instance: FunctionInstanceId,
    pub parent: Option<TypedNodeId>,
    pub role: TypedChildRole,
    pub kind: TypedNodeKind,
    pub origin: NodeOrigin,
    children: Vec<TypedNodeId>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceDisposition {
    Output(TypedNodeId),
    OperandOnly(TypedNodeId),
    EmbeddedOperand(TypedNodeId),
    SignatureOnly,
    StaticShapeOnly {
        shape: ShapeId,
        continuation: SourceNodeId,
    },
    RepresentedByShape(ShapeId),
    AliasOfShape {
        shape: ShapeId,
        binding: SourceNodeId,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OriginQueryError {
    ForeignInstance,
    ForeignSource,
    WrongFunction,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceRepresentation<'a> {
    Represented(&'a [SourceDisposition]),
    OutsideCoverage,
}

/// Private pairing prevents attaching a valid map to modified IR. Whole clones
/// retain the domain; independent lowering receives a fresh domain. Ordinals
/// describe final semantic preorder, never a cross-edit identity.
#[derive(Clone, Debug)]
pub struct CopyNodeOriginProgram {
    functions: FunctionOriginProgram,
    nodes: Vec<TypedNodeRecord>,
    relations: Vec<Vec<Relation>>,
    shapes: Vec<ShapeRecord>,
    dispositions: HashMap<(usize, usize), Vec<SourceDisposition>>,
}
impl CopyNodeOriginProgram {
    pub fn coverage(&self) -> Coverage {
        Coverage::CopyFunctionBodies
    }
    pub fn function_origins(&self) -> &FunctionOriginProgram {
        &self.functions
    }
    pub fn sources(&self) -> &SourceProgram {
        self.functions.sources()
    }
    /// All source occurrences, including erased bindings and annotations.
    /// IDs belong to this immutable snapshot; declaration nodes can be outside coverage.
    pub fn source_nodes(
        &self,
    ) -> impl ExactSizeIterator<Item = (SourceNodeId, &ResolvedNode)> + '_ {
        self.sources()
            .nodes()
            .iter()
            .enumerate()
            .map(|(slot, node)| {
                (
                    SourceNodeId {
                        domain: self.functions.domain.clone(),
                        slot,
                    },
                    node,
                )
            })
    }
    pub fn typed(&self) -> &TypedMain {
        self.functions.typed()
    }
    pub fn into_typed(self) -> TypedMain {
        self.functions.into_typed()
    }
    pub fn into_function_origins(self) -> FunctionOriginProgram {
        self.functions
    }
    pub fn nodes(&self) -> &[TypedNodeRecord] {
        &self.nodes
    }
    pub fn node(&self, id: &TypedNodeId) -> Option<&TypedNodeRecord> {
        (id.domain == self.functions.domain)
            .then(|| self.nodes.get(id.slot))
            .flatten()
    }
    pub fn shapes(&self) -> &[ShapeRecord] {
        &self.shapes
    }
    pub fn shape(&self, id: &ShapeId) -> Option<&ShapeRecord> {
        (id.domain == self.functions.domain)
            .then(|| self.shapes.get(id.slot))
            .flatten()
    }
    pub fn relations(&self, id: &RelationSetId) -> Option<&[Relation]> {
        (id.domain == self.functions.domain)
            .then(|| self.relations.get(id.slot).map(Vec::as_slice))
            .flatten()
    }
    pub fn children(&self, id: &TypedNodeId) -> Option<&[TypedNodeId]> {
        Some(&self.node(id)?.children)
    }
    /// Direct source-derived origins preserve every parser/module contribution.
    /// Generated nodes deliberately return no direct span.
    pub fn direct_source(&self, id: &TypedNodeId) -> Option<&ResolvedNode> {
        match &self.node(id)?.origin {
            NodeOrigin::SourceDerived { source, .. } => self.functions.source_node(source),
            NodeOrigin::Generated { .. } => None,
        }
    }
    pub fn source_representation(
        &self,
        instance: &FunctionInstanceId,
        source: &SourceNodeId,
    ) -> Result<SourceRepresentation<'_>, OriginQueryError> {
        let function = self
            .functions
            .instance(instance)
            .ok_or(OriginQueryError::ForeignInstance)?;
        let source_node = self
            .functions
            .source_node(source)
            .ok_or(OriginQueryError::ForeignSource)?;
        let AstDeclaration::Function(index) = source_node.path.declaration else {
            return Ok(SourceRepresentation::OutsideCoverage);
        };
        let definition = self
            .functions
            .definition(&function.definition)
            .ok_or(OriginQueryError::ForeignInstance)?;
        if definition.target.declaration != AstDeclaration::Function(index) {
            return Err(OriginQueryError::WrongFunction);
        }
        Ok(self
            .dispositions
            .get(&(instance.slot, source.slot))
            .map_or(SourceRepresentation::OutsideCoverage, |v| {
                SourceRepresentation::Represented(v)
            }))
    }
    /// Resolve through the validated parent links. No IR pointer is retained.
    pub fn typed_node(&self, id: &TypedNodeId) -> Option<TypedNodeRef<'_>> {
        let mut record = self.node(id)?;
        let mut path = Vec::new();
        while let Some(parent) = &record.parent {
            path.push(&record.role);
            record = self.node(parent)?;
        }
        let function = self.typed().functions.get(record.instance.slot)?;
        let mut node = TypedNodeRef::Function {
            params: &function.params,
            result: &function.result,
            body: &function.body,
        };
        for role in path.into_iter().rev() {
            node = structure::child(node, role)?;
        }
        Some(node)
    }
    /// Caller-bounded explanation, independent of compilation budgets.
    /// Returns at most `max_nodes` ancestors from this node toward its root.
    pub fn ancestors(&self, id: &TypedNodeId, max_nodes: usize) -> Option<Vec<TypedNodeId>> {
        let mut result = Vec::new();
        let mut current = Some(self.node(id)?);
        while let Some(record) = current {
            if result.len() == max_nodes {
                break;
            }
            result.push(record.id.clone());
            current = record.parent.as_ref().and_then(|p| self.node(p));
        }
        Some(result)
    }
}

/// Logical retained-record, edge and cumulative-work budgets. The work counter
/// is shared with function discovery and finalization, never reset per pass.
#[derive(Clone)]
pub(super) struct BodyBudget {
    records: usize,
    edges: usize,
    work: Rc<Cell<usize>>,
    failed: Rc<Cell<bool>>,
}
impl Default for BodyBudget {
    fn default() -> Self {
        Self::with_limits(1_000_000, 4_000_000, 8_000_000)
    }
}
impl BodyBudget {
    fn with_limits(records: usize, edges: usize, work: usize) -> Self {
        Self {
            records,
            edges,
            work: Rc::new(Cell::new(work)),
            failed: Rc::new(Cell::new(false)),
        }
    }
    fn records(&mut self, n: usize) -> Result<(), String> {
        self.records = self.records.checked_sub(n).ok_or_else(|| {
            self.failed.set(true);
            "E-TYPED-SOURCE-MAP-LIMIT: Copy body record budget exceeded".to_string()
        })?;
        Ok(())
    }
    fn edges(&mut self, n: usize) -> Result<(), String> {
        self.edges = self.edges.checked_sub(n).ok_or_else(|| {
            self.failed.set(true);
            "E-TYPED-SOURCE-MAP-LIMIT: Copy body edge budget exceeded".to_string()
        })?;
        Ok(())
    }
    fn work(&mut self, n: usize) -> Result<(), String> {
        let remaining = self.work.get().checked_sub(n).ok_or_else(|| {
            self.failed.set(true);
            "E-TYPED-SOURCE-MAP-LIMIT: combined origin work budget exceeded".to_string()
        })?;
        self.work.set(remaining);
        Ok(())
    }
}

/// One analysis, specialization, semantic lowering and ordinary verification.
/// Only this opt-in API excludes *all checked* owner obligations, including
/// unreachable declarations. Native mapping remains explicitly unsupported.
pub fn lower_main_with_copy_node_origins(
    sources: SourceProgram,
) -> Result<CopyNodeOriginProgram, String> {
    lower_with_budget(sources, BodyBudget::default())
}
fn lower_with_budget(
    sources: SourceProgram,
    budget: BodyBudget,
) -> Result<CopyNodeOriginProgram, String> {
    let checked = check::analyze(sources.program())?;
    let native = !sources.program().externs.is_empty()
        || !sources.program().constants.is_empty()
        || !sources.program().native.includes.is_empty()
        || checked.has_c_values();
    let owner = checked.has_owned();
    let mut requests =
        instances::Trace::with_shared_work(budget.work.clone(), budget.failed.clone());
    let mut trace = Trace::new(&sources, budget);
    let (typed, instances) = crate::typed::lower_main_traced(
        &checked,
        Some(&mut requests),
        (!native && !owner).then_some(&mut trace),
    )?;
    if native || super::has_native_representation(&typed) {
        return Err(
            "E-TYPED-UNSUPPORTED: Copy node origin index does not map native programs".into(),
        );
    }
    if owner {
        return Err(
            "E-TYPED-UNSUPPORTED: Copy node origin index excludes checked owner obligations".into(),
        );
    }
    check_traces(&requests, &trace)?;
    let data =
        super::finalize_function_data(&sources, &checked, &typed, &instances, &mut requests)?;
    let metadata = finalize(&sources, &checked, &typed, &instances, &data, &mut trace)?;
    drop(trace);
    drop(checked);
    let functions = data.into_program(sources, typed);
    Ok(CopyNodeOriginProgram {
        functions,
        nodes: metadata.nodes,
        relations: metadata.relations,
        shapes: metadata.shapes,
        dispositions: metadata.dispositions,
    })
}

fn check_traces(requests: &instances::Trace, trace: &Trace<'_>) -> Result<(), String> {
    // The shared boolean is only a stop signal; preserve the observer's actual
    // diagnostic rather than turning a correspondence error into a limit error.
    if let Some(error) = requests.failure.as_ref().or(trace.failure.as_ref()) {
        return Err(error.clone());
    }
    requests.check()?;
    trace.check()
}

struct Metadata {
    nodes: Vec<TypedNodeRecord>,
    relations: Vec<Vec<Relation>>,
    shapes: Vec<ShapeRecord>,
    dispositions: HashMap<(usize, usize), Vec<SourceDisposition>>,
}
fn finalize(
    sources: &SourceProgram,
    checked: &CheckedProgram<'_>,
    typed: &TypedMain,
    instances: &instances::Instances,
    data: &super::FunctionOriginData,
    trace: &mut Trace<'_>,
) -> Result<Metadata, String> {
    use trace::RawOrigin;
    let actual = structure::enumerate(typed, &mut trace.budget)?;
    if trace.roots.len() != typed.functions.len() {
        return Err(fail("missing/extra body root fragments"));
    }
    trace
        .budget
        .records(actual.len() + 2 * trace.fragments.len())?;
    trace
        .budget
        .records(actual.len() + trace.shape_bindings.len() + data.requests.len())?;
    let mut next_child = vec![0; actual.len()];
    trace.binding_sources.clear();
    for (index, binding) in trace.shape_bindings.iter().enumerate() {
        trace.budget.work(1)?;
        if trace
            .binding_sources
            .insert((binding.instance, binding.source), index)
            .is_some()
        {
            return Err(fail("duplicate shape binding source"));
        }
    }
    let mut request_index = HashMap::new();
    for request in &data.requests {
        trace.budget.work(1)?;
        if let RequestOrigin::Call { caller, site } = &request.origin {
            if request_index
                .insert((caller.slot, site.slot), request.callee.slot)
                .is_some()
            {
                return Err(fail("duplicate function request source"));
            }
        }
    }
    let mut actual_fragments = Vec::with_capacity(actual.len());
    let mut fragment_slots = vec![None; trace.fragments.len()];
    let mut fragment_nodes = vec![None; trace.fragments.len()];
    let mut types =
        instances::Trace::with_shared_work(trace.budget.work.clone(), trace.budget.failed.clone());
    for (slot, occurrence) in actual.iter().enumerate() {
        trace.budget.work(1)?;
        let fragment = if let Some(parent) = occurrence.parent {
            let &parent_fragment = actual_fragments
                .get(parent)
                .ok_or_else(|| fail("body parent is not earlier in preorder"))?;
            let parent_record: &trace::Fragment = trace
                .fragments
                .get(parent_fragment)
                .ok_or_else(|| fail("invalid parent fragment"))?;
            trace.budget.work(1)?;
            let (role, fragment) = parent_record
                .children
                .get(next_child[parent])
                .ok_or_else(|| fail("missing ordered body child fragment"))?;
            next_child[parent] += 1;
            if *role != occurrence.role {
                return Err(fail("ordered body child role differs from actual IR"));
            }
            *fragment
        } else {
            trace.roots[occurrence.instance]
        };
        let record = trace
            .fragments
            .get(fragment)
            .ok_or_else(|| fail("body fragment reference out of bounds"))?;
        if record.instance != occurrence.instance
            || fragment_slots[fragment].replace(slot).is_some()
        {
            return Err(fail("shared/foreign body output fragment"));
        }
        let fingerprint = structure::fingerprint(occurrence.node, &mut trace.budget)?;
        if fingerprint != record.fingerprint {
            return Err(fail(format!(
                "construction attributes differ at typed slot {slot}"
            )));
        }
        // Fingerprints are temporary here, not another retained copy.
        trace.budget.records += 1;
        if occurrence.node.children().len() != record.children.len() {
            return Err(fail("missing/extra output child fragment"));
        }
        trace.budget.work(record.children.len())?;
        if !occurrence
            .node
            .children()
            .map(|(role, _)| role)
            .eq(record.children.iter().map(|(role, _)| *role))
        {
            return Err(fail(
                "output child order differs from semantic stored order",
            ));
        }
        if let RawOrigin::Source { source, rule } = &record.origin {
            if trace.source_index.function(*source)
                != Some(instances.list[occurrence.instance].source)
            {
                return Err(fail("source fragment belongs to another function"));
            }
            validate::direct(
                trace
                    .source_index
                    .node(*source)
                    .ok_or_else(|| fail("invalid source fragment"))?,
                *rule,
                occurrence.node,
                occurrence.instance,
                *source,
                checked,
                typed,
                instances,
                &request_index,
                &mut trace.budget,
                &mut types,
            )?;
        }
        actual_fragments.push(fragment);
        fragment_nodes[fragment] = Some(occurrence.node);
    }
    if fragment_slots.iter().any(Option::is_none) {
        return Err(fail("unreachable extra construction fragment"));
    }
    validate::routing(trace, instances)?;
    let lexical = validate::lexical(&trace.source_index, instances, &mut trace.budget)?;
    validate::bindings(trace, &lexical, &fragment_nodes)?;
    for receipt in &trace.uses {
        let Some(AstNode::Expression(expr)) = trace.source_index.node(receipt.source) else {
            return Err(fail("shape use lacks source expression"));
        };
        let instance = instances
            .list
            .get(receipt.instance)
            .ok_or_else(|| fail("shape use has invalid instance"))?;
        let expected = super::checked_type(checked, expr, &instance.substitution, &mut types)?;
        if !super::signature_matches(&expected, receipt.target, typed, &mut types)? {
            return Err(fail("shape use target differs from checked instance type"));
        }
    }
    trace.validate_shape_receipts(typed, checked, |fragment| {
        fragment_nodes.get(fragment).copied().flatten()
    })?;
    validate_generated(trace, &fragment_nodes, instances)?;
    validate_dispositions(trace, instances, &fragment_nodes)?;
    trace.check()?;
    let domain = &data.domain;
    let node_id = |slot| TypedNodeId {
        domain: domain.clone(),
        slot,
    };
    let source_id = |slot| SourceNodeId {
        domain: domain.clone(),
        slot,
    };
    let shape_id = |slot| ShapeId {
        domain: domain.clone(),
        slot,
    };
    let mut nodes = Vec::new();
    let mut relations = Vec::new();
    for (slot, occurrence) in actual.iter().enumerate() {
        trace.budget.records(1)?;
        trace.budget.work(1)?;
        let fragment = &trace.fragments[actual_fragments[slot]];
        let origin = match &fragment.origin {
            RawOrigin::Source { source, rule } => NodeOrigin::SourceDerived {
                source: source_id(*source),
                rule: *rule,
            },
            RawOrigin::Generated {
                reason,
                relations: raw,
            } => {
                trace.budget.records(1)?;
                trace.budget.edges(raw.len())?;
                trace.budget.work(raw.len())?;
                let mut values = Vec::with_capacity(raw.len());
                for relation in raw {
                    let target = match relation.target {
                        RawTarget::Source(source) => {
                            if sources.nodes().get(source).is_none() {
                                return Err(fail("invalid related source"));
                            }
                            RelationTarget::Source(source_id(source))
                        }
                        RawTarget::Fragment(fragment) => RelationTarget::Typed(node_id(
                            fragment_slots
                                .get(fragment)
                                .copied()
                                .flatten()
                                .ok_or_else(|| fail("invalid related output fragment"))?,
                        )),
                        RawTarget::Shape(shape) => {
                            if trace.shapes.get(shape).is_none() {
                                return Err(fail("invalid related shape"));
                            }
                            RelationTarget::Shape(shape_id(shape))
                        }
                    };
                    values.push(Relation {
                        role: relation.role,
                        target,
                    });
                }
                let related = RelationSetId {
                    domain: domain.clone(),
                    slot: relations.len(),
                };
                relations.push(values);
                NodeOrigin::Generated {
                    reason: *reason,
                    related,
                }
            }
        };
        trace.budget.edges(fragment.children.len())?;
        trace.budget.work(fragment.children.len())?;
        let children = fragment
            .children
            .iter()
            .map(|(_, child)| node_id(fragment_slots[*child].expect("validated child")))
            .collect();
        nodes.push(TypedNodeRecord {
            id: node_id(slot),
            instance: FunctionInstanceId {
                domain: domain.clone(),
                slot: occurrence.instance,
            },
            parent: occurrence.parent.map(node_id),
            role: occurrence.role,
            kind: occurrence.node.kind(),
            origin,
            children,
        });
    }
    let shapes = trace.public_shapes(domain, &fragment_slots)?;
    let mut dispositions: HashMap<(usize, usize), Vec<SourceDisposition>> = HashMap::new();
    for receipt in &trace.dispositions {
        trace.budget.records(1)?;
        trace.budget.work(1)?;
        let representation = match receipt.disposition {
            RawDisposition::Output(fragment) => SourceDisposition::Output(node_id(
                fragment_slots[fragment].expect("validated output"),
            )),
            RawDisposition::OperandOnly(fragment) => SourceDisposition::OperandOnly(node_id(
                fragment_slots[fragment].expect("validated argument"),
            )),
            RawDisposition::EmbeddedOperand(fragment) => SourceDisposition::EmbeddedOperand(
                node_id(fragment_slots[fragment].expect("validated operand")),
            ),
            RawDisposition::SignatureOnly => SourceDisposition::SignatureOnly,
            RawDisposition::StaticShapeOnly {
                shape,
                continuation,
            } => SourceDisposition::StaticShapeOnly {
                shape: shape_id(shape),
                continuation: source_id(continuation),
            },
            RawDisposition::RepresentedByShape(shape) => {
                SourceDisposition::RepresentedByShape(shape_id(shape))
            }
            RawDisposition::AliasOfShape { shape, binding } => SourceDisposition::AliasOfShape {
                shape: shape_id(shape),
                binding: source_id(binding),
            },
        };
        dispositions
            .entry((receipt.instance, receipt.source))
            .or_default()
            .push(representation);
    }
    Ok(Metadata {
        nodes,
        relations,
        shapes,
        dispositions,
    })
}

fn validate_generated(
    trace: &mut Trace<'_>,
    actual: &[Option<TypedNodeRef<'_>>],
    instances: &instances::Instances,
) -> Result<(), String> {
    use trace::RawOrigin;
    for (index, fragment) in trace.fragments.iter().enumerate() {
        let RawOrigin::Generated { reason, relations } = &fragment.origin else {
            continue;
        };
        trace.budget.work(1 + relations.len())?;
        trace.budget.records(relations.len())?;
        let mut seen: Vec<&RawRelation> = Vec::new();
        for relation in relations {
            trace.budget.work(seen.len())?;
            if seen.contains(&relation) {
                return Err(fail("duplicate generated relation"));
            }
            seen.push(relation);
            match relation.target {
                RawTarget::Source(source)
                    if trace.source_index.function(source)
                        == Some(instances.list[fragment.instance].source) => {}
                RawTarget::Fragment(other)
                    if trace
                        .fragments
                        .get(other)
                        .is_some_and(|r| r.instance == fragment.instance) => {}
                RawTarget::Shape(shape)
                    if trace
                        .shapes
                        .get(shape)
                        .is_some_and(|r| r.instance == fragment.instance) => {}
                _ => {
                    return Err(fail(
                        "generated relation outside its instance/source function",
                    ))
                }
            }
        }
        match reason {
            GenerationReason::InferredResultType => {
                if !matches!(actual[index], Some(TypedNodeRef::Result(_))) || relations.len() != 2 {
                    return Err(fail(
                        "inferred result slot requires exact generated definition/body relations",
                    ));
                }
                let root = trace
                    .source_index
                    .root(AstDeclaration::Function(
                        instances.list[fragment.instance].source,
                    ))
                    .ok_or_else(|| fail("missing generated signature source"))?;
                let body = trace
                    .source_index
                    .child(root, crate::source_map::AstStep::FunctionBody)
                    .ok_or_else(|| fail("missing generated signature body"))?;
                if relations[0]
                    != (RawRelation {
                        role: RelationRole::Definition,
                        target: RawTarget::Source(root),
                    })
                    || relations[1]
                        != (RawRelation {
                            role: RelationRole::Body,
                            target: RawTarget::Source(body),
                        })
                {
                    return Err(fail("inferred result has wrong source relationships"));
                }
            }
            GenerationReason::ReadArgumentTemporary => {
                if !matches!(
                    actual[index],
                    Some(TypedNodeRef::Argument(
                        crate::typed::CallArg::ReadTemporary(_)
                    ))
                ) || relations.len() != 1
                    || relations[0].role != RelationRole::CausedByUse
                {
                    return Err(fail("invalid generated read argument slot"));
                }
                let RawTarget::Source(source) = relations[0].target else {
                    return Err(fail("read argument has no exact source operand"));
                };
                let Some((TypedChildRole::ArgumentValue, child)) = fragment.children.first() else {
                    return Err(fail("read temporary lacks its value child"));
                };
                let child = *child;
                // Validated below without holding the fragment borrow.
                let _ = (source, child);
            }
            GenerationReason::CaptureEnvironmentBinding
            | GenerationReason::CaptureProduct
            | GenerationReason::CaptureProductField
            | GenerationReason::ShapeMaterialization
            | GenerationReason::ShapeRecordField
            | GenerationReason::CaptureProjection
            | GenerationReason::ProjectedCaptureTemporary => {
                // Shape validator checks exact actual child identities and each
                // rule-specific source/shape/capture relation independently.
                if relations.is_empty() {
                    return Err(fail("generated shape occurrence lacks relationships"));
                }
            }
        }
    }
    for index in 0..trace.fragments.len() {
        if let trace::RawOrigin::Generated {
            reason: GenerationReason::ReadArgumentTemporary,
            relations,
        } = &trace.fragments[index].origin
        {
            let RawTarget::Source(source) = relations[0].target else {
                unreachable!()
            };
            let child = trace.fragments[index].children[0].1;
            let instance = trace.fragments[index].instance;
            validate::route(trace, instance, source, child)?;
        }
    }
    Ok(())
}

fn validate_dispositions(
    trace: &mut Trace<'_>,
    instances: &instances::Instances,
    actual: &[Option<TypedNodeRef<'_>>],
) -> Result<(), String> {
    use crate::source_map::{AstNodeKind, AstStep};
    let mut represented: HashMap<(usize, usize), Vec<&RawDisposition>> = HashMap::new();
    for receipt in &trace.dispositions {
        trace.budget.records(1)?;
        trace.budget.work(1)?;
        let definition = instances
            .list
            .get(receipt.instance)
            .ok_or_else(|| fail("disposition has invalid instance"))?;
        if trace.source_index.function(receipt.source) != Some(definition.source) {
            return Err(fail("disposition belongs to another source function"));
        }
        let source = trace
            .source_index
            .node(receipt.source)
            .ok_or_else(|| fail("disposition has invalid source"))?;
        match receipt.disposition {
            RawDisposition::Output(fragment)
            | RawDisposition::OperandOnly(fragment)
            | RawDisposition::EmbeddedOperand(fragment) => {
                if trace
                    .fragments
                    .get(fragment)
                    .is_none_or(|f| f.instance != receipt.instance)
                {
                    return Err(fail("disposition references foreign/missing output"));
                }
                let origin = &trace.fragments[fragment].origin;
                let has_anchor = match origin {
                    trace::RawOrigin::Source { source, .. } => *source == receipt.source,
                    trace::RawOrigin::Generated { relations, .. } => {
                        trace.budget.work(relations.len())?;
                        relations.iter().any(|r| {
                            matches!(r.target,RawTarget::Source(source) if source==receipt.source)
                                && matches!(
                                    r.role,
                                    RelationRole::CausedByUse | RelationRole::IntroducedForBinding
                                )
                        })
                    }
                };
                if !has_anchor {
                    return Err(fail("output disposition has another source anchor"));
                }
                match (
                    receipt.disposition.clone(),
                    actual.get(fragment).copied().flatten(),
                ) {
                    (RawDisposition::OperandOnly(_), Some(TypedNodeRef::Argument(_)))
                    | (RawDisposition::EmbeddedOperand(_), Some(TypedNodeRef::Capacity(_)))
                    | (RawDisposition::Output(_), Some(_)) => {}
                    _ => return Err(fail("disposition references wrong output category")),
                }
            }
            RawDisposition::SignatureOnly => {
                if source.kind() != AstNodeKind::TypeExpression {
                    return Err(fail("signature-only source is not an annotation"));
                }
            }
            RawDisposition::StaticShapeOnly {
                shape,
                continuation,
            } => {
                trace.budget.work(1)?;
                if trace.shapes.get(shape).is_none()
                    || trace.source_index.child(receipt.source, AstStep::LetBody)
                        != Some(continuation)
                    || !trace
                        .binding_sources
                        .get(&(receipt.instance, receipt.source))
                        .and_then(|&i| trace.shape_bindings.get(i))
                        .is_some_and(|b| {
                            b.shape == shape
                                && b.environment.is_none()
                                && b.continuation == continuation
                        })
                {
                    return Err(fail("invalid erased source let disposition"));
                }
            }
            RawDisposition::RepresentedByShape(shape) => {
                if trace.shapes.get(shape).is_none_or(|shape| {
                    shape.instance != receipt.instance || shape.source != receipt.source
                }) {
                    return Err(fail("invalid shape source disposition"));
                }
            }
            RawDisposition::AliasOfShape { shape, binding } => {
                trace.budget.work(1)?;
                if trace.shapes.get(shape).is_none()
                    || !trace
                        .binding_sources
                        .get(&(receipt.instance, binding))
                        .and_then(|&i| trace.shape_bindings.get(i))
                        .is_some_and(|b| b.shape == shape)
                {
                    return Err(fail("invalid alias disposition"));
                }
            }
        }
        let list = represented
            .entry((receipt.instance, receipt.source))
            .or_default();
        trace.budget.work(list.len())?;
        if list.contains(&&receipt.disposition) {
            return Err(fail("duplicate source disposition"));
        }
        list.push(&receipt.disposition);
    }
    for (instance, definition) in instances.list.iter().enumerate() {
        for source in trace
            .source_index
            .function_range(definition.source)
            .ok_or_else(|| fail("missing source function range"))?
        {
            trace.budget.work(1)?;
            if !represented.contains_key(&(instance, source)) {
                return Err(fail(format!(
                    "missing source disposition at instance {instance}, source {source}"
                )));
            }
        }
    }
    Ok(())
}
