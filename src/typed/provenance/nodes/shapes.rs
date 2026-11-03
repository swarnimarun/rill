//! Construction receipts for the shared, private local-value shape DAG.
//! Checker identities remain private and instance-qualified. Public shape
//! handles describe initializer structure, never addressable typed storage.
use super::*;
use crate::source_map::AstStep;
use crate::typed::{AdtOrigin, ExprKind, FieldPlace, FieldStep, LocalId, Type as IrType};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShapeChild {
    pub source: SourceNodeId,
    pub shape: ShapeId,
    pub written: usize,
    pub declaration: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    Capture {
        evaluation: TypedNodeId,
    },
    Constructor {
        definition: SourceNodeId,
        variant: usize,
        fields: Vec<ShapeChild>,
    },
    Record {
        definition: SourceNodeId,
        fields: Vec<ShapeChild>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShapeRecord {
    pub id: ShapeId,
    pub instance: FunctionInstanceId,
    pub source: SourceNodeId,
    pub kind: ShapeKind,
}
#[derive(Clone, Debug)]
pub(in crate::typed) struct ShapeEdgeReceipt {
    pub source: usize,
    pub shape: usize,
    pub written: usize,
    pub declaration: usize,
}
#[derive(Clone, Debug)]
pub(in crate::typed) enum ShapeReceiptKind {
    Capture {
        capture: usize,
    },
    Constructor {
        definition: usize,
        variant: usize,
        fields: Vec<ShapeEdgeReceipt>,
    },
    Record {
        definition: usize,
        fields: Vec<ShapeEdgeReceipt>,
    },
}
#[derive(Clone, Debug)]
pub(super) struct ShapeReceipt {
    pub instance: usize,
    pub source: usize,
    pub kind: ShapeReceiptKind,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct CaptureEnvironment {
    pub binding: usize,
    pub root: LocalId,
    pub adt: usize,
    pub field: usize,
}
#[derive(Clone, Debug)]
pub(super) struct CaptureReceipt {
    pub instance: usize,
    pub source: usize,
    pub bookkeeping: LocalId,
    pub ty: IrType,
    pub evaluation: usize,
    pub environment: Option<CaptureEnvironment>,
}
#[derive(Clone, Debug)]
pub(super) struct ShapeBindingReceipt {
    pub instance: usize,
    pub source: usize,
    pub initializer: usize,
    pub continuation: usize,
    pub checked_identity: usize,
    pub shape: usize,
    pub captures: Vec<usize>,
    pub environment: Option<(LocalId, usize)>,
    pub body: Option<usize>,
    pub output: Option<usize>,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct ShapeProjectionStep {
    pub definition: usize,
    pub field: usize,
    pub arity: usize,
}
#[derive(Clone, Debug)]
pub(super) struct MaterializationReceipt {
    pub shape: usize,
    pub target: IrType,
    pub output: usize,
    pub children: Vec<usize>,
}
#[derive(Clone, Debug)]
pub(super) struct ProjectedCaptureReceipt {
    pub shape: usize,
    pub offset: usize,
    pub root: LocalId,
    pub path: Vec<FieldStep>,
    pub capture: usize,
    pub read: usize,
    pub output: usize,
}
#[derive(Clone, Debug)]
pub(super) struct ShapeUseReceipt {
    pub instance: usize,
    pub source: usize,
    pub checked_identity: usize,
    pub binding: usize,
    pub shape: usize,
    pub target: IrType,
    pub projection: Vec<ShapeProjectionStep>,
    pub selected: Option<usize>,
    pub output: Option<usize>,
    pub materializations: Vec<MaterializationReceipt>,
    pub temporary: Option<ProjectedCaptureReceipt>,
}

impl trace::Trace<'_> {
    pub(in crate::typed) fn shape_edge(
        &mut self,
        edges: &mut Vec<ShapeEdgeReceipt>,
        source: Option<usize>,
        shape: Option<usize>,
        written: usize,
        declaration: usize,
    ) {
        self.observe(|this| {
            let (source, shape) = (
                source.ok_or_else(|| fail("missing shape child source"))?,
                shape.ok_or_else(|| fail("missing shape child"))?,
            );
            this.budget.edges(1)?;
            this.budget.work(1)?;
            edges.push(ShapeEdgeReceipt {
                source,
                shape,
                written,
                declaration,
            });
            Ok(())
        });
    }
    pub(in crate::typed) fn shape_node(
        &mut self,
        source: Option<usize>,
        kind: ShapeReceiptKind,
    ) -> Option<usize> {
        self.observe(|this| {
            let source = source.ok_or_else(|| fail("missing shape source"))?;
            this.budget.records(1)?;
            this.budget.work(1)?;
            let id = this.shapes.len();
            this.shapes.push(ShapeReceipt {
                instance: this.instance,
                source,
                kind,
            });
            Ok(id)
        })
    }
    pub(in crate::typed) fn shape_capture(
        &mut self,
        source: Option<usize>,
        bookkeeping: LocalId,
        ty: IrType,
        evaluation: Option<usize>,
    ) -> Option<usize> {
        self.observe(|this| {
            let source = source.ok_or_else(|| fail("missing capture source"))?;
            let evaluation = evaluation.ok_or_else(|| fail("missing capture evaluation"))?;
            this.budget.records(1)?;
            this.budget.work(1)?;
            let id = this.captures.len();
            this.captures.push(CaptureReceipt {
                instance: this.instance,
                source,
                bookkeeping,
                ty,
                evaluation,
                environment: None,
            });
            Ok(id)
        })
    }
    pub(in crate::typed) fn shape_alias(
        &mut self,
        source: Option<usize>,
        identity: Option<usize>,
        shape: Option<usize>,
    ) {
        let selected = self.observe(|this| {
            let source = source.ok_or_else(|| fail("missing alias source"))?;
            let identity = identity.ok_or_else(|| fail("missing alias identity"))?;
            let shape = shape.ok_or_else(|| fail("missing alias shape"))?;
            this.budget.work(1)?;
            let binding = *this
                .shape_binding_index
                .get(&(this.instance, identity))
                .ok_or_else(|| fail("alias binding not active"))?;
            let receipt = &this.shape_bindings[binding];
            if receipt.shape != shape {
                return Err(fail("alias selected another shape"));
            }
            Ok((source, shape, receipt.source))
        });
        if let Some((source, shape, binding)) = selected {
            self.disposition(source, RawDisposition::AliasOfShape { shape, binding });
        }
    }
    // Keep independent construction facts explicit at this private observer seam.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::typed) fn shape_binding(
        &mut self,
        source: Option<usize>,
        initializer: Option<usize>,
        continuation: Option<usize>,
        identity: usize,
        shape: Option<usize>,
        captures: impl Iterator<Item = Option<usize>>,
        environment: Option<(LocalId, usize)>,
    ) -> Option<usize> {
        self.observe(|this| {
            let source = source.ok_or_else(|| fail("missing shape binding source"))?;
            let initializer =
                initializer.ok_or_else(|| fail("missing shape initializer source"))?;
            let continuation =
                continuation.ok_or_else(|| fail("missing shape continuation source"))?;
            let shape = shape.ok_or_else(|| fail("missing binding shape"))?;
            let mut leaves = Vec::new();
            for capture in captures {
                let capture = capture.ok_or_else(|| fail("missing binding capture"))?;
                this.budget.edges(1)?;
                this.budget.work(1)?;
                leaves.push(capture);
            }
            this.budget.records(2)?;
            this.budget.work(1)?;
            let id = this.shape_bindings.len();
            if this
                .shape_binding_index
                .insert((this.instance, identity), id)
                .is_some()
            {
                return Err(fail("duplicate shape binding identity"));
            }
            if let Some((root, adt)) = environment {
                for (field, &capture) in leaves.iter().enumerate() {
                    this.budget.work(1)?;
                    let capture = this
                        .captures
                        .get_mut(capture)
                        .ok_or_else(|| fail("invalid capture receipt"))?;
                    if capture.environment.is_some() || capture.instance != this.instance {
                        return Err(fail("capture belongs to another environment"));
                    }
                    capture.environment = Some(CaptureEnvironment {
                        binding: id,
                        root,
                        adt,
                        field,
                    });
                }
            }
            this.shape_bindings.push(ShapeBindingReceipt {
                instance: this.instance,
                source,
                initializer,
                continuation,
                checked_identity: identity,
                shape,
                captures: leaves,
                environment,
                body: None,
                output: None,
            });
            Ok(id)
        })
    }
    pub(in crate::typed) fn finish_shape_binding(
        &mut self,
        binding: Option<usize>,
        body: Option<usize>,
        output: Option<usize>,
    ) {
        let disposition = self.observe(|this| {
            this.budget.work(1)?;
            let binding = binding.ok_or_else(|| fail("missing shape binding receipt"))?;
            let body = body.ok_or_else(|| fail("missing shape continuation fragment"))?;
            let output = output.ok_or_else(|| fail("missing shape binding output"))?;
            let receipt = this
                .shape_bindings
                .get_mut(binding)
                .ok_or_else(|| fail("invalid shape binding receipt"))?;
            receipt.body = Some(body);
            receipt.output = Some(output);
            this.shape_binding_index
                .remove(&(receipt.instance, receipt.checked_identity));
            Ok((
                receipt.source,
                if receipt.environment.is_some() {
                    RawDisposition::Output(output)
                } else {
                    RawDisposition::StaticShapeOnly {
                        shape: receipt.shape,
                        continuation: receipt.continuation,
                    }
                },
            ))
        });
        if let Some((source, disposition)) = disposition {
            self.disposition(source, disposition);
        }
    }
    pub(in crate::typed) fn shape_use(
        &mut self,
        source: Option<usize>,
        identity: Option<usize>,
        shape: Option<usize>,
        target: IrType,
    ) -> Option<usize> {
        self.observe(|this| {
            let source = source.ok_or_else(|| fail("missing materialization source"))?;
            let identity = identity.ok_or_else(|| fail("missing materialization identity"))?;
            let shape = shape.ok_or_else(|| fail("missing materialization shape"))?;
            this.budget.work(1)?;
            let binding = *this
                .shape_binding_index
                .get(&(this.instance, identity))
                .ok_or_else(|| fail("materialization binding not active"))?;
            if this.shape_bindings[binding].shape != shape {
                return Err(fail("materialization binding shape mismatch"));
            }
            this.budget.records(1)?;
            let id = this.uses.len();
            this.uses.push(ShapeUseReceipt {
                instance: this.instance,
                source,
                checked_identity: identity,
                binding,
                shape,
                target,
                projection: Vec::new(),
                selected: None,
                output: None,
                materializations: Vec::new(),
                temporary: None,
            });
            Ok(id)
        })
    }
    pub(in crate::typed) fn shape_projection_step(
        &mut self,
        use_id: Option<usize>,
        definition: Option<usize>,
        field: usize,
        arity: usize,
    ) {
        self.observe(|this| {
            let use_id = use_id.ok_or_else(|| fail("missing projected use"))?;
            let definition = definition.ok_or_else(|| fail("missing projection definition"))?;
            this.budget.edges(1)?;
            this.budget.work(1)?;
            this.uses
                .get_mut(use_id)
                .ok_or_else(|| fail("invalid projected use"))?
                .projection
                .push(ShapeProjectionStep {
                    definition,
                    field,
                    arity,
                });
            Ok(())
        });
    }
    pub(in crate::typed) fn shape_relations(
        &mut self,
        use_id: Option<usize>,
        shape: Option<usize>,
        reads_capture: bool,
    ) -> Vec<RawRelation> {
        let mut relations = Vec::new();
        let context = self.observe(|this| {
            this.budget.work(1)?;
            let use_id = use_id.ok_or_else(|| fail("missing materialization context"))?;
            let shape = shape.ok_or_else(|| fail("missing selected shape"))?;
            let use_receipt = this
                .uses
                .get(use_id)
                .ok_or_else(|| fail("invalid materialization context"))?;
            let capture = if reads_capture {
                let ShapeReceiptKind::Capture { capture } = this
                    .shapes
                    .get(shape)
                    .ok_or_else(|| fail("invalid capture shape"))?
                    .kind
                else {
                    return Err(fail("projection does not read a capture"));
                };
                Some(
                    this.captures
                        .get(capture)
                        .ok_or_else(|| fail("invalid capture"))?
                        .evaluation,
                )
            } else {
                None
            };
            Ok((use_receipt.source, shape, capture))
        });
        if let Some((source, shape, capture)) = context {
            self.relation(
                &mut relations,
                RelationRole::CausedByUse,
                RawTarget::Source(source),
            );
            self.relation(
                &mut relations,
                RelationRole::SelectedShape,
                RawTarget::Shape(shape),
            );
            if let Some(capture) = capture {
                self.relation(
                    &mut relations,
                    RelationRole::ReadsCapture,
                    RawTarget::Fragment(capture),
                );
            }
        }
        relations
    }
    pub(in crate::typed) fn capture_relations(
        &mut self,
        binding: Option<usize>,
        only: Option<usize>,
    ) -> Vec<RawRelation> {
        let mut relations = Vec::new();
        let context = self.observe(|this| {
            this.budget.work(1)?;
            let binding = binding.ok_or_else(|| fail("missing environment binding"))?;
            let receipt = this
                .shape_bindings
                .get(binding)
                .ok_or_else(|| fail("invalid environment binding"))?;
            let fields = match only {
                Some(field) if field < receipt.captures.len() => field..field + 1,
                Some(_) => return Err(fail("invalid capture relation field")),
                None => 0..receipt.captures.len(),
            };
            Ok((binding, receipt.source, fields))
        });
        if let Some((binding, source, fields)) = context {
            self.relation(
                &mut relations,
                RelationRole::IntroducedForBinding,
                RawTarget::Source(source),
            );
            for field in fields {
                let Some(evaluation) = self.observe(|this| {
                    this.budget.work(1)?;
                    let capture = this.shape_bindings[binding].captures[field];
                    this.captures
                        .get(capture)
                        .map(|capture| capture.evaluation)
                        .ok_or_else(|| fail("invalid capture relation receipt"))
                }) else {
                    break;
                };
                self.relation(
                    &mut relations,
                    RelationRole::CapturedLeaf,
                    RawTarget::Fragment(evaluation),
                );
            }
        }
        relations
    }
    pub(in crate::typed) fn materialized_shape(
        &mut self,
        use_id: Option<usize>,
        shape: Option<usize>,
        target: IrType,
        output: Option<usize>,
        children: &[Option<usize>],
    ) {
        self.observe(|this| {
            let use_id = use_id.ok_or_else(|| fail("missing materialization use"))?;
            let shape = shape.ok_or_else(|| fail("missing materialization shape"))?;
            let output = output.ok_or_else(|| fail("missing materialization output"))?;
            let mut child_ids = Vec::new();
            for &child in children {
                let child = child.ok_or_else(|| fail("missing materialization child"))?;
                this.budget.edges(1)?;
                this.budget.work(1)?;
                child_ids.push(child);
            }
            this.budget.records(1)?;
            this.budget.work(1)?;
            this.uses
                .get_mut(use_id)
                .ok_or_else(|| fail("invalid materialization use"))?
                .materializations
                .push(MaterializationReceipt {
                    shape,
                    target,
                    output,
                    children: child_ids,
                });
            Ok(())
        });
    }
    pub(in crate::typed) fn finish_shape_use(
        &mut self,
        use_id: Option<usize>,
        selected: Option<usize>,
        output: Option<usize>,
    ) {
        let disposition = self.observe(|this| {
            let use_id = use_id.ok_or_else(|| fail("missing completed use"))?;
            let selected = selected.ok_or_else(|| fail("missing selected shape"))?;
            let output = output.ok_or_else(|| fail("missing completed use output"))?;
            this.budget.work(1)?;
            let receipt = this
                .uses
                .get_mut(use_id)
                .ok_or_else(|| fail("invalid completed use"))?;
            receipt.selected = Some(selected);
            receipt.output = Some(output);
            Ok((receipt.source, output))
        });
        if let Some((source, output)) = disposition {
            self.disposition(source, RawDisposition::Output(output));
        }
    }
    // Source path, capture slot and actual output are distinct correspondence facts.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::typed) fn projected_capture(
        &mut self,
        use_id: Option<usize>,
        shape: Option<usize>,
        offset: usize,
        place: &FieldPlace,
        capture: Option<usize>,
        read: Option<usize>,
        output: Option<usize>,
    ) {
        self.observe(|this| {
            let use_id = use_id.ok_or_else(|| fail("missing temporary use"))?;
            let shape = shape.ok_or_else(|| fail("missing temporary shape"))?;
            let capture = capture.ok_or_else(|| fail("missing temporary capture"))?;
            let read = read.ok_or_else(|| fail("missing temporary read"))?;
            let output = output.ok_or_else(|| fail("missing temporary output"))?;
            this.budget.records(1)?;
            this.budget.edges(place.path.len())?;
            this.budget.work(place.path.len() + 1)?;
            let temporary = ProjectedCaptureReceipt {
                shape,
                offset,
                root: place.root,
                path: place.path.clone(),
                capture,
                read,
                output,
            };
            let receipt = this
                .uses
                .get_mut(use_id)
                .ok_or_else(|| fail("invalid temporary use"))?;
            if receipt.temporary.replace(temporary).is_some() {
                return Err(fail("duplicate projected temporary"));
            }
            Ok(())
        });
    }
}

impl trace::Trace<'_> {
    pub(in crate::typed) fn shape_output_edge(
        &mut self,
        children: &mut Vec<Option<usize>>,
        child: Option<usize>,
    ) {
        self.observe(|this| {
            let child = child.ok_or_else(|| fail("missing shape output child"))?;
            this.budget.edges(1)?;
            this.budget.work(1)?;
            children.push(Some(child));
            Ok(())
        });
    }
}

