//! Capture-and-shape elaboration of the checker's nonexpansive local schemes.
//!
//! A shape is a compile-time product/tag tree, never a value at an arbitrary
//! concrete type. Every concrete leaf is evaluated once at the original let
//! into a fresh immutable Copy slot. Aliases share those slots. At each use,
//! the requested closed type determines concrete constructors and projections;
//! the ordinary IR verifier then checks every operation and capture identity.
//! This is a lowering construction, not a proof that source and IR coincide.
use super::provenance::nodes::shapes::ShapeReceiptKind;
use super::provenance::nodes::{
    DirectRule, Edges, GenerationReason, LoweredExpr, RawDisposition, TypedChildRole, TypedNodeRef,
};
use super::*;
use crate::source_map::AstNode;
use std::rc::Rc;

pub(super) const MAX_WORK: usize = 100_000;
pub(super) const MAX_CAPTURES: usize = 256;
const MAX_DEPTH: usize = 128;
const COMPLEXITY: &str = "E-TYPED-LOCAL-COMPLEXITY: local shape elaboration limit exceeded";
pub(super) const CAPTURE_NAME: &str = "$local_capture";
const INVALID: &str = "E-TYPED-LOCAL: invalid polymorphic value shape";

pub(super) struct Budget {
    remaining: usize,
    captures: usize,
    copy_types: HashSet<Type>,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            remaining: MAX_WORK,
            captures: 0,
            copy_types: HashSet::new(),
        }
    }
}
impl Budget {
    fn charge(&mut self) -> Result<(), String> {
        self.charge_many(1)
    }
    fn charge_many(&mut self, count: usize) -> Result<(), String> {
        self.remaining = self.remaining.checked_sub(count).ok_or(COMPLEXITY)?;
        Ok(())
    }
    fn capture(&mut self) -> Result<(), String> {
        if self.captures >= MAX_CAPTURES {
            return Err(COMPLEXITY.into());
        }
        self.captures += 1;
        Ok(())
    }
}

pub(super) enum Shape {
    Capture {
        local: LocalId,
        ty: Type,
        trace: Option<usize>,
    },
    Constructor {
        nominal: String,
        name: String,
        fields: Vec<Rc<Shape>>,
        height: usize,
        trace: Option<usize>,
    },
    Record {
        nominal: String,
        fields: Vec<(usize, Rc<Shape>)>,
        height: usize,
        trace: Option<usize>,
    },
}

impl Shape {
    fn trace(&self) -> Option<usize> {
        match self {
            Self::Capture { trace, .. }
            | Self::Constructor { trace, .. }
            | Self::Record { trace, .. } => *trace,
        }
    }
    fn height(&self) -> usize {
        match self {
            Self::Capture { .. } => 0,
            Self::Constructor { height, .. } | Self::Record { height, .. } => *height,
        }
    }
}
fn product_height<'a>(fields: impl Iterator<Item = &'a Rc<Shape>>) -> Result<usize, String> {
    let height = fields.map(|field| field.height()).max().unwrap_or(0) + 1;
    if height > MAX_DEPTH {
        return Err(COMPLEXITY.into());
    }
    Ok(height)
}

struct CapturedValue {
    local: LocalId,
    value: LoweredExpr,
    receipt: Option<usize>,
}

impl Lower<'_, '_> {
    pub(super) fn local_shape(&self, expr: &Expr) -> Option<Rc<Shape>> {
        self.checked
            .local_use(expr)
            .and_then(|id| self.shapes.get(&id))
            .cloned()
    }