fn expression<'a>(node: Option<TypedNodeRef<'a>>) -> Result<&'a ExprNode, String> {
    match node {
        Some(TypedNodeRef::Expression(value)) => Ok(value),
        _ => Err(fail("shape fragment is not an expression")),
    }
}
fn same_expression<'a>(
    node: Option<TypedNodeRef<'a>>,
    expected: &'a ExprNode,
) -> Result<(), String> {
    if std::ptr::eq(expression(node)?, expected) {
        Ok(())
    } else {
        Err(fail("shape child fragment names another occurrence"))
    }
}
fn generated_matches(
    budget: &mut BodyBudget,
    covered: &mut [bool],
    fragments: &[trace::Fragment],
    fragment: usize,
    instance: usize,
    reason: GenerationReason,
    expected: impl Iterator<Item = (RelationRole, RawTarget)>,
) -> Result<(), String> {
    budget.work(1)?;
    let seen = covered
        .get_mut(fragment)
        .ok_or_else(|| fail("shape generated fragment outside coverage table"))?;
    if std::mem::replace(seen, true) {
        return Err(fail("shape generated occurrence claimed more than once"));
    }
    let fragment = fragments
        .get(fragment)
        .ok_or_else(|| fail("shape fragment outside trace"))?;
    if fragment.instance != instance {
        return Err(fail("shape fragment belongs to another instance"));
    }
    let trace::RawOrigin::Generated {
        reason: actual,
        relations,
    } = &fragment.origin
    else {
        return Err(fail("generated shape operation has a direct source"));
    };
    if *actual != reason {
        return Err(fail("shape generation rule mismatch"));
    }
    let mut expected = expected;
    for relation in relations {
        budget.work(1)?;
        let Some((role, target)) = expected.next() else {
            return Err(fail("extra shape cause relation"));
        };
        if relation.role != role || relation.target != target {
            return Err(fail("shape cause relation mismatch"));
        }
    }
    if expected.next().is_some() {
        return Err(fail("missing shape cause relation"));
    }
    Ok(())
}

impl trace::Trace<'_> {
    /// Checks receipts against source grammar, still-borrowed checker facts,
    /// final registry types and an independently enumerated final IR mapping.
    /// No origin is inferred, repaired, or paired by expression-value equality.
    pub(super) fn validate_shape_receipts<'ir>(
        &mut self,
        typed: &'ir TypedMain,
        checked: &CheckedProgram<'_>,
        fragment_node: impl Fn(usize) -> Option<TypedNodeRef<'ir>>,
    ) -> Result<(), String> {
        self.check()?;
        self.budget.records(self.fragments.len())?;
        let mut covered_generated = vec![false; self.fragments.len()];
        let index = &self.source_index;
        self.budget.records(self.shape_bindings.len())?;
        let mut by_source = HashMap::new();
        for (slot, binding) in self.shape_bindings.iter().enumerate() {
            self.budget.work(1)?;
            if by_source
                .insert((binding.instance, binding.source), slot)
                .is_some()
            {
                return Err(fail("duplicate shape source binding"));
            }
            let Some(AstNode::Expression(expr @ Expr::Let(_, _, _))) = index.node(binding.source)
            else {
                return Err(fail("shape binding is not a source let"));
            };
            if checked.polymorphic_local(expr) != Some(binding.checked_identity)
                || index.child(binding.source, AstStep::LetValue) != Some(binding.initializer)
                || index.child(binding.source, AstStep::LetBody) != Some(binding.continuation)
                || self
                    .shapes
                    .get(binding.shape)
                    .is_none_or(|shape| shape.instance != binding.instance)
            {
                return Err(fail("shape binder identity or source route mismatch"));
            }
        }
        // Index the explicit alias dispositions, not spelling or equal syntax.
        self.budget.records(self.dispositions.len())?;
        let mut aliases = HashMap::new();
        self.budget.records(self.dispositions.len())?;
        let mut source_dispositions: HashMap<(usize, usize), Vec<&RawDisposition>> = HashMap::new();
        for receipt in &self.dispositions {
            self.budget.work(1)?;
            self.budget.edges(1)?;
            source_dispositions
                .entry((receipt.instance, receipt.source))
                .or_default()
                .push(&receipt.disposition);
            if let RawDisposition::AliasOfShape { shape, binding } = receipt.disposition {
                let binder = *by_source
                    .get(&(receipt.instance, binding))
                    .ok_or_else(|| fail("alias names missing binder"))?;
                let binder = &self.shape_bindings[binder];
                let Some(AstNode::Expression(expr)) = index.node(receipt.source) else {
                    return Err(fail("alias has no source expression"));
                };
                if checked.local_use(expr) != Some(binder.checked_identity) || binder.shape != shape
                {
                    return Err(fail("alias checker-selected binding mismatch"));
                }
                if aliases
                    .insert((receipt.instance, receipt.source), (shape, binding, false))
                    .is_some()
                {
                    return Err(fail("duplicate shape alias receipt"));
                }
            }
        }
        self.budget.records(self.captures.len())?;
        let mut seen_captures = vec![false; self.captures.len()];
        for (slot, shape) in self.shapes.iter().enumerate() {
            self.budget.work(1)?;
            if shape.instance >= typed.functions.len() || index.function(shape.source).is_none() {
                return Err(fail("shape source or instance outside body coverage"));
            }
            if !matches!(shape.kind, ShapeReceiptKind::Capture { .. }) {
                let dispositions = source_dispositions
                    .get(&(shape.instance, shape.source))
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                self.budget.work(dispositions.len())?;
                if !dispositions
                    .iter()
                    .any(|disposition| **disposition == RawDisposition::RepresentedByShape(slot))
                {
                    return Err(fail(
                        "shape initializer lacks its explicit shape disposition",
                    ));
                }
            }
            match &shape.kind {
                ShapeReceiptKind::Capture { capture } => {
                    let seen = seen_captures
                        .get_mut(*capture)
                        .ok_or_else(|| fail("capture outside receipt table"))?;
                    if std::mem::replace(seen, true) {
                        return Err(fail("two shape leaves share one evaluation receipt"));
                    }
                    let capture = self
                        .captures
                        .get(*capture)
                        .ok_or_else(|| fail("shape capture outside table"))?;
                    if capture.instance != shape.instance
                        || capture.source != shape.source
                        || expression(fragment_node(capture.evaluation))?.ty != capture.ty
                    {
                        return Err(fail("shape capture source, instance or type mismatch"));
                    }
                    let fragment = self
                        .fragments
                        .get(capture.evaluation)
                        .ok_or_else(|| fail("capture evaluation missing"))?;
                    if fragment.instance != shape.instance
                        || !matches!(fragment.origin,trace::RawOrigin::Source {source,rule:DirectRule::Expression} if source == shape.source)
                    {
                        return Err(fail("capture evaluation lost its initializer origin"));
                    }
                }
                ShapeReceiptKind::Constructor {
                    definition, fields, ..
                }
                | ShapeReceiptKind::Record { definition, fields } => {
                    let Some(AstNode::TypeDeclaration(decl)) = index.node(*definition) else {
                        return Err(fail("shape nominal has no source declaration"));
                    };
                    let Some(AstNode::Expression(source)) = index.node(shape.source) else {
                        return Err(fail("shape has no initializer expression"));
                    };
                    if !matches!(checked.expression_type(source), Some(crate::check::Type::Nominal(name, _)) if *name == decl.name)
                    {
                        return Err(fail(
                            "shape declaration differs from checked nominal identity",
                        ));
                    }
                    let is_record = matches!(shape.kind, ShapeReceiptKind::Record { .. });
                    match (&shape.kind, source) {
                        (ShapeReceiptKind::Constructor { variant, .. }, Expr::Call(name, args))
                            if decl.record_fields.is_none()
                                && decl.variants.get(*variant).is_some_and(|v| {
                                    v.name == *name && v.fields.len() == fields.len()
                                })
                                && args.len() == fields.len() => {}
                        (ShapeReceiptKind::Constructor { variant, .. }, Expr::Var(name))
                            if fields.is_empty()
                                && decl.record_fields.is_none()
                                && decl
                                    .variants
                                    .get(*variant)
                                    .is_some_and(|v| v.name == *name && v.fields.is_empty()) => {}
                        (ShapeReceiptKind::Record { .. }, Expr::Record(name, written))
                            if decl.name == *name
                                && decl
                                    .record_fields
                                    .as_ref()
                                    .is_some_and(|names| names.len() == fields.len())
                                && written.len() == fields.len() => {}
                        _ => return Err(fail("shape initializer grammar or nominal mismatch")),
                    }
                    for (written, edge) in fields.iter().enumerate() {
                        self.budget.work(1)?;
                        let child = self
                            .shapes
                            .get(edge.shape)
                            .ok_or_else(|| fail("shape child outside table"))?;
                        let step = if is_record {
                            AstStep::RecordField(written)
                        } else {
                            AstStep::Argument(written)
                        };
                        if edge.written != written
                            || edge.shape >= slot
                            || child.instance != shape.instance
                            || index.child(shape.source, step) != Some(edge.source)
                        {
                            return Err(fail("shape child occurrence/order/DAG mismatch"));
                        }
                        if child.source != edge.source
                            && aliases
                                .get(&(shape.instance, edge.source))
                                .is_none_or(|(selected, _, _)| *selected != edge.shape)
                        {
                            return Err(fail(
                                "shape child is neither direct initializer nor validated alias",
                            ));
                        }
                        if is_record {
                            let Expr::Record(_, written_fields) = source else {
                                unreachable!()
                            };
                            let names = decl.record_fields.as_ref().unwrap();
                            let info = checked.record_info(source).ok_or_else(|| {
                                fail("shape record lacks checked field identities")
                            })?;
                            if info.field_indices.get(written) != Some(&edge.declaration)
                                || names.get(edge.declaration) != Some(&written_fields[written].0)
                            {
                                return Err(fail("shape record declaration field mismatch"));
                            }
                        } else if edge.declaration != written {
                            return Err(fail("shape constructor ordinal mismatch"));
                        }
                    }
                }
            }
        }
        self.budget.work(seen_captures.len())?;
        if seen_captures.iter().any(|seen| !seen) {
            return Err(fail("unreachable capture evaluation receipt"));
        }
        self.budget.records(self.shapes.len())?;
        let mut seen_shapes = vec![false; self.shapes.len()];
        for (binding_id, binding) in self.shape_bindings.iter().enumerate() {
            self.budget.work(1)?;
            let body = binding
                .body
                .ok_or_else(|| fail("unfinished shape binding"))?;
            let output = binding
                .output
                .ok_or_else(|| fail("shape binding missing output"))?;
            if self
                .fragments
                .get(body)
                .is_none_or(|fragment| fragment.instance != binding.instance)
                || self
                    .fragments
                    .get(output)
                    .is_none_or(|fragment| fragment.instance != binding.instance)
                || index.function(self.shapes[binding.shape].source)
                    != index.function(binding.source)
            {
                return Err(fail(
                    "shape binding source or output belongs to another function instance",
                ));
            }
            let dispositions = source_dispositions
                .get(&(binding.instance, binding.source))
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            self.budget.work(dispositions.len())?;
            let expected = if binding.environment.is_some() {
                RawDisposition::Output(output)
            } else {
                RawDisposition::StaticShapeOnly {
                    shape: binding.shape,
                    continuation: binding.continuation,
                }
            };
            if !dispositions
                .iter()
                .any(|disposition| **disposition == expected)
            {
                return Err(fail(
                    "shape binding lacks its explicit output/erasure disposition",
                ));
            }
            // Walk each newly constructed initializer occurrence once. Aliases
            // are checked edges and do not recursively expand old ancestry.
            self.budget.edges(1)?;
            let mut pending = vec![(binding.initializer, binding.shape)];
            let mut leaf = 0;
            while let Some((source, shape_id)) = pending.pop() {
                self.budget.work(1)?;
                let shape = &self.shapes[shape_id];
                if shape.source != source {
                    let (selected, _, used) = aliases
                        .get_mut(&(binding.instance, source))
                        .ok_or_else(|| fail("binding initializer lacks its alias receipt"))?;
                    if *selected != shape_id || std::mem::replace(used, true) {
                        return Err(fail("binding initializer alias is incorrect or reused"));
                    }
                    continue;
                }
                if seen_shapes[shape_id] {
                    return Err(fail("initializer occurrence constructed more than once"));
                }
                seen_shapes[shape_id] = true;
                match &shape.kind {
                    ShapeReceiptKind::Capture { capture } => {
                        if binding.captures.get(leaf) != Some(capture) {
                            return Err(fail("capture evaluation order mismatch"));
                        }
                        leaf += 1;
                    }
                    ShapeReceiptKind::Constructor { fields, .. }
                    | ShapeReceiptKind::Record { fields, .. } => {
                        self.budget.edges(fields.len())?;
                        pending.extend(fields.iter().rev().map(|edge| (edge.source, edge.shape)));
                    }
                }
            }
            if leaf != binding.captures.len() {
                return Err(fail("extra capture evaluations in environment"));
            }
            if let Some((root, adt)) = binding.environment {
                let output_node = expression(fragment_node(output))?;
                let ExprKind::Let {
                    local,
                    mutable: false,
                    value,
                    body: actual_body,
                } = &output_node.kind
                else {
                    return Err(fail("capture environment is not its generated let"));
                };
                let ExprKind::Record {
                    adt: actual_adt,
                    fields,
                } = &value.kind
                else {
                    return Err(fail("capture environment lacks product"));
                };
                let layout = typed
                    .adts
                    .get(adt)
                    .ok_or_else(|| fail("capture layout outside registry"))?;
                if *local != root
                    || *actual_adt != adt
                    || layout.origin != AdtOrigin::LocalCapture
                    || fields.len() != binding.captures.len()
                    || fields.is_empty()
                {
                    return Err(fail("capture environment root/layout mismatch"));
                }
                same_expression(fragment_node(body), actual_body)?;
                let expected = std::iter::once((
                    RelationRole::IntroducedForBinding,
                    RawTarget::Source(binding.source),
                ))
                .chain(binding.captures.iter().map(|&capture| {
                    (
                        RelationRole::CapturedLeaf,
                        RawTarget::Fragment(self.captures[capture].evaluation),
                    )
                }));
                generated_matches(
                    &mut self.budget,
                    &mut covered_generated,
                    &self.fragments,
                    output,
                    binding.instance,
                    GenerationReason::CaptureEnvironmentBinding,
                    expected,
                )?;
                let product = self.fragments[output]
                    .children
                    .iter()
                    .find(|(role, _)| *role == TypedChildRole::LetValue)
                    .map(|(_, fragment)| *fragment)
                    .ok_or_else(|| fail("capture product fragment missing"))?;
                same_expression(fragment_node(product), value)?;
                let expected = std::iter::once((
                    RelationRole::IntroducedForBinding,
                    RawTarget::Source(binding.source),
                ))
                .chain(binding.captures.iter().map(|&capture| {
                    (
                        RelationRole::CapturedLeaf,
                        RawTarget::Fragment(self.captures[capture].evaluation),
                    )
                }));
                generated_matches(
                    &mut self.budget,
                    &mut covered_generated,
                    &self.fragments,
                    product,
                    binding.instance,
                    GenerationReason::CaptureProduct,
                    expected,
                )?;
                for (field, (&capture, (actual_field, value))) in
                    binding.captures.iter().zip(fields).enumerate()
                {
                    self.budget.work(1)?;
                    let capture = &self.captures[capture];
                    let env = capture
                        .environment
                        .ok_or_else(|| fail("capture was not assigned environment storage"))?;
                    if env.binding != binding_id
                        || env.root != root
                        || env.adt != adt
                        || env.field != field
                        || *actual_field != field
                        || capture.bookkeeping == root
                        || layout.arguments.get(field) != Some(&capture.ty)
                    {
                        return Err(fail("capture leaf-to-environment translation mismatch"));
                    }
                    same_expression(fragment_node(capture.evaluation), value)?;
                    let tuple = self.fragments[product]
                        .children
                        .get(field)
                        .filter(|(role, _)| *role == TypedChildRole::RecordField(field))
                        .map(|(_, fragment)| *fragment)
                        .ok_or_else(|| fail("capture product field fragment missing"))?;
                    generated_matches(
                        &mut self.budget,
                        &mut covered_generated,
                        &self.fragments,
                        tuple,
                        binding.instance,
                        GenerationReason::CaptureProductField,
                        [
                            (
                                RelationRole::IntroducedForBinding,
                                RawTarget::Source(binding.source),
                            ),
                            (
                                RelationRole::CapturedLeaf,
                                RawTarget::Fragment(capture.evaluation),
                            ),
                        ]
                        .into_iter(),
                    )?;
                }
            } else if !binding.captures.is_empty() || output != body {
                return Err(fail("erased shape let did not retain exact body fragment"));
            }
        }
        self.budget.work(seen_shapes.len())?;
        if seen_shapes.iter().any(|seen| !seen) {
            return Err(fail("unreachable shape construction receipt"));
        }
        self.budget.work(aliases.len())?;
        if aliases.values().any(|(_, _, used)| !used) {
            return Err(fail("alias disposition is not an initializer occurrence"));
        }
        for use_receipt in &self.uses {
            self.budget.work(1)?;
            let binding = self
                .shape_bindings
                .get(use_receipt.binding)
                .ok_or_else(|| fail("shape use binding outside table"))?;
            let Some(AstNode::Expression(source)) = index.node(use_receipt.source) else {
                return Err(fail("shape use has no source occurrence"));
            };
            if checked.local_use(source) != Some(use_receipt.checked_identity)
                || binding.checked_identity != use_receipt.checked_identity
                || binding.instance != use_receipt.instance
                || binding.shape != use_receipt.shape
            {
                return Err(fail("shape use selected another checked binding"));
            }
            let output = use_receipt
                .output
                .ok_or_else(|| fail("unfinished shape use"))?;
            if self
                .fragments
                .get(output)
                .is_none_or(|fragment| fragment.instance != use_receipt.instance)
                || index.function(use_receipt.source) != index.function(binding.source)
                || expression(fragment_node(output))?.ty != use_receipt.target
            {
                return Err(fail("shape use closed result type mismatch"));
            }
            let dispositions = source_dispositions
                .get(&(use_receipt.instance, use_receipt.source))
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            self.budget.work(dispositions.len())?;
            if !dispositions
                .iter()
                .any(|disposition| **disposition == RawDisposition::Output(output))
            {
                return Err(fail("shape use lacks its materialized output disposition"));
            }
            let checked_path = checked.field_info(source).unwrap_or(&[]);
            if checked_path.len() != use_receipt.projection.len() || checked_path.len() > 16 {
                return Err(fail("shape use checked field path length mismatch"));
            }
            if !matches!(source, Expr::Var(_) | Expr::Field(_, _))
                || (matches!(source, Expr::Field(_, _)) && checked_path.is_empty())
            {
                return Err(fail("invalid shape use grammar"));
            }
            let mut selected = use_receipt.shape;
            let mut capture_offset = None;
            for (offset, (step, checked_step)) in
                use_receipt.projection.iter().zip(checked_path).enumerate()
            {
                self.budget.work(1)?;
                let Some(AstNode::TypeDeclaration(decl)) = index.node(step.definition) else {
                    return Err(fail("projected shape definition missing"));
                };
                if decl.name != checked_step.record
                    || step.field != checked_step.index
                    || step.arity != checked_step.arity
                {
                    return Err(fail("projected shape checked path mismatch"));
                }
                if capture_offset.is_some() {
                    continue;
                }
                match &self.shapes[selected].kind {
                    ShapeReceiptKind::Record { definition, fields }
                        if *definition == step.definition && fields.len() == step.arity =>
                    {
                        self.budget.work(fields.len())?;
                        selected = fields
                            .iter()
                            .find(|field| field.declaration == step.field)
                            .ok_or_else(|| fail("projected shape field missing"))?
                            .shape;
                    }
                    ShapeReceiptKind::Capture { .. } => capture_offset = Some(offset),
                    _ => return Err(fail("shape projection route mismatch")),
                }
            }
            if use_receipt.selected != Some(selected) {
                return Err(fail("shape use selected a different shape node"));
            }
            if let Some(offset) = capture_offset {
                let temp = use_receipt
                    .temporary
                    .as_ref()
                    .ok_or_else(|| fail("captured-record projection lacks temporary"))?;
                if temp.offset != offset
                    || temp.shape != selected
                    || temp.output != output
                    || !use_receipt.materializations.is_empty()
                {
                    return Err(fail("captured-record temporary context mismatch"));
                }
                let ShapeReceiptKind::Capture { capture } = self.shapes[selected].kind else {
                    return Err(fail("projected temporary did not select capture"));
                };
                let capture = &self.captures[capture];
                let env = capture
                    .environment
                    .ok_or_else(|| fail("projected capture has no environment"))?;
                let capture_value = expression(fragment_node(temp.capture))?;
                let read = expression(fragment_node(temp.read))?;
                let outer = expression(fragment_node(output))?;
                let ExprKind::Let {
                    local,
                    mutable: false,
                    value,
                    body,
                } = &outer.kind
                else {
                    return Err(fail("projected temporary lacks generated let"));
                };
                let ExprKind::FieldRead(place) = &read.kind else {
                    return Err(fail("projected use lacks field read"));
                };
                if *local != temp.root
                    || place.root != temp.root
                    || place.path != temp.path
                    || temp.path.len() != checked_path.len() - offset
                    || capture_value.ty != capture.ty
                {
                    return Err(fail("projected temporary root, path or type mismatch"));
                }
                same_expression(fragment_node(temp.capture), value)?;
                same_expression(fragment_node(temp.read), body)?;
                validate_capture_read(capture_value, env, capture.ty)?;
                let mut ty = capture.ty;
                for (step, checked_step) in temp.path.iter().zip(&checked_path[offset..]) {
                    self.budget.work(1)?;
                    let IrType::Adt { id, owned: false } = ty else {
                        return Err(fail("temporary projection has non-record step"));
                    };
                    let adt = typed
                        .adts
                        .get(id)
                        .ok_or_else(|| fail("temporary record type missing"))?;
                    if step.record != id
                        || step.field != checked_step.index
                        || adt.name != checked_step.record
                        || adt.record_fields.as_ref().map(Vec::len) != Some(checked_step.arity)
                    {
                        return Err(fail("temporary projection checked path mismatch"));
                    }
                    ty = *adt.variants[0]
                        .fields
                        .get(step.field)
                        .ok_or_else(|| fail("temporary projection field missing"))?;
                }
                if ty != use_receipt.target || read.ty != ty {
                    return Err(fail("temporary projected result type mismatch"));
                }
                generated_matches(
                    &mut self.budget,
                    &mut covered_generated,
                    &self.fragments,
                    temp.capture,
                    use_receipt.instance,
                    GenerationReason::CaptureProjection,
                    [
                        (
                            RelationRole::CausedByUse,
                            RawTarget::Source(use_receipt.source),
                        ),
                        (RelationRole::SelectedShape, RawTarget::Shape(selected)),
                        (
                            RelationRole::ReadsCapture,
                            RawTarget::Fragment(capture.evaluation),
                        ),
                    ]
                    .into_iter(),
                )?;
                generated_matches(
                    &mut self.budget,
                    &mut covered_generated,
                    &self.fragments,
                    output,
                    use_receipt.instance,
                    GenerationReason::ProjectedCaptureTemporary,
                    [
                        (
                            RelationRole::CausedByUse,
                            RawTarget::Source(use_receipt.source),
                        ),
                        (RelationRole::SelectedShape, RawTarget::Shape(selected)),
                    ]
                    .into_iter(),
                )?;
                let seen = covered_generated
                    .get_mut(temp.read)
                    .ok_or_else(|| fail("projected source read outside coverage"))?;
                if std::mem::replace(seen, true) {
                    return Err(fail("projected source read claimed more than once"));
                }
                let fragment = self
                    .fragments
                    .get(temp.read)
                    .ok_or_else(|| fail("projected source read missing"))?;
                if fragment.instance != use_receipt.instance
                    || !matches!(fragment.origin,trace::RawOrigin::Source {source,rule:DirectRule::ProjectedUse} if source == use_receipt.source)
                {
                    return Err(fail("projected read lost its explicit temporary-root rule"));
                }
            } else {
                if use_receipt.temporary.is_some() {
                    return Err(fail("unexpected projected capture temporary"));
                }
                self.budget.records(use_receipt.materializations.len())?;
                let mut occurrences = HashMap::new();
                for (position, materialized) in use_receipt.materializations.iter().enumerate() {
                    self.budget.work(1)?;
                    if occurrences
                        .insert(materialized.output, (position, 0usize))
                        .is_some()
                    {
                        return Err(fail("shared output materialization fragment"));
                    }
                    let shape = self
                        .shapes
                        .get(materialized.shape)
                        .ok_or_else(|| fail("materialized shape missing"))?;
                    let value = expression(fragment_node(materialized.output))?;
                    if shape.instance != use_receipt.instance || value.ty != materialized.target {
                        return Err(fail("materialized shape instance or type mismatch"));
                    }
                    let children: &[ShapeEdgeReceipt] = match &shape.kind {
                        ShapeReceiptKind::Capture { capture } => {
                            let capture = &self.captures[*capture];
                            validate_capture_read(
                                value,
                                capture.environment.ok_or_else(|| {
                                    fail("materialized capture has no environment")
                                })?,
                                capture.ty,
                            )?;
                            generated_matches(
                                &mut self.budget,
                                &mut covered_generated,
                                &self.fragments,
                                materialized.output,
                                use_receipt.instance,
                                GenerationReason::CaptureProjection,
                                [
                                    (
                                        RelationRole::CausedByUse,
                                        RawTarget::Source(use_receipt.source),
                                    ),
                                    (
                                        RelationRole::SelectedShape,
                                        RawTarget::Shape(materialized.shape),
                                    ),
                                    (
                                        RelationRole::ReadsCapture,
                                        RawTarget::Fragment(capture.evaluation),
                                    ),
                                ]
                                .into_iter(),
                            )?;
                            &[]
                        }
                        ShapeReceiptKind::Constructor {
                            definition,
                            variant,
                            fields,
                        } => {
                            let Some(AstNode::TypeDeclaration(decl)) = index.node(*definition)
                            else {
                                return Err(fail("materialized constructor definition missing"));
                            };
                            let ExprKind::Constructor {
                                adt,
                                variant: actual,
                                fields: values,
                            } = &value.kind
                            else {
                                return Err(fail("materialized constructor operation mismatch"));
                            };
                            let layout = typed
                                .adts
                                .get(*adt)
                                .ok_or_else(|| fail("materialized constructor layout missing"))?;
                            if layout.origin != AdtOrigin::Source
                                || layout.name != decl.name
                                || *actual != *variant
                                || values.len() != fields.len()
                                || value.ty
                                    != (IrType::Adt {
                                        id: *adt,
                                        owned: false,
                                    })
                            {
                                return Err(fail(
                                    "materialized constructor nominal/variant mismatch",
                                ));
                            }
                            fields
                        }
                        ShapeReceiptKind::Record { definition, fields } => {
                            let Some(AstNode::TypeDeclaration(decl)) = index.node(*definition)
                            else {
                                return Err(fail("materialized record definition missing"));
                            };
                            let ExprKind::Record {
                                adt,
                                fields: values,
                            } = &value.kind
                            else {
                                return Err(fail("materialized record operation mismatch"));
                            };
                            let layout = typed
                                .adts
                                .get(*adt)
                                .ok_or_else(|| fail("materialized record layout missing"))?;
                            if layout.origin != AdtOrigin::Source
                                || layout.name != decl.name
                                || values.len() != fields.len()
                                || value.ty
                                    != (IrType::Adt {
                                        id: *adt,
                                        owned: false,
                                    })
                            {
                                return Err(fail("materialized record nominal mismatch"));
                            }
                            fields
                        }
                    };
                    if !matches!(shape.kind, ShapeReceiptKind::Capture { .. }) {
                        generated_matches(
                            &mut self.budget,
                            &mut covered_generated,
                            &self.fragments,
                            materialized.output,
                            use_receipt.instance,
                            GenerationReason::ShapeMaterialization,
                            [
                                (
                                    RelationRole::CausedByUse,
                                    RawTarget::Source(use_receipt.source),
                                ),
                                (
                                    RelationRole::SelectedShape,
                                    RawTarget::Shape(materialized.shape),
                                ),
                            ]
                            .into_iter(),
                        )?;
                    }
                    if children.len() != materialized.children.len() {
                        return Err(fail("materialization child count mismatch"));
                    }
                    for (written, (edge, &child)) in
                        children.iter().zip(&materialized.children).enumerate()
                    {
                        self.budget.work(1)?;
                        let (child_position, count) =
                            occurrences.get_mut(&child).ok_or_else(|| {
                                fail("materialization child missing or not constructed first")
                            })?;
                        if *child_position >= position
                            || use_receipt.materializations[*child_position].shape != edge.shape
                        {
                            return Err(fail("materialization child selects another shape"));
                        }
                        *count += 1;
                        let actual = match &value.kind {
                            ExprKind::Constructor { fields, .. } => &fields[written],
                            ExprKind::Record { fields, .. } => {
                                if fields[written].0 != edge.declaration {
                                    return Err(fail("materialized record field order mismatch"));
                                }
                                let tuple = self.fragments[materialized.output]
                                    .children
                                    .get(written)
                                    .filter(|(role, _)| {
                                        *role == TypedChildRole::RecordField(written)
                                    })
                                    .map(|(_, fragment)| *fragment)
                                    .ok_or_else(|| fail("materialized record tuple missing"))?;
                                generated_matches(
                                    &mut self.budget,
                                    &mut covered_generated,
                                    &self.fragments,
                                    tuple,
                                    use_receipt.instance,
                                    GenerationReason::ShapeRecordField,
                                    [
                                        (
                                            RelationRole::CausedByUse,
                                            RawTarget::Source(use_receipt.source),
                                        ),
                                        (
                                            RelationRole::SelectedShape,
                                            RawTarget::Shape(materialized.shape),
                                        ),
                                    ]
                                    .into_iter(),
                                )?;
                                &fields[written].1
                            }
                            _ => return Err(fail("non-product materialization has children")),
                        };
                        same_expression(fragment_node(child), actual)?;
                    }
                }
                let (root, _) = occurrences
                    .get(&output)
                    .ok_or_else(|| fail("shape use root not materialized"))?;
                if use_receipt.materializations[*root].shape != selected {
                    return Err(fail("materialized use root shape mismatch"));
                }
                for (&fragment, &(_, references)) in &occurrences {
                    self.budget.work(1)?;
                    if references != usize::from(fragment != output) {
                        return Err(fail("unreachable or shared materialization output"));
                    }
                }
            }
        }
        for (slot, fragment) in self.fragments.iter().enumerate() {
            self.budget.work(1)?;
            let requires_shape_receipt = match &fragment.origin {
                trace::RawOrigin::Generated { reason, .. } => matches!(
                    reason,
                    GenerationReason::CaptureEnvironmentBinding
                        | GenerationReason::CaptureProduct
                        | GenerationReason::CaptureProductField
                        | GenerationReason::ShapeMaterialization
                        | GenerationReason::ShapeRecordField
                        | GenerationReason::CaptureProjection
                        | GenerationReason::ProjectedCaptureTemporary
                ),
                trace::RawOrigin::Source { rule, .. } => *rule == DirectRule::ProjectedUse,
            };
            if requires_shape_receipt != covered_generated[slot] {
                return Err(fail(
                    "shape output occurrence lacks its unique construction receipt",
                ));
            }
        }
        Ok(())
    }

    pub(super) fn public_shapes(
        &mut self,
        domain: &Domain,
        fragment_slots: &[Option<usize>],
    ) -> Result<Vec<ShapeRecord>, String> {
        let mut result = Vec::new();
        for (slot, shape) in self.shapes.iter().enumerate() {
            self.budget.records(1)?;
            self.budget.work(1)?;
            let source_id = |slot| SourceNodeId {
                domain: domain.clone(),
                slot,
            };
            let shape_id = |slot| ShapeId {
                domain: domain.clone(),
                slot,
            };
            let kind = match &shape.kind {
                ShapeReceiptKind::Capture { capture } => {
                    let evaluation = self
                        .captures
                        .get(*capture)
                        .ok_or_else(|| fail("public shape capture missing"))?
                        .evaluation;
                    let slot = fragment_slots
                        .get(evaluation)
                        .copied()
                        .flatten()
                        .ok_or_else(|| fail("public capture evaluation missing"))?;
                    ShapeKind::Capture {
                        evaluation: TypedNodeId {
                            domain: domain.clone(),
                            slot,
                        },
                    }
                }
                ShapeReceiptKind::Constructor {
                    definition, fields, ..
                }
                | ShapeReceiptKind::Record { definition, fields } => {
                    let mut children = Vec::new();
                    for edge in fields {
                        self.budget.edges(1)?;
                        self.budget.work(1)?;
                        children.push(ShapeChild {
                            source: source_id(edge.source),
                            shape: shape_id(edge.shape),
                            written: edge.written,
                            declaration: edge.declaration,
                        });
                    }
                    match &shape.kind {
                        ShapeReceiptKind::Constructor { variant, .. } => ShapeKind::Constructor {
                            definition: source_id(*definition),
                            variant: *variant,
                            fields: children,
                        },
                        ShapeReceiptKind::Record { .. } => ShapeKind::Record {
                            definition: source_id(*definition),
                            fields: children,
                        },
                        _ => unreachable!(),
                    }
                }
            };
            result.push(ShapeRecord {
                id: shape_id(slot),
                instance: FunctionInstanceId {
                    domain: domain.clone(),
                    slot: shape.instance,
                },
                source: source_id(shape.source),
                kind,
            });
        }
        Ok(result)
    }
}