    pub(super) fn polymorphic_let(
        &mut self,
        expr: &Expr,
        identity: usize,
    ) -> Result<LoweredExpr, String> {
        let Expr::Let(name, value, body) = expr else {
            return Err(INVALID.into());
        };
        let mut captures = Vec::new();
        let shape = self.capture_shape(value, &mut captures, 0)?;
        // Preserve once-only, written-order evaluation in a single product.
        let environment = if captures.is_empty() {
            None
        } else {
            let fields: Vec<_> = captures
                .iter()
                .map(|capture| capture.value.value.ty)
                .collect();
            let adt = self.capture_record(&fields)?;
            let root = self.fresh()?;
            for (field, capture) in captures.iter().enumerate() {
                self.capture_places.insert(
                    capture.local,
                    FieldPlace {
                        root,
                        path: vec![FieldStep { record: adt, field }],
                    },
                );
            }
            Some((root, adt))
        };
        let binding = self.node_trace.as_deref_mut().and_then(|trace| {
            let source = trace.source(AstNode::Expression(expr));
            let initializer = trace.source(AstNode::Expression(value));
            let continuation = trace.source(AstNode::Expression(body));
            trace.shape_binding(
                source,
                initializer,
                continuation,
                identity,
                shape.trace(),
                captures.iter().map(|capture| capture.receipt),
                environment,
            )
        });
        let previous = self.locals.remove(name);
        if self.shapes.insert(identity, shape).is_some() {
            return Err(INVALID.into());
        }
        let lowered = self.expr_observed(body);
        self.shapes.remove(&identity);
        for capture in &captures {
            self.capture_places.remove(&capture.local);
        }
        if let Some(previous) = previous {
            self.locals.insert(name.clone(), previous);
        }
        let body = lowered?;
        let body_trace = body.trace;
        let result = if let Some((root, adt)) = environment {
            let mut fields = Vec::new();
            let mut product_edges = Edges::new();
            for (index, capture) in captures.into_iter().enumerate() {
                if let Some(trace) = self.node_trace.as_deref_mut() {
                    let mut edges = Edges::new();
                    trace.edge(&mut edges, TypedChildRole::RecordValue, capture.value.trace);
                    let relations = trace.capture_relations(binding, Some(index));
                    let field = trace.generated(
                        GenerationReason::CaptureProductField,
                        relations,
                        TypedNodeRef::RecordField {
                            written: index,
                            declaration: index,
                            value: &capture.value.value,
                        },
                        edges,
                    );
                    trace.edge(
                        &mut product_edges,
                        TypedChildRole::RecordField(index),
                        field,
                    );
                }
                fields.push((index, capture.value.value));
            }
            let product = ExprNode {
                ty: Type::Adt {
                    id: adt,
                    owned: false,
                },
                kind: ExprKind::Record { adt, fields },
            };
            let product_trace = self.node_trace.as_deref_mut().and_then(|trace| {
                let relations = trace.capture_relations(binding, None);
                trace.generated(
                    GenerationReason::CaptureProduct,
                    relations,
                    TypedNodeRef::Expression(&product),
                    product_edges,
                )
            });
            let value = ExprNode {
                ty: body.value.ty,
                kind: ExprKind::Let {
                    local: root,
                    mutable: false,
                    value: Box::new(product),
                    body: Box::new(body.value),
                },
            };
            let origin = self.node_trace.as_deref_mut().and_then(|trace| {
                let mut edges = Edges::new();
                trace.edge(&mut edges, TypedChildRole::LetValue, product_trace);
                trace.edge(&mut edges, TypedChildRole::LetBody, body_trace);
                let relations = trace.capture_relations(binding, None);
                trace.generated(
                    GenerationReason::CaptureEnvironmentBinding,
                    relations,
                    TypedNodeRef::Expression(&value),
                    edges,
                )
            });
            LoweredExpr {
                value,
                trace: origin,
            }
        } else {
            // Erasure forwards the exact continuation fragment. No synthetic
            // unit, let, or relabelled source origin may be introduced here.
            body
        };
        if let Some(trace) = self.node_trace.as_deref_mut() {
            trace.finish_shape_binding(binding, body_trace, result.trace);
        }
        Ok(result)
    }

    fn capture_record(&mut self, fields: &[Type]) -> Result<usize, String> {
        // Each requesting environment has its own binding receipt, including
        // a dedup hit. A layout is not a source binding or source declaration.
        if let Some(id) = self
            .registry
            .types
            .iter()
            .position(|adt| adt.origin == AdtOrigin::LocalCapture && adt.arguments == fields)
        {
            return Ok(id);
        }
        if self.registry.types.len() >= 1024 {
            return Err("E-TYPED-ADT: excessive capture layouts".into());
        }
        let id = self.registry.types.len();
        self.registry.types.push(AdtType {
            origin: AdtOrigin::LocalCapture,
            name: CAPTURE_NAME.into(),
            arguments: fields.to_vec(),
            owned: false,
            record_fields: Some(
                (0..fields.len())
                    .map(|index| format!("capture_{index}"))
                    .collect(),
            ),
            variants: vec![AdtVariant {
                name: CAPTURE_NAME.into(),
                fields: fields.to_vec(),
            }],
        });
        Ok(id)
    }

    fn capture_value(
        &mut self,
        local: LocalId,
        ty: Type,
        shape: Option<usize>,
        use_id: Option<usize>,
    ) -> Result<LoweredExpr, String> {
        let place = self.capture_places.get(&local).ok_or(INVALID)?.clone();
        let value = ExprNode {
            ty,
            kind: ExprKind::FieldRead(place),
        };
        let origin = self.node_trace.as_deref_mut().and_then(|trace| {
            let relations = trace.shape_relations(use_id, shape, true);
            trace.generated(
                GenerationReason::CaptureProjection,
                relations,
                TypedNodeRef::Expression(&value),
                Edges::new(),
            )
        });
        Ok(LoweredExpr {
            value,
            trace: origin,
        })
    }

    fn capture_shape(
        &mut self,
        expr: &Expr,
        captures: &mut Vec<CapturedValue>,
        depth: usize,
    ) -> Result<Rc<Shape>, String> {
        self.local_work.charge()?;
        if depth > MAX_DEPTH {
            return Err(COMPLEXITY.into());
        }
        if let Some(shape) = self.local_shape(expr) {
            // O(1) sharing survives aliases. Their use/binding relationship is
            // recorded separately instead of overwriting the defining source.
            if let Some(trace) = self.node_trace.as_deref_mut() {
                let source = trace.source(AstNode::Expression(expr));
                trace.shape_alias(source, self.checked.local_use(expr), shape.trace());
            }
            return Ok(shape);
        }
        let shape = match expr {
            Expr::Int(_) | Expr::Bool(_) | Expr::Unit | Expr::Str(_) => {
                self.capture_leaf(expr, captures)?
            }
            Expr::Var(name) if self.checked.local_use(expr).is_some() => {
                let local = self.locals.get(name).ok_or(INVALID)?;
                if local.mutable
                    || local.mode != ParamMode::Value
                    || !local.initialization.fully_initialized()
                    || field_loans::conflicts(local, &[], false)
                {
                    return Err(INVALID.into());
                }
                self.capture_leaf(expr, captures)?
            }
            Expr::Var(name) if self.native.constant_id(name).is_some() => {
                self.capture_leaf(expr, captures)?
            }
            Expr::Var(name) => self.constructor_shape(expr, name, &[], captures, depth)?,
            Expr::Call(name, args) => self.constructor_shape(expr, name, args, captures, depth)?,
            Expr::Record(name, fields) => {
                let info = self.checked.record_info(expr).ok_or(INVALID)?;
                let decl = self
                    .checked
                    .program
                    .types
                    .iter()
                    .find(|d| d.name == *name)
                    .ok_or(INVALID)?;
                let names = decl.record_fields.as_ref().ok_or(INVALID)?;
                if names.len() != fields.len() || info.field_indices.len() != fields.len() {
                    return Err(INVALID.into());
                }
                let indices = info.field_indices.clone();
                let mut seen = HashSet::new();
                let mut result = Vec::new();
                let mut shape_edges = Vec::new();
                for (written, ((name, value), index)) in fields.iter().zip(indices).enumerate() {
                    if names.get(index) != Some(name) || !seen.insert(index) {
                        return Err(INVALID.into());
                    }
                    let child = self.capture_shape(value, captures, depth + 1)?;
                    if let Some(trace) = self.node_trace.as_deref_mut() {
                        let source = trace.source(AstNode::Expression(value));
                        trace.shape_edge(&mut shape_edges, source, child.trace(), written, index);
                    }
                    result.push((index, child));
                }
                let origin = self.node_trace.as_deref_mut().and_then(|trace| {
                    let source = trace.source(AstNode::Expression(expr));
                    let definition = trace.source(AstNode::TypeDeclaration(decl))?;
                    trace.shape_node(
                        source,
                        ShapeReceiptKind::Record {
                            definition,
                            fields: shape_edges,
                        },
                    )
                });
                Shape::Record {
                    nominal: decl.name.clone(),
                    height: product_height(result.iter().map(|(_, field)| field))?,
                    fields: result,
                    trace: origin,
                }
            }
            _ => return Err(INVALID.into()),
        };
        if !matches!(shape, Shape::Capture { .. }) {
            if let Some(trace) = self.node_trace.as_deref_mut() {
                if let (Some(source), Some(shape)) =
                    (trace.source(AstNode::Expression(expr)), shape.trace())
                {
                    trace.disposition(source, RawDisposition::RepresentedByShape(shape));
                }
            }
        }
        Ok(Rc::new(shape))
    }