fn validate_capture_read(
    value: &ExprNode,
    environment: CaptureEnvironment,
    ty: IrType,
) -> Result<(), String> {
    let ExprKind::FieldRead(place) = &value.kind else {
        return Err(fail("capture projection is not a field read"));
    };
    if value.ty != ty
        || place.root != environment.root
        || place.path.len() != 1
        || place.path[0]
            != (FieldStep {
                record: environment.adt,
                field: environment.field,
            })
    {
        return Err(fail("capture projection reads another environment or leaf"));
    }
    Ok(())
}

impl trace::Trace<'_> {
    pub(in crate::typed) fn shape_lookup_work(&mut self, count: usize) -> bool {
        self.observe(|this| this.budget.work(count)).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_relations_charge_only_selected_fields_and_reject_invalid_selections() {
        let path = std::env::temp_dir().join(format!(
            "rill-selected-capture-relations-{}.rill",
            std::process::id()
        ));
        std::fs::write(&path, "fn main() { 42 }").unwrap();
        let sources = crate::modules::compile::load_program_with_sources(&path).unwrap();
        std::fs::remove_file(path).unwrap();

        fn with_captures(sources: &SourceProgram, fields: Vec<usize>) -> Trace<'_> {
            let mut trace = Trace::new(sources, BodyBudget::default());
            trace.check().unwrap();
            trace.shape_bindings.push(ShapeBindingReceipt {
                instance: 0,
                source: 0,
                initializer: 1,
                continuation: 1,
                checked_identity: 0,
                shape: 0,
                captures: fields,
                environment: None,
                body: None,
                output: None,
            });
            trace.captures = (0..3)
                .map(|index| CaptureReceipt {
                    instance: 0,
                    source: 1,
                    bookkeeping: LocalId(index as u32),
                    ty: IrType::Int,
                    evaluation: 42 + index,
                    environment: None,
                })
                .collect();
            trace
        }

        let binding = RawRelation {
            role: RelationRole::IntroducedForBinding,
            target: RawTarget::Source(0),
        };
        let leaf = |evaluation| RawRelation {
            role: RelationRole::CapturedLeaf,
            target: RawTarget::Fragment(evaluation),
        };
        for (width, selected) in [(1, 0), (256, 0), (256, 255)] {
            // All unselected slots are deliberately invalid. A field-specific
            // request must follow one field, with width-independent charges.
            let mut fields = vec![usize::MAX; width];
            fields[selected] = 0;
            let mut trace = with_captures(&sources, fields);
            trace.budget = BodyBudget::with_limits(0, 2, 5);
            assert_eq!(
                trace.capture_relations(Some(0), Some(selected)),
                [binding.clone(), leaf(42)]
            );
            trace.check().unwrap();
            assert_eq!(
                (
                    trace.budget.records,
                    trace.budget.edges,
                    trace.budget.work.get()
                ),
                (0, 0, 0)
            );
        }

        let mut trace = with_captures(&sources, vec![0, 1, 2]);
        trace.budget = BodyBudget::with_limits(0, 4, 14);
        assert_eq!(
            trace.capture_relations(Some(0), None),
            [binding.clone(), leaf(42), leaf(43), leaf(44)]
        );
        trace.check().unwrap();
        assert_eq!((trace.budget.edges, trace.budget.work.get()), (0, 0));

        let mut trace = with_captures(&sources, vec![0]);
        trace.budget = BodyBudget::with_limits(0, 2, 4);
        assert_eq!(trace.capture_relations(Some(0), Some(0)), [binding]);
        assert!(trace
            .check()
            .unwrap_err()
            .starts_with("E-TYPED-SOURCE-MAP-LIMIT"));
        assert_eq!((trace.budget.edges, trace.budget.work.get()), (1, 1));
        assert!(trace.capture_relations(Some(0), Some(0)).is_empty());
        assert_eq!((trace.budget.edges, trace.budget.work.get()), (1, 1));

        for selected in [1, usize::MAX] {
            let mut trace = with_captures(&sources, vec![0]);
            trace.budget = BodyBudget::with_limits(0, 2, 1);
            assert!(trace.capture_relations(Some(0), Some(selected)).is_empty());
            assert!(trace
                .check()
                .unwrap_err()
                .contains("invalid capture relation field"));
            assert_eq!((trace.budget.edges, trace.budget.work.get()), (2, 0));
        }
    }

    #[test]
    fn capture_projection_rejects_same_typed_wrong_root_and_field() {
        let env = CaptureEnvironment {
            binding: 0,
            root: LocalId(7),
            adt: 3,
            field: 2,
        };
        let value = |root, field| ExprNode {
            ty: IrType::Int,
            kind: ExprKind::FieldRead(FieldPlace {
                root,
                path: vec![FieldStep { record: 3, field }],
            }),
        };
        assert!(validate_capture_read(&value(LocalId(7), 2), env, IrType::Int).is_ok());
        assert!(
            validate_capture_read(&value(LocalId(8), 2), env, IrType::Int)
                .unwrap_err()
                .starts_with("E-TYPED-SOURCE-MAP:")
        );
        assert!(
            validate_capture_read(&value(LocalId(7), 1), env, IrType::Int)
                .unwrap_err()
                .starts_with("E-TYPED-SOURCE-MAP:")
        );
    }

    #[test]
    fn capture_projection_rejects_conflating_environment_and_source_steps() {
        let env = CaptureEnvironment {
            binding: 0,
            root: LocalId(7),
            adt: 3,
            field: 2,
        };
        let value = ExprNode {
            ty: IrType::Int,
            kind: ExprKind::FieldRead(FieldPlace {
                root: LocalId(7),
                path: vec![
                    FieldStep {
                        record: 3,
                        field: 2,
                    },
                    FieldStep {
                        record: 4,
                        field: 0,
                    },
                ],
            }),
        };
        assert!(validate_capture_read(&value, env, IrType::Int).is_err());
    }
}