    fn capture_leaf(
        &mut self,
        expr: &Expr,
        captures: &mut Vec<CapturedValue>,
    ) -> Result<Shape, String> {
        self.local_work.capture()?;
        let source = self.source_type(expr)?;
        let ty = self.registry.register(self.checked, &source)?;
        self.copy_dependency(ty)?;
        let value = self.expr_observed(expr)?;
        let local = self.fresh()?;
        let receipt = self.node_trace.as_deref_mut().and_then(|trace| {
            let source = trace.source(AstNode::Expression(expr));
            trace.shape_capture(source, local, ty, value.trace)
        });
        let origin = self.node_trace.as_deref_mut().and_then(|trace| {
            let source = trace.source(AstNode::Expression(expr));
            trace.shape_node(source, ShapeReceiptKind::Capture { capture: receipt? })
        });
        captures.push(CapturedValue {
            local,
            value,
            receipt,
        });
        Ok(Shape::Capture {
            local,
            ty,
            trace: origin,
        })
    }

    fn constructor_shape(
        &mut self,
        expr: &Expr,
        name: &str,
        args: &[Expr],
        captures: &mut Vec<CapturedValue>,
        depth: usize,
    ) -> Result<Shape, String> {
        let Some(InferredType::Nominal(nominal, _)) = self.checked.expression_type(expr) else {
            return Err(INVALID.into());
        };
        let decl = self
            .checked
            .program
            .types
            .iter()
            .find(|d| d.name == *nominal)
            .ok_or(INVALID)?;
        if decl.record_fields.is_some() {
            return Err(INVALID.into());
        }
        let (variant_index, variant) = decl
            .variants
            .iter()
            .enumerate()
            .find(|(_, v)| v.name == name)
            .ok_or(INVALID)?;
        if variant.fields.len() != args.len() {
            return Err(INVALID.into());
        }
        let mut fields = Vec::new();
        let mut shape_edges = Vec::new();
        for (index, arg) in args.iter().enumerate() {
            let field = self.capture_shape(arg, captures, depth + 1)?;
            if let Some(trace) = self.node_trace.as_deref_mut() {
                let source = trace.source(AstNode::Expression(arg));
                trace.shape_edge(&mut shape_edges, source, field.trace(), index, index);
            }
            fields.push(field);
        }
        let origin = self.node_trace.as_deref_mut().and_then(|trace| {
            let source = trace.source(AstNode::Expression(expr));
            let definition = trace.source(AstNode::TypeDeclaration(decl))?;
            trace.shape_node(
                source,
                ShapeReceiptKind::Constructor {
                    definition,
                    variant: variant_index,
                    fields: shape_edges,
                },
            )
        });
        Ok(Shape::Constructor {
            nominal: nominal.clone(),
            name: name.into(),
            height: product_height(fields.iter())?,
            fields,
            trace: origin,
        })
    }

    fn copy_dependency(&mut self, ty: Type) -> Result<(), String> {
        require_copy(ty, &self.registry.types, &mut self.local_work)
    }

    pub(super) fn polymorphic_use(
        &mut self,
        expr: &Expr,
        mut shape: Rc<Shape>,
    ) -> Result<LoweredExpr, String> {
        let source = self.source_type(expr)?;
        let target = self.registry.register(self.checked, &source)?;
        self.copy_dependency(target)?;
        let use_id = self.node_trace.as_deref_mut().and_then(|trace| {
            let source = trace.source(AstNode::Expression(expr));
            trace.shape_use(source, self.checked.local_use(expr), shape.trace(), target)
        });
        if let Expr::Field(_, _) = expr {
            let info = self.checked.field_info(expr).ok_or(INVALID)?;
            if info.is_empty() || info.len() > 16 {
                return Err(INVALID.into());
            }
            if let Some(trace) = self.node_trace.as_deref_mut() {
                for step in info {
                    if !trace.shape_lookup_work(self.checked.program.types.len()) {
                        break;
                    }
                    let definition = self
                        .checked
                        .program
                        .types
                        .iter()
                        .find(|decl| decl.name == step.record)
                        .and_then(|decl| trace.source(AstNode::TypeDeclaration(decl)));
                    trace.shape_projection_step(use_id, definition, step.index, step.arity);
                }
            }
            for (offset, step) in info.iter().enumerate() {
                self.local_work.charge()?;
                match &*shape {
                    Shape::Record {
                        nominal, fields, ..
                    } => {
                        if nominal != &step.record || fields.len() != step.arity {
                            return Err(INVALID.into());
                        }
                        shape = fields
                            .iter()
                            .find(|(index, _)| *index == step.index)
                            .ok_or(INVALID)?
                            .1
                            .clone();
                    }
                    Shape::Capture { local, ty, .. } => {
                        let mut ty = *ty;
                        let mut path = Vec::new();
                        for step in &info[offset..] {
                            self.local_work.charge()?;
                            let Type::Adt { id, owned: false } = ty else {
                                return Err(INVALID.into());
                            };
                            let adt = &self.registry.types[id];
                            if adt.record_fields.is_none()
                                || adt.name != step.record
                                || adt.variants[0].fields.len() != step.arity
                            {
                                return Err(INVALID.into());
                            }
                            ty = *adt.variants[0].fields.get(step.index).ok_or(INVALID)?;
                            path.push(FieldStep {
                                record: id,
                                field: step.index,
                            });
                        }
                        if ty != target {
                            return Err(INVALID.into());
                        }
                        // The environment step stays separate from all sixteen
                        // permitted source steps. This is an actual new root.
                        let capture_type = match &*shape {
                            Shape::Capture { ty, .. } => *ty,
                            _ => unreachable!(),
                        };
                        let value =
                            self.capture_value(*local, capture_type, shape.trace(), use_id)?;
                        let root = self.fresh()?;
                        let read = ExprNode {
                            ty,
                            kind: ExprKind::FieldRead(FieldPlace { root, path }),
                        };
                        let read_trace = self.node_trace.as_deref_mut().and_then(|trace| {
                            trace.direct(
                                AstNode::Expression(expr),
                                DirectRule::ProjectedUse,
                                TypedNodeRef::Expression(&read),
                                Edges::new(),
                            )
                        });
                        let capture_trace = value.trace;
                        let result = ExprNode {
                            ty,
                            kind: ExprKind::Let {
                                local: root,
                                mutable: false,
                                value: Box::new(value.value),
                                body: Box::new(read),
                            },
                        };
                        let origin = self.node_trace.as_deref_mut().and_then(|trace| {
                            let mut edges = Edges::new();
                            trace.edge(&mut edges, TypedChildRole::LetValue, capture_trace);
                            trace.edge(&mut edges, TypedChildRole::LetBody, read_trace);
                            let relations = trace.shape_relations(use_id, shape.trace(), false);
                            trace.generated(
                                GenerationReason::ProjectedCaptureTemporary,
                                relations,
                                TypedNodeRef::Expression(&result),
                                edges,
                            )
                        });
                        if let Some(trace) = self.node_trace.as_deref_mut() {
                            if let ExprKind::Let { body, .. } = &result.kind {
                                if let ExprKind::FieldRead(place) = &body.kind {
                                    trace.projected_capture(
                                        use_id,
                                        shape.trace(),
                                        offset,
                                        place,
                                        capture_trace,
                                        read_trace,
                                        origin,
                                    );
                                }
                            }
                            trace.finish_shape_use(use_id, shape.trace(), origin);
                        }
                        return Ok(LoweredExpr {
                            value: result,
                            trace: origin,
                        });
                    }
                    _ => return Err(INVALID.into()),
                }
            }
        } else if !matches!(expr, Expr::Var(_)) {
            return Err(INVALID.into());
        }
        let result = self.materialize(&shape, target, 0, use_id)?;
        if let Some(trace) = self.node_trace.as_deref_mut() {
            trace.finish_shape_use(use_id, shape.trace(), result.trace);
        }
        Ok(result)
    }

    fn materialize(
        &mut self,
        shape: &Shape,
        target: Type,
        depth: usize,
        use_id: Option<usize>,
    ) -> Result<LoweredExpr, String> {
        self.local_work.charge()?;
        if depth > MAX_DEPTH {
            return Err(COMPLEXITY.into());
        }
        self.copy_dependency(target)?;
        let mut edges = Edges::new();
        let mut children = Vec::new();
        let kind = match shape {
            Shape::Capture { local, ty, .. } => {
                if *ty != target {
                    return Err(INVALID.into());
                }
                let result = self.capture_value(*local, target, shape.trace(), use_id)?;
                if let Some(trace) = self.node_trace.as_deref_mut() {
                    trace.materialized_shape(use_id, shape.trace(), target, result.trace, &[]);
                }
                return Ok(result);
            }
            Shape::Constructor {
                nominal,
                name,
                fields,
                ..
            } => {
                let Type::Adt { id, owned: false } = target else {
                    return Err(INVALID.into());
                };
                let adt = &self.registry.types[id];
                if adt.name != *nominal || adt.record_fields.is_some() {
                    return Err(INVALID.into());
                }
                let variant = adt
                    .variants
                    .iter()
                    .position(|v| v.name == *name)
                    .ok_or(INVALID)?;
                let expected = adt.variants[variant].fields.clone();
                if fields.len() != expected.len() {
                    return Err(INVALID.into());
                }
                let mut values = Vec::new();
                for (index, (field, expected)) in fields.iter().zip(expected).enumerate() {
                    let value = self.materialize(field, expected, depth + 1, use_id)?;
                    if let Some(trace) = self.node_trace.as_deref_mut() {
                        trace.edge(
                            &mut edges,
                            TypedChildRole::ConstructorField(index),
                            value.trace,
                        );
                        trace.shape_output_edge(&mut children, value.trace);
                    }
                    values.push(value.value);
                }
                ExprKind::Constructor {
                    adt: id,
                    variant,
                    fields: values,
                }
            }
            Shape::Record {
                nominal, fields, ..
            } => {
                let Type::Adt { id, owned: false } = target else {
                    return Err(INVALID.into());
                };
                let adt = &self.registry.types[id];
                if adt.name != *nominal || adt.record_fields.is_none() {
                    return Err(INVALID.into());
                }
                let expected = adt.variants[0].fields.clone();
                if fields.len() != expected.len() {
                    return Err(INVALID.into());
                }
                let mut values = Vec::new();
                let mut seen = HashSet::new();
                for (written, (index, field)) in fields.iter().enumerate() {
                    if !seen.insert(index) {
                        return Err(INVALID.into());
                    }
                    let ty = *expected.get(*index).ok_or(INVALID)?;
                    let value = self.materialize(field, ty, depth + 1, use_id)?;
                    if let Some(trace) = self.node_trace.as_deref_mut() {
                        let mut field_edges = Edges::new();
                        trace.edge(&mut field_edges, TypedChildRole::RecordValue, value.trace);
                        let relations = trace.shape_relations(use_id, shape.trace(), false);
                        let field_trace = trace.generated(
                            GenerationReason::ShapeRecordField,
                            relations,
                            TypedNodeRef::RecordField {
                                written,
                                declaration: *index,
                                value: &value.value,
                            },
                            field_edges,
                        );
                        trace.edge(
                            &mut edges,
                            TypedChildRole::RecordField(written),
                            field_trace,
                        );
                        trace.shape_output_edge(&mut children, value.trace);
                    }
                    values.push((*index, value.value));
                }
                ExprKind::Record {
                    adt: id,
                    fields: values,
                }
            }
        };
        let value = ExprNode { ty: target, kind };
        let origin = self.node_trace.as_deref_mut().and_then(|trace| {
            let relations = trace.shape_relations(use_id, shape.trace(), false);
            trace.generated(
                GenerationReason::ShapeMaterialization,
                relations,
                TypedNodeRef::Expression(&value),
                edges,
            )
        });
        if let Some(trace) = self.node_trace.as_deref_mut() {
            trace.materialized_shape(use_id, shape.trace(), target, origin, &children);
        }
        Ok(LoweredExpr {
            value,
            trace: origin,
        })
    }
}

/// A capture product is explicitly compiler-generated and has no source
/// declaration identity. Its payloads and identity arguments are exactly equal.
pub(super) fn verify_capture_definition(adt: &AdtType, types: &[AdtType]) -> Result<(), String> {
    if adt.origin == AdtOrigin::Source {
        if adt.name == CAPTURE_NAME {
            return Err(INVALID.into());
        }
        return Ok(());
    }
    let Some(names) = &adt.record_fields else {
        return Err(INVALID.into());
    };
    if adt.name != CAPTURE_NAME
        || adt.owned
        || adt.arguments.is_empty()
        || adt.arguments.len() > MAX_CAPTURES
        || adt.variants.len() != 1
        || adt.variants[0].name != CAPTURE_NAME
        || adt.variants[0].fields != adt.arguments
        || names.len() != adt.arguments.len()
        || names
            .iter()
            .enumerate()
            .any(|(index, name)| *name != format!("capture_{index}"))
    {
        return Err(INVALID.into());
    }
    let mut budget = Budget::default();
    for &ty in &adt.arguments {
        require_copy(ty, types, &mut budget)?;
    }
    Ok(())
}

fn require_copy(ty: Type, types: &[AdtType], budget: &mut Budget) -> Result<(), String> {
    // Pay for every queued edge before growing the worklist, including the
    // initial root. Cached/duplicate visits still consume that reserved unit.
    budget.charge()?;
    let mut pending = vec![ty];
    let mut seen = HashSet::new();
    while let Some(ty) = pending.pop() {
        if budget.copy_types.contains(&ty) || !seen.insert(ty) {
            continue;
        }
        if ty.is_owner() {
            return Err("E-TYPED-LOCAL: owner-dependent instantiation is unsupported".into());
        }
        if let Type::Adt { id, .. } = ty {
            let adt = types.get(id).ok_or(INVALID)?;
            if adt.owned {
                return Err(INVALID.into());
            }
            budget.charge_many(adt.variants.len())?;
            let children = adt
                .variants
                .iter()
                .try_fold(adt.arguments.len(), |count, variant| {
                    count.checked_add(variant.fields.len()).ok_or(COMPLEXITY)
                })?;
            budget.charge_many(children)?;
            pending.extend(adt.arguments.iter().copied());
            // A nominal field can hide owner-valued phantom arguments even
            // when neither the root's arguments nor its physical layout owns.
            pending.extend(
                adt.variants
                    .iter()
                    .flat_map(|variant| &variant.fields)
                    .copied(),
            );
        }
    }
    budget.copy_types.extend(seen);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{check, parser};

    #[test]
    fn work_and_capture_boundaries_are_exact() {
        let mut work = Budget::default();
        for _ in 0..MAX_WORK {
            work.charge().unwrap();
        }
        assert_eq!(work.remaining, 0);
        assert_eq!(work.charge().unwrap_err(), COMPLEXITY);
        assert_eq!(work.charge().unwrap_err(), COMPLEXITY);
        let mut captures = Budget::default();
        for _ in 0..MAX_CAPTURES {
            captures.capture().unwrap();
        }
        assert_eq!(captures.capture().unwrap_err(), COMPLEXITY);
    }

    #[test]
    fn copy_dependency_reserves_children_before_worklist_growth() {
        let types = vec![AdtType {
            origin: AdtOrigin::Source,
            name: "Wide".into(),
            arguments: vec![],
            owned: false,
            record_fields: None,
            variants: vec![AdtVariant {
                name: "Wide".into(),
                fields: vec![Type::Int; 3],
            }],
        }];
        let root = Type::Adt {
            id: 0,
            owned: false,
        };
        let mut exact = Budget {
            remaining: 5,
            ..Budget::default()
        };
        require_copy(root, &types, &mut exact).unwrap();
        assert_eq!(exact.remaining, 0); // root + variant metadata + three edges
        let mut short = Budget {
            remaining: 4,
            ..Budget::default()
        };
        assert_eq!(
            require_copy(root, &types, &mut short).unwrap_err(),
            COMPLEXITY
        );
        assert_eq!(
            short.remaining, 2,
            "failed batch is never partly queued or charged"
        );
        assert!(short.copy_types.is_empty());
    }

    #[test]
    fn shared_shape_height_is_bounded_without_expanding_the_dag() {
        let mut shape = Rc::new(Shape::Capture {
            local: LocalId(0),
            ty: Type::Int,
            trace: None,
        });
        for _ in 0..MAX_DEPTH {
            let fields = vec![shape.clone(), shape.clone()];
            let height = product_height(fields.iter()).unwrap();
            shape = Rc::new(Shape::Constructor {
                nominal: "N".into(),
                name: "N".into(),
                fields,
                height,
                trace: None,
            });
        }
        assert_eq!(shape.height(), MAX_DEPTH);
        assert_eq!(
            product_height([&shape].into_iter()).unwrap_err(),
            COMPLEXITY
        );
    }

    #[test]
    fn repeated_dag_uses_charge_before_materializing_each_concrete_copy() {
        let mut declarations = "type D0<a> = Leaf\n".to_string();
        let mut bindings = "let d0 = Leaf;".to_string();
        for level in 1..=12 {
            let previous = level - 1;
            declarations.push_str(&format!(
                "type D{level}<a> = Fork{level}(D{previous}<a>,D{previous}<a>)\n"
            ));
            bindings.push_str(&format!(
                "let d{level} = Fork{level}(d{previous},d{previous});"
            ));
        }
        declarations.push_str("fn take(x: D12<i32>) { 0 }\n");
        for uses in [6, 7] {
            let source = format!(
                "{declarations}fn main() {{ {bindings} {} }}",
                vec!["take(d12)"; uses].join(" + ")
            );
            let p = parser::parse(&source).unwrap();
            let checked = check::analyze(&p).unwrap();
            if uses == 6 {
                lower_main(&checked).unwrap();
            } else {
                assert_eq!(lower_main(&checked).unwrap_err(), COMPLEXITY);
            }
        }
    }
}
