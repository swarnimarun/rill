//! A provisional typed IR for closed scalar, static-string, fixed byte-buffer/
//! vector, finite enum/record and Copy native-transport programs, plus bounded
//! finite Copy products with frozen logical phantom arguments.
//!
//! This is not the verified MIR in PLAN.md. Lowering requires an existing
//! checked program. The C backend uses it for supported forms; owned-box
//! programs never fall back to the copying AST evaluator. Generic source
//! functions specialize to exact logical signatures with verified runtime layouts.
//! Programs outside this narrow subset are rejected without approximation.
mod adt_eval;
mod adt_lower;
#[cfg(test)]
mod adt_tests;
mod adt_verify;
mod adts;
mod allocation;
mod control;
mod control_lower;
#[cfg(test)]
mod control_tests;
mod copy_loans;
#[cfg(test)]
mod copy_loans_tests;
mod copy_match;
#[cfg(test)]
mod independent_nonreturning_tests;
use copy_loans::{CopyLocals, CopySlot};
#[cfg(test)]
mod copy_match_tests;
mod foreign;
use foreign::NativeManifest;
pub use foreign::{
    ForeignArg, ForeignConstantId, ForeignConvert, ForeignFunctionId, PointerTypeId,
};
mod field_loans;
#[cfg(test)]
mod field_loans_tests;
#[cfg(test)]
mod foreign_tests;
#[cfg(test)]
mod independent_phantom_product_tests;
mod initialization;
#[cfg(test)]
mod initialization_tests;
mod instances;
mod logical;
#[cfg(test)]
mod logical_tests;
pub use logical::{LogicalType, OpaqueId, SymbolicInstance};
mod discarded;
mod places;
mod results;
pub use discarded::{DiscardArm, DiscardExpr, DiscardKind};
pub use results::{
    Computation, InterruptedCall, NoNormalArm, NoNormalExpr, NoNormalKind, NoStorageBinding,
    ResultRepr,
};
pub mod provenance;
pub use initialization::{Cleanup, CleanupTree, Initialization};
pub use places::Place;
use provenance::nodes::{self, DirectRule, LoweredExpr, TypedChildRole as NodeRole, TypedNodeRef};
mod local_values;
#[cfg(test)]
mod local_values_tests;
mod records;
mod vectors;
#[cfg(test)]
mod vectors_tests;
pub use vectors::VectorOp;
#[cfg(test)]
mod records_tests;
use allocation::AllocationFailure;
pub use allocation::{AllocationContract, AllocationResultLayout, SystemAllocationKind};
mod c;
pub use c::emit_c;

use std::collections::{HashMap, HashSet};

use crate::ast::{BinaryOp, Expr, ParamMode};
use crate::check::{CheckedProgram, Type as InferredType};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LocalId(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    Int,
    Bool,
    Unit,
    StaticStr,
    CU8,
    CU32,
    CU64,
    CF32,
    RawPointer(PointerTypeId),
    U8x16,
    Bytes(usize),
    OwnedI32,
    OwnedBytes,
    Adt { id: usize, owned: bool },
}

impl Type {
    fn is_raw(self) -> bool {
        matches!(
            self,
            Self::CU8 | Self::CU32 | Self::CU64 | Self::CF32 | Self::RawPointer(_)
        )
    }

    fn is_owner(self) -> bool {
        matches!(
            self,
            Self::OwnedI32 | Self::OwnedBytes | Self::Adt { owned: true, .. }
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AdtOrigin {
    Source,
    /// Compiler-generated storage for concrete leaves of a local value shape.
    LocalCapture,
}

#[derive(Clone, Debug)]
pub struct AdtType {
    /// Full logical arguments of a proved finite Copy open nominal. Closed layouts
    /// retain `arguments`; opaque identities never become runtime `Type`s.
    pub symbolic: Option<SymbolicInstance>,
    pub origin: AdtOrigin,
    /// Named record product, never a positional enum; one variant stores field types.
    pub record_fields: Option<Vec<String>>,
    pub name: String,
    pub arguments: Vec<Type>,
    pub variants: Vec<AdtVariant>,
    pub owned: bool,
}
#[derive(Clone, Debug)]
pub struct AdtVariant {
    pub name: String,
    pub fields: Vec<Type>,
}
#[derive(Clone, Debug)]
pub struct AdtArm {
    pub variant: Option<usize>,
    pub fields: Vec<Option<LocalId>>,
    pub body: ExprNode,
    pub cleanup: Vec<Cleanup>,
}

/// A non-consuming pattern. Its input type comes from the enclosing match or
/// constructor field, and is independently checked before any projection.
#[derive(Clone, Debug)]
pub enum CopyPattern {
    Wildcard,
    Binding(LocalId),
    Int(i32),
    Bool(bool),
    Unit,
    Constructor {
        adt: usize,
        variant: usize,
        fields: Vec<CopyPattern>,
    },
}

#[derive(Clone, Debug)]
pub struct CopyArm {
    pub pattern: CopyPattern,
    pub body: ExprNode,
}

#[derive(Clone, Debug)]
pub struct ExprNode {
    pub ty: Type,
    pub kind: ExprKind,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    InterruptedCall(Box<InterruptedCall>),
    DiscardLet {
        binding: NoStorageBinding,
        value: Box<DiscardExpr>,
        body: Box<ExprNode>,
    },
    NoStorageLet {
        binding: NoStorageBinding,
        value: Box<NoNormalExpr>,
        body: Box<ExprNode>,
    },
    TryAlloc {
        kind: SystemAllocationKind,
        input: Box<ExprNode>,
        layout: AllocationResultLayout,
    },
    Int(i32),
    Str(String),
    Bool(bool),
    Unit,
    Read(LocalId),
    Record {
        adt: usize,
        fields: Vec<(usize, ExprNode)>,
    },
    FieldRead(FieldPlace),
    FieldAssign(FieldPlace, Box<ExprNode>),
    Call(usize, Vec<CallArg>),
    ForeignCall(ForeignFunctionId, Vec<ForeignArg>),
    ForeignConst(ForeignConstantId),
    ForeignConvert(ForeignConvert, Box<ExprNode>),
    NullPointer,
    IsNull(Box<ExprNode>),
    Constructor {
        adt: usize,
        variant: usize,
        fields: Vec<ExprNode>,
    },
    Match {
        value: Box<ExprNode>,
        arms: Vec<AdtArm>,
    },
    CopyMatch {
        value: Box<ExprNode>,
        arms: Vec<CopyArm>,
    },
    ByteLen(Box<ExprNode>),
    ByteAt(Box<ExprNode>, Box<ExprNode>),
    BytesNew(usize),
    Vector(VectorOp, Vec<ExprNode>),
    BytesLen(Box<ExprNode>),
    BytesGet(Box<ExprNode>, Box<ExprNode>),
    BytesSet(Box<ExprNode>, Box<ExprNode>, Box<ExprNode>),
    OwnerLet {
        local: LocalId,
        mutable: bool,
        init: OwnerInit,
        body: Box<ExprNode>,
        cleanup: Option<Cleanup>,
    },
    OwnerBytesLen(LocalId),
    OwnerBytesGet(LocalId, Box<ExprNode>),
    OwnerBytesSet(LocalId, Box<ExprNode>, Box<ExprNode>),
    OwnerRead(LocalId),
    OwnerMove(LocalId),
    OwnerWrite(LocalId, Box<ExprNode>),
    Let {
        local: LocalId,
        mutable: bool,
        value: Box<ExprNode>,
        body: Box<ExprNode>,
    },
    Assign(LocalId, Box<ExprNode>),
    Binary(BinaryOp, Box<ExprNode>, Box<ExprNode>),
    If(Box<ExprNode>, Box<ExprNode>, Box<ExprNode>),
    While(Box<ExprNode>, Box<ExprNode>),
}

/// One nominal record step, not a byte offset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldStep {
    pub record: usize,
    pub field: usize,
}
/// A root slot followed by one to sixteen record-field steps.
#[derive(Clone, Debug)]
pub struct FieldPlace {
    pub root: LocalId,
    pub path: Vec<FieldStep>,
}

#[derive(Clone, Debug)]
pub enum CallArg {
    Value(ExprNode),
    ReadCopy(LocalId),
    InoutCopy(LocalId),
    ReadTemporary(ExprNode),
    ReadField(FieldPlace),
    InoutField(FieldPlace),
    Move(LocalId),
    ReadOwner(LocalId),
    InoutOwner(LocalId),
}

#[derive(Clone, Debug)]
pub enum OwnerInit {
    Alloc(Box<ExprNode>),
    BytesAlloc(Box<ExprNode>),
    Move(LocalId),
    Call(Box<ExprNode>),
    Expr(Box<ExprNode>),
}

#[derive(Clone, Debug)]
pub struct TypedMain {
    logical: logical::Metadata,
    native: NativeManifest,
    allocation_contract: Option<AllocationContract>,
    adts: Vec<AdtType>,
    functions: Vec<TypedFunction>,
    entry: usize,
}

#[derive(Clone, Debug)]
struct TypedFunction {
    params: Vec<(LocalId, Type, ParamMode)>,
    result: ResultRepr,
    body: Computation,
    param_cleanup: Vec<Cleanup>,
}

fn primitive_type(ty: &InferredType) -> Result<Type, String> {
    match ty {
        InferredType::Int => Ok(Type::Int),
        InferredType::Bool => Ok(Type::Bool),
        InferredType::Unit => Ok(Type::Unit),
        InferredType::StaticStr => Ok(Type::StaticStr),
        InferredType::CU8 => Ok(Type::CU8),
        InferredType::CU32 => Ok(Type::CU32),
        InferredType::CU64 => Ok(Type::CU64),
        InferredType::CF32 => Ok(Type::CF32),
        InferredType::U8x16 => Ok(Type::U8x16),
        InferredType::Bytes(n) if *n <= 256 => Ok(Type::Bytes(*n)),
        InferredType::SystemBoxI32 => Ok(Type::OwnedI32),
        InferredType::SystemBytes => Ok(Type::OwnedBytes),
        _ => Err("E-TYPED-UNSUPPORTED: nonprimitive or unresolved type".into()),
    }
}

#[derive(Clone)]
struct Local {
    id: LocalId,
    ty: Type,
    mutable: bool,
    initialization: Initialization,
    loaned: bool,
    shared: usize,
    field_loans: Vec<field_loans::ProjectionLoan>,
    mode: ParamMode,
}

/// Instantiate reachable functions and lower the supported scalar, string,
/// buffer/vector and finite nominal subset to independently checked operations.
/// The C backend uses this result for supported programs; other checked
/// programs use the existing AST backend.
pub fn lower_main(checked: &CheckedProgram<'_>) -> Result<TypedMain, String> {
    lower_main_traced(checked, None, None).map(|(typed, _)| typed)
}

// One lowering path; optional discovery metadata cannot change semantic errors.
fn lower_main_traced<'source>(
    checked: &CheckedProgram<'source>,
    trace: Option<&mut instances::Trace>,
    body_trace: Option<&mut nodes::Trace<'source>>,
) -> Result<(TypedMain, instances::Instances), String> {
    let program = checked.program;
    let native = NativeManifest::from_checked(checked)?;
    if checked.has_owned() && (native.has_foreign() || checked.has_c_values()) {
        return Err(
            "E-TYPED-OWNER: owner programs cannot use foreign declarations or values".into(),
        );
    }
    let mut control = None;
    let instances = match instances::Instances::discover_traced(checked, trace) {
        Ok(instances) => instances,
        Err(error)
            if error == instances::BARE_RESULT || error == logical::layout::UNREPRESENTABLE =>
        {
            let facts = control::SourceFacts::build(checked)?;
            let instances = instances::Instances::discover_control(checked, &facts)?;
            control = Some(facts);
            instances
        }
        Err(error) => return Err(error),
    };
    let registry = if control.is_some() {
        adts::Registry::build_control(checked, &instances, &native, control.as_ref())?
    } else {
        adts::Registry::build(checked, &instances, &native)?
    };
    if let Some(control) = &control {
        control.preflight_lower(&instances, &registry.types)?;
    }
    instances.charge_control(program.functions.len())?;
    for function in &program.functions {
        instances.charge_control(function.name.len().saturating_add(function.params.len()))?;
    }
    let mut lower = Lower {
        native: &native,
        node_trace: if control.is_none() { body_trace } else { None },
        control: control.as_ref(),
        registry,
        checked,
        locals: HashMap::new(),
        shapes: HashMap::new(),
        capture_places: HashMap::new(),
        local_work: local_values::Budget::default(),
        functions: &instances.functions,
        instances: &instances,
        substitution: &instances.list[instances.entry].substitution,
        function_modes: program
            .functions
            .iter()
            .map(|f| (f.name.clone(), f.params.iter().map(|p| p.mode).collect()))
            .collect(),
        next: 0,
    };
    let mut result = TypedMain {
        logical: Default::default(),
        native: native.clone(),
        allocation_contract: program
            .allocation_types
            .as_ref()
            .map(AllocationContract::from),
        adts: Vec::new(),
        functions: Vec::new(),
        entry: instances.entry,
    };
    for (instance_slot, instance) in instances.list.iter().enumerate() {
        if let Some(trace) = lower.node_trace.as_deref_mut() {
            trace.set_instance(instance_slot);
        }
        let function = &program.functions[instance.source];
        lower.substitution = &instance.substitution;
        let result_type = if instance.demand == instances::Demand::Discard {
            ResultRepr::Discarded(
                lower
                    .registry
                    .logical
                    .logical_type(&instance.result, &mut *instances.control_budget()?)?,
            )
        } else {
            match instance.result {
                InferredType::Var(index) if control.is_some() => {
                    ResultRepr::NoNormal(lower.registry.logical.opaque_id(index)?)
                }
                _ => ResultRepr::Value(lower.registry.convert(&instance.result)?),
            }
        };
        let param_types = instance
            .params
            .iter()
            .map(|ty| lower.registry.convert(ty))
            .collect::<Result<Vec<_>, _>>()?;
        if param_types.len() != function.params.len() {
            return Err("E-TYPED: invalid function signature".into());
        }
        lower.locals.clear();
        lower.shapes.clear();
        lower.capture_places.clear();
        lower.local_work = local_values::Budget::default();
        let mut params = Vec::new();
        for (param, ty) in function.params.iter().zip(param_types) {
            let id = lower.fresh()?;
            if let Some(trace) = lower.node_trace.as_deref_mut() {
                trace.bind(crate::source_map::AstNode::Parameter(param), id);
            }
            lower.locals.insert(
                param.name.clone(),
                Local {
                    id,
                    ty,
                    mutable: param.mode == ParamMode::Inout,
                    initialization: Initialization::full(ty, &lower.registry.types)?,
                    loaned: false,
                    shared: 0,
                    field_loans: Vec::new(),
                    mode: param.mode,
                },
            );
            params.push((id, ty, param.mode));
        }
        let (body, body_fragment) = match &result_type {
            ResultRepr::Discarded(_) => {
                (Computation::Discarded(lower.discard(&function.body)?), None)
            }
            ResultRepr::Value(_) => {
                let lowered = lower.expr_observed(&function.body)?;
                (Computation::Value(lowered.value), lowered.trace)
            }
            ResultRepr::NoNormal(_) => (
                Computation::NoNormal(lower.no_normal(&function.body)?),
                None,
            ),
        };
        let mut param_cleanup = Vec::new();
        for (param, (id, ty, mode)) in function.params.iter().zip(&params).rev() {
            if ty.is_owner() && *mode == ParamMode::Value {
                let local = lower
                    .locals
                    .get(&param.name)
                    .filter(|local| local.id == *id)
                    .ok_or("E-TYPED-OWNER: lost parameter state")?;
                if let Some(cleanup) =
                    local
                        .initialization
                        .cleanup_entry(*id, *ty, &lower.registry.types)?
                {
                    param_cleanup.push(cleanup);
                }
            }
        }
        if let Some(trace) = lower.node_trace.as_deref_mut() {
            trace.signature(
                function,
                &params,
                result_type.value_ref()?,
                body.value_expr()?,
                body_fragment,
            );
        }
        result.functions.push(TypedFunction {
            params,
            result: result_type,
            body,
            param_cleanup,
        });
    }
    result.logical = lower.registry.logical;
    result.native.pointers = lower.registry.pointers;
    result.adts = lower.registry.types;
    if let Some(control) = control {
        let proof = control.freeze(&instances)?;
        proof.verify_source(checked)?;
        result.logical.control = Some(std::sync::Arc::new(proof));
    }
    verify(&result)?;
    Ok((result, instances))
}

struct Lower<'a, 'b> {
    control: Option<&'a control::SourceFacts<'a, 'b>>,
    node_trace: Option<&'a mut nodes::Trace<'b>>,
    native: &'a NativeManifest,
    registry: adts::Registry,
    checked: &'a CheckedProgram<'b>,
    locals: HashMap<String, Local>,
    shapes: HashMap<usize, std::rc::Rc<local_values::Shape>>,
    capture_places: HashMap<LocalId, FieldPlace>,
    local_work: local_values::Budget,
    functions: &'a HashMap<String, usize>,
    instances: &'a instances::Instances,
    substitution: &'a instances::Substitution,
    function_modes: HashMap<String, Vec<ParamMode>>,
    next: u32,
}
impl Lower<'_, '_> {
    fn fresh(&mut self) -> Result<LocalId, String> {
        let id = LocalId(self.next);
        self.next = self.next.checked_add(1).ok_or("E-TYPED: too many locals")?;
        Ok(id)
    }
    fn source_type(&self, expr: &Expr) -> Result<InferredType, String> {
        self.instances
            .expression_result_type(self.checked, expr, self.substitution, self.control)
    }
    fn call_arguments(
        &mut self,
        expr: &Expr,
        name: &str,
        args: &[Expr],
    ) -> Result<(usize, Vec<CallArg>), String> {
        self.call_arguments_demand(expr, name, args, instances::Demand::Observe)
    }
    fn call_arguments_demand(
        &mut self,
        expr: &Expr,
        name: &str,
        args: &[Expr],
        demand: instances::Demand,
    ) -> Result<(usize, Vec<CallArg>), String> {
        self.instances
            .charge_control(args.len().saturating_mul(3))?;
        let param_types = args
            .iter()
            .map(|arg| self.source_type(arg))
            .collect::<Result<Vec<_>, _>>()?;
        for ty in &param_types {
            self.instances.reserve_control_type(ty, 1)?;
        }
        let id = self.instances.call_demand(
            name,
            param_types.clone(),
            self.instances.expression_type_demand(
                self.checked,
                expr,
                self.substitution,
                self.control,
                demand,
            )?,
            demand,
        )?;
        let modes = self
            .function_modes
            .get(name)
            .ok_or("E-TYPED: missing function modes")?
            .clone();
        if modes.len() != args.len() {
            return Err("E-TYPED: invalid function modes".into());
        }
        let mut lowered = Vec::new();
        let loans: Vec<_> = self
            .locals
            .iter()
            .map(|(name, local)| {
                (
                    name.clone(),
                    local.id,
                    local.shared,
                    local.loaned,
                    local.field_loans.clone(),
                )
            })
            .collect();
        for (ordinal, ((arg, ty), mode)) in args.iter().zip(&param_types).zip(modes).enumerate() {
            if self.registry.convert(ty)?.is_owner() {
                let source = match (mode, arg) {
                    (ParamMode::Value, Expr::Move(source))
                    | (ParamMode::Read, Expr::Var(source))
                    | (ParamMode::Inout, Expr::Inout(source)) => source,
                    _ => return Err("E-TYPED-OWNER: wrong owner argument mode".into()),
                };
                let local = self
                    .locals
                    .get_mut(source)
                    .ok_or("E-TYPED-OWNER: unknown owner root")?;
                if local.ty != self.registry.convert(ty)?
                    || !local.initialization.fully_initialized()
                {
                    return Err("E-TYPED-OWNER: invalid owner root".into());
                }
                match mode {
                    ParamMode::Value => {
                        if local.mode != ParamMode::Value || local.loaned || local.shared != 0 {
                            return Err("E-TYPED-OWNER: invalid move".into());
                        }
                        places::take(local, &[], &self.registry.types)?;
                        lowered.push(CallArg::Move(local.id));
                    }
                    ParamMode::Read => {
                        if local.loaned {
                            return Err("E-TYPED-OWNER: overlapping loan".into());
                        }
                        local.shared = local
                            .shared
                            .checked_add(1)
                            .ok_or("E-TYPED-OWNER: too many shared loans")?;
                        lowered.push(CallArg::ReadOwner(local.id));
                    }
                    ParamMode::Inout => {
                        if !local.mutable || local.loaned || local.shared != 0 {
                            return Err("E-TYPED-OWNER: overlapping loan".into());
                        }
                        local.loaned = true;
                        lowered.push(CallArg::InoutOwner(local.id));
                    }
                }
            } else {
                let ty = self.registry.convert(ty)?;
                let argument = self.copy_argument_observed(arg, ty, mode)?;
                if let Some(trace) = self.node_trace.as_deref_mut() {
                    trace.attach(NodeRole::CallArgument(ordinal), argument.1);
                }
                lowered.push(argument.0);
            }
        }
        for (name, id, shared, loaned, fields) in loans {
            let local = self
                .locals
                .get_mut(&name)
                .filter(|local| local.id == id)
                .ok_or("E-TYPED-LOAN: lost loan scope")?;
            local.shared = shared;
            local.loaned = loaned;
            local.field_loans = fields;
        }
        Ok((id, lowered))
    }
    fn expr(&mut self, expr: &Expr) -> Result<ExprNode, String> {
        self.expr_observed(expr).map(|lowered| lowered.value)
    }
    fn expr_child(&mut self, expr: &Expr, role: NodeRole) -> Result<ExprNode, String> {
        let lowered = self.expr_observed(expr)?;
        if let Some(trace) = self.node_trace.as_deref_mut() {
            trace.attach(role, lowered.trace);
        }
        Ok(lowered.value)
    }
    fn expr_observed(&mut self, expr: &Expr) -> Result<LoweredExpr, String> {
        if let Some(id) = self.checked.polymorphic_local(expr) {
            return self.polymorphic_let(expr, id);
        }
        if let Some(shape) = self.local_shape(expr) {
            return self.polymorphic_use(expr, shape);
        }
        let frame = self
            .node_trace
            .as_deref_mut()
            .and_then(|trace| trace.begin());
        let value = self.expr_direct(expr)?;
        let fragment = self
            .node_trace
            .as_deref_mut()
            .and_then(|trace| trace.finish_expression(frame, expr, &value));
        Ok(LoweredExpr {
            value,
            trace: fragment,
        })
    }
    fn expr_direct(&mut self, expr: &Expr) -> Result<ExprNode, String> {
        let ty = self.registry.convert(&self.source_type(expr)?)?;
        if let Some(call) = self.interrupted_call(expr)? {
            return Ok(ExprNode {
                ty,
                kind: ExprKind::InterruptedCall(Box::new(call)),
            });
        }
        let kind = match expr {
            Expr::Call(name, args) if self.native.function_id(name).is_some() => {
                self.foreign_call(self.native.function_id(name).unwrap(), args)?
            }
            Expr::Var(name)
                if !self.locals.contains_key(name) && self.native.constant_id(name).is_some() =>
            {
                ExprKind::ForeignConst(self.native.constant_id(name).unwrap())
            }
            Expr::Call(name, args)
                if ForeignConvert::from_name(name).is_some() && args.len() == 1 =>
            {
                ExprKind::ForeignConvert(
                    ForeignConvert::from_name(name).unwrap(),
                    Box::new(self.expr(&args[0])?),
                )
            }
            Expr::Call(name, args) if name == "c_null" && args.is_empty() => ExprKind::NullPointer,
            Expr::Call(name, args) if name == "is_null" && args.len() == 1 => {
                ExprKind::IsNull(Box::new(self.expr(&args[0])?))
            }
            Expr::Record(_, fields) => self.record(expr, ty, fields)?,
            Expr::Field(root, _) => ExprKind::FieldRead(self.field_place(expr, root, false)?),
            Expr::FieldAssign(root, _, value) => {
                let place = self.field_place(expr, root, true)?;
                ExprKind::FieldAssign(
                    place,
                    Box::new(self.expr_child(value, NodeRole::AssignmentValue)?),
                )
            }
            Expr::Call(name, args)
                if matches!(name.as_str(), "system_box_i32" | "system_bytes_zeroed")
                    && args.len() == 1 =>
            {
                let kind = if name == "system_box_i32" {
                    SystemAllocationKind::BoxI32
                } else {
                    SystemAllocationKind::BytesZeroed
                };
                let contract = self
                    .checked
                    .program
                    .allocation_types
                    .as_ref()
                    .map(AllocationContract::from)
                    .ok_or("E-TYPED-ALLOC: missing canonical allocation contract")?;
                let layout = allocation::resolve(ty, &self.registry.types, &contract)?;
                ExprKind::TryAlloc {
                    kind,
                    input: Box::new(self.expr(&args[0])?),
                    layout,
                }
            }
            Expr::Call(name, args) if matches!(ty,Type::Adt{id,..} if self.registry.types[id].variants.iter().any(|v|v.name==*name)) => {
                self.constructor(ty, name, args)?
            }
            Expr::Var(name)
                if !self.locals.contains_key(name) && matches!(ty, Type::Adt { .. }) =>
            {
                self.constructor(ty, name, &[])?
            }
            Expr::Match(value, arms)
                if self.source_type(value).is_ok_and(|ty| {
                    matches!(
                        self.registry.convert(&ty),
                        Ok(Type::Adt { owned: true, .. })
                    )
                }) =>
            {
                self.match_adt(value, arms)?
            }
            Expr::Match(value, arms) => self.match_copy(value, arms)?,
            Expr::Int(n) => ExprKind::Int(*n),
            Expr::Str(value) => ExprKind::Str(value.clone()),
            Expr::Bool(b) => ExprKind::Bool(*b),
            Expr::Unit => ExprKind::Unit,
            Expr::Move(name) => {
                let local = self
                    .locals
                    .get_mut(name)
                    .ok_or("E-TYPED: unknown move source")?;
                if !local.ty.is_owner()
                    || local.mode != ParamMode::Value
                    || !local.initialization.fully_initialized()
                    || local.loaned
                    || local.shared != 0
                {
                    return Err("E-TYPED-OWNER: invalid returned move".into());
                }
                places::take(local, &[], &self.registry.types)?;
                ExprKind::OwnerMove(local.id)
            }
            Expr::Var(name) => {
                let local = self.locals.get(name).ok_or("E-TYPED: missing local")?;
                if local.ty.is_owner()
                    || !local.initialization.fully_initialized()
                    || field_loans::conflicts(local, &[], false)
                {
                    return Err("E-TYPED: cannot copy an owner".into());
                }
                ExprKind::Read(local.id)
            }
            Expr::Let(name, value, body) | Expr::LetMut(name, value, body) => {
                if self.instances.initializer_demand(
                    self.checked,
                    expr,
                    self.substitution,
                    self.control,
                )? == instances::Demand::Discard
                {
                    let binding = self
                        .control
                        .ok_or(instances::BARE_RESULT)?
                        .discard_binding(expr, &self.instances.domain)?;
                    return Ok(ExprNode {
                        ty,
                        kind: ExprKind::DiscardLet {
                            binding,
                            value: Box::new(self.discard(value)?),
                            body: Box::new(self.expr(body)?),
                        },
                    });
                }
                let source_type = self.source_type(value)?;
                if matches!(source_type, InferredType::Var(_)) {
                    let binding = self
                        .control
                        .ok_or(instances::BARE_RESULT)?
                        .no_storage_binding(expr, &self.instances.domain)?;
                    return Ok(ExprNode {
                        ty,
                        kind: ExprKind::NoStorageLet {
                            binding,
                            value: Box::new(self.no_normal(value)?),
                            body: Box::new(self.expr(body)?),
                        },
                    });
                }
                let owner_type = self.registry.convert(&source_type)?;
                let is_owner = owner_type.is_owner();
                if is_owner {
                    let init = match &**value {
                        Expr::Call(function, args)
                            if function == "system_box_i32_abort" && args.len() == 1 =>
                        {
                            OwnerInit::Alloc(Box::new(self.expr(&args[0])?))
                        }
                        Expr::Call(function, args)
                            if function == "system_bytes_zeroed_abort" && args.len() == 1 =>
                        {
                            OwnerInit::BytesAlloc(Box::new(self.expr(&args[0])?))
                        }
                        Expr::Move(source) => {
                            let local = self
                                .locals
                                .get_mut(source)
                                .ok_or("E-TYPED: unknown owner")?;
                            if local.ty != owner_type
                                || local.mode != ParamMode::Value
                                || !local.initialization.fully_initialized()
                                || local.loaned
                                || local.shared != 0
                            {
                                return Err("E-TYPED: invalid move".into());
                            }
                            places::take(local, &[], &self.registry.types)?;
                            OwnerInit::Move(local.id)
                        }
                        Expr::Call(name, ..) if self.functions.contains_key(name) => {
                            OwnerInit::Call(Box::new(self.expr(value)?))
                        }
                        Expr::Call(..)
                        | Expr::Var(_)
                        | Expr::If(..)
                        | Expr::Match(..)
                        | Expr::Let(..)
                        | Expr::LetMut(..) => OwnerInit::Expr(Box::new(self.expr(value)?)),
                        _ => return Err("E-TYPED-UNSUPPORTED: owner initializer".into()),
                    };
                    let id = self.fresh()?;
                    let mutable = matches!(expr, Expr::LetMut(..));
                    let previous = self.locals.insert(
                        name.clone(),
                        Local {
                            id,
                            ty: owner_type,
                            mutable,
                            initialization: Initialization::full(owner_type, &self.registry.types)?,
                            loaned: false,
                            shared: 0,
                            field_loans: Vec::new(),
                            mode: ParamMode::Value,
                        },
                    );
                    let body = self.expr(body);
                    let cleanup = self
                        .locals
                        .get(name)
                        .filter(|local| local.id == id)
                        .ok_or("E-TYPED-OWNER: lost local state")?
                        .initialization
                        .cleanup_entry(id, owner_type, &self.registry.types)?;
                    if let Some(previous) = previous {
                        self.locals.insert(name.clone(), previous);
                    } else {
                        self.locals.remove(name);
                    }
                    ExprKind::OwnerLet {
                        local: id,
                        mutable,
                        init,
                        body: Box::new(body?),
                        cleanup,
                    }
                } else {
                    let value = self.expr_child(value, NodeRole::LetValue)?;
                    let id = self.fresh()?;
                    if let Some(trace) = self.node_trace.as_deref_mut() {
                        trace.bind(crate::source_map::AstNode::Expression(expr), id);
                    }
                    let mutable = matches!(expr, Expr::LetMut(..));
                    let previous = self.locals.insert(
                        name.clone(),
                        Local {
                            id,
                            ty: value.ty,
                            mutable,
                            initialization: Initialization::full(value.ty, &self.registry.types)?,
                            loaned: false,
                            shared: 0,
                            field_loans: Vec::new(),
                            mode: ParamMode::Value,
                        },
                    );
                    let body = self.expr_child(body, NodeRole::LetBody);
                    if let Some(previous) = previous {
                        self.locals.insert(name.clone(), previous);
                    } else {
                        self.locals.remove(name);
                    }
                    ExprKind::Let {
                        local: id,
                        mutable,
                        value: Box::new(value),
                        body: Box::new(body?),
                    }
                }
            }
            Expr::Assign(name, value) => {
                let local = self
                    .locals
                    .get(name)
                    .ok_or("E-TYPED: missing assignment local")?
                    .clone();
                if !local.mutable
                    || local.ty.is_owner()
                    || !local.initialization.fully_initialized()
                    || field_loans::conflicts(&local, &[], true)
                {
                    return Err("E-TYPED: invalid assignment local".into());
                }
                ExprKind::Assign(
                    local.id,
                    Box::new(self.expr_child(value, NodeRole::AssignmentValue)?),
                )
            }
            Expr::Call(name, args) if name == "box_read" && args.len() == 1 => {
                let Expr::Var(root) = &args[0] else {
                    return Err("E-TYPED: owner read requires a root".into());
                };
                let local = self.locals.get(root).ok_or("E-TYPED: unknown owner")?;
                if local.ty != Type::OwnedI32
                    || !local.initialization.fully_initialized()
                    || local.loaned
                {
                    return Err("E-TYPED: invalid owner read".into());
                }
                ExprKind::OwnerRead(local.id)
            }
            Expr::Call(name, args) if name == "box_write" && args.len() == 2 => {
                let Expr::Inout(root) = &args[0] else {
                    return Err("E-TYPED: owner write requires inout root".into());
                };
                let local = self
                    .locals
                    .get(root)
                    .ok_or("E-TYPED: unknown owner")?
                    .clone();
                if local.ty != Type::OwnedI32
                    || !local.initialization.fully_initialized()
                    || !local.mutable
                    || local.loaned
                    || local.shared != 0
                {
                    return Err("E-TYPED: invalid owner write".into());
                }
                self.locals.get_mut(root).unwrap().loaned = true;
                let value = self.expr(&args[1]);
                self.locals.get_mut(root).unwrap().loaned = false;
                ExprKind::OwnerWrite(local.id, Box::new(value?))
            }
            Expr::Call(name, args)
                if matches!(
                    name.as_str(),
                    "owned_bytes_len" | "owned_bytes_get" | "owned_bytes_set"
                ) =>
            {
                let expected = match name.as_str() {
                    "owned_bytes_len" => 1,
                    "owned_bytes_get" => 2,
                    _ => 3,
                };
                if args.len() != expected {
                    return Err("E-TYPED-OWNER: invalid bytes operation arity".into());
                }
                let exclusive = name == "owned_bytes_set";
                let root = match (&args[0], exclusive) {
                    (Expr::Var(root), false) | (Expr::Inout(root), true) => root,
                    _ => return Err("E-TYPED-OWNER: invalid bytes operation root".into()),
                };
                let local = self
                    .locals
                    .get(root)
                    .ok_or("E-TYPED-OWNER: unknown root")?
                    .clone();
                if local.ty != Type::OwnedBytes
                    || !local.initialization.fully_initialized()
                    || local.loaned
                    || (exclusive && (!local.mutable || local.shared != 0))
                {
                    return Err("E-TYPED-OWNER: invalid bytes loan".into());
                }
                if exclusive {
                    self.locals.get_mut(root).unwrap().loaned = true;
                } else {
                    self.locals.get_mut(root).unwrap().shared += 1;
                }
                let values = args[1..]
                    .iter()
                    .map(|arg| self.expr(arg))
                    .collect::<Result<Vec<_>, _>>();
                if exclusive {
                    self.locals.get_mut(root).unwrap().loaned = false;
                } else {
                    self.locals.get_mut(root).unwrap().shared -= 1;
                }
                let mut values = values?.into_iter();
                match name.as_str() {
                    "owned_bytes_len" => ExprKind::OwnerBytesLen(local.id),
                    "owned_bytes_get" => {
                        ExprKind::OwnerBytesGet(local.id, Box::new(values.next().unwrap()))
                    }
                    _ => ExprKind::OwnerBytesSet(
                        local.id,
                        Box::new(values.next().unwrap()),
                        Box::new(values.next().unwrap()),
                    ),
                }
            }
            Expr::Call(name, args) if VectorOp::named(name).is_some() => {
                let op = VectorOp::named(name).unwrap();
                let args = args
                    .iter()
                    .enumerate()
                    .map(|(i, arg)| self.expr_child(arg, NodeRole::BuiltinOperand(i)))
                    .collect::<Result<Vec<_>, _>>()?;
                op.result(&args.iter().map(|arg| arg.ty).collect::<Vec<_>>())?;
                ExprKind::Vector(op, args)
            }
            Expr::Call(name, args) if name == "bytes_new" && args.len() == 1 => {
                let Expr::Int(n) = args[0] else {
                    return Err("E-TYPED: invalid bytes_new capacity".into());
                };
                let n = usize::try_from(n).map_err(|_| "E-TYPED: invalid bytes_new capacity")?;
                if n > 256 {
                    return Err("E-TYPED: invalid bytes_new capacity".into());
                }
                ExprKind::BytesNew(n)
            }
            Expr::Call(name, args) if name == "bytes_len" && args.len() == 1 => ExprKind::BytesLen(
                Box::new(self.expr_child(&args[0], NodeRole::BuiltinOperand(0))?),
            ),
            Expr::Call(name, args) if name == "bytes_get" && args.len() == 2 => ExprKind::BytesGet(
                Box::new(self.expr_child(&args[0], NodeRole::BuiltinOperand(0))?),
                Box::new(self.expr_child(&args[1], NodeRole::BuiltinOperand(1))?),
            ),
            Expr::Call(name, args) if name == "bytes_set" && args.len() == 3 => ExprKind::BytesSet(
                Box::new(self.expr_child(&args[0], NodeRole::BuiltinOperand(0))?),
                Box::new(self.expr_child(&args[1], NodeRole::BuiltinOperand(1))?),
                Box::new(self.expr_child(&args[2], NodeRole::BuiltinOperand(2))?),
            ),
            Expr::Call(name, args) if name == "byte_len" && args.len() == 1 => ExprKind::ByteLen(
                Box::new(self.expr_child(&args[0], NodeRole::BuiltinOperand(0))?),
            ),
            Expr::Call(name, args) if name == "byte_at" && args.len() == 2 => ExprKind::ByteAt(
                Box::new(self.expr_child(&args[0], NodeRole::BuiltinOperand(0))?),
                Box::new(self.expr_child(&args[1], NodeRole::BuiltinOperand(1))?),
            ),
            Expr::Call(name, args) => {
                let (id, lowered) = self.call_arguments(expr, name, args)?;
                ExprKind::Call(id, lowered)
            }
            Expr::Binary(op, a, b) => ExprKind::Binary(
                *op,
                Box::new(self.expr_child(a, NodeRole::BinaryLeft)?),
                Box::new(self.expr_child(b, NodeRole::BinaryRight)?),
            ),
            Expr::If(c, a, b) => {
                let condition = self.expr_child(c, NodeRole::Condition)?;
                let before = self.locals.clone();
                let yes = self.expr_child(a, NodeRole::Then)?;
                let after_yes = self.locals.clone();
                self.locals = before;
                let no = self.expr_child(b, NodeRole::Else)?;
                if !Self::same_states(&after_yes, &self.locals) {
                    return Err(
                        "E-TYPED-OWNER: move in conditional has incompatible branch ownership"
                            .into(),
                    );
                }
                self.locals = after_yes;
                ExprKind::If(Box::new(condition), Box::new(yes), Box::new(no))
            }
            Expr::While(c, b) => ExprKind::While(
                Box::new(self.expr_child(c, NodeRole::Condition)?),
                Box::new(self.expr_child(b, NodeRole::LoopBody)?),
            ),
            _ => return Err("E-TYPED-UNSUPPORTED: expression is outside primitive subset".into()),
        };
        Ok(ExprNode { ty, kind })
    }
}

/// Check the structural invariants of this provisional IR, including scope,
/// local identity, mutability, and operation types. This is not an ownership
/// or effect proof and must not be used to bypass the source checker.
pub fn verify(program: &TypedMain) -> Result<(), String> {
    if let Some(proof) = &program.logical.control {
        // This performs bounded actual-tree and frozen-witness preflight before
        // any generic recursive consumer. It proves correspondence to a frozen
        // witness; lower_main separately checked that witness against source.
        proof.verify_structure(program)?;
    } else if has_control_extension(program) {
        return Err(
            "E-TYPED-CONTROL: no-normal or discarded representation requires the canonical source proof".into(),
        );
    }
    program.native.verify()?;
    let logical_facts = program.logical.verify(program)?;
    adts::verify(&program.adts)?;
    for adt in &program.adts {
        for &ty in adt
            .arguments
            .iter()
            .chain(adt.variants.iter().flat_map(|v| &v.fields))
        {
            program.native.validate_type(ty)?;
        }
    }
    for function in &program.functions {
        let error = std::cell::RefCell::new(None);
        any_computation(&function.body, &|kind| {
            if let ExprKind::TryAlloc { kind, layout, .. } = kind {
                if let Err(e) = allocation::validate(
                    *kind,
                    layout,
                    &program.adts,
                    program.allocation_contract.as_ref(),
                ) {
                    *error.borrow_mut() = Some(e);
                }
            }
            false
        });
        if let Some(error) = error.into_inner() {
            return Err(error);
        }
    }
    let entry = program
        .functions
        .get(program.entry)
        .ok_or("E-TYPED: invalid entry")?;
    if matches!(entry.result, ResultRepr::Discarded(_)) {
        return Err("E-TYPED-CONTROL: main cannot discard its observed result".into());
    }
    if matches!(entry.result, ResultRepr::Value(ty) if ty.is_owner()) {
        return Err("E-TYPED-OWNER: main cannot return an owner".into());
    }
    if !entry.params.is_empty() {
        return Err("E-TYPED: main has parameters".into());
    }
    let has_owner = program.adts.iter().any(|adt| {
        adt.owned
            || adt.arguments.iter().any(|ty| ty.is_owner())
            || adt
                .symbolic
                .as_ref()
                .is_some_and(|logical| logical.dependencies.owner)
    }) || program.functions.iter().any(|f| {
        matches!(f.result, ResultRepr::Value(ty) if ty.is_owner())
            || f.params.iter().any(|(_, ty, _)| ty.is_owner())
            || any_computation(&f.body, &|kind| {
                matches!(
                    kind,
                    ExprKind::TryAlloc { .. }
                        | ExprKind::OwnerLet { .. }
                        | ExprKind::OwnerRead(_)
                        | ExprKind::OwnerWrite(..)
                        | ExprKind::OwnerMove(_)
                        | ExprKind::OwnerBytesLen(_)
                        | ExprKind::OwnerBytesGet(..)
                        | ExprKind::OwnerBytesSet(..)
                )
            })
    });
    if has_owner {
        if program.native.has_foreign()
            || program.adts.iter().any(|adt| {
                adt.symbolic
                    .as_ref()
                    .is_some_and(|logical| logical.dependencies.native)
                    || adt
                        .arguments
                        .iter()
                        .chain(adt.variants.iter().flat_map(|v| &v.fields))
                        .any(|ty| ty.is_raw())
            })
            || program.functions.iter().any(|f| {
                matches!(f.result, ResultRepr::Value(ty) if ty.is_raw())
                    || f.params.iter().any(|(_, ty, _)| ty.is_raw())
            })
            || program.functions.iter().any(|f| {
                any_computation(&f.body, &|kind| {
                    matches!(
                        kind,
                        ExprKind::ForeignCall(..)
                            | ExprKind::ForeignConst(_)
                            | ExprKind::ForeignConvert(..)
                            | ExprKind::NullPointer
                            | ExprKind::IsNull(_)
                    )
                })
            })
        {
            return Err(
                "E-TYPED-OWNER: owner programs cannot use foreign declarations or values".into(),
            );
        }
        verify_acyclic_calls(program)?;
    }
    let pattern_work = program
        .logical
        .control
        .as_ref()
        .map(|_| std::cell::RefCell::new(100_000usize));
    let mut ids = HashSet::new();
    for (function_index, function) in program.functions.iter().enumerate() {
        let context = VerifyContext {
            pattern_work: pattern_work.as_ref(),
            functions: &program.functions,
            adts: &program.adts,
            native: &program.native,
            logical: &logical_facts,
            function: function_index,
        };
        match &function.result {
            ResultRepr::Value(ty) => {
                adts::validate_type(*ty, &program.adts)?;
                logical_facts.in_scope(*ty, function_index)?;
                program.native.validate_type(*ty)?;
            }
            ResultRepr::NoNormal(id) => logical_facts.opaque_in_scope(id, function_index)?,
            ResultRepr::Discarded(ty) => logical_facts.logical_in_scope(ty, function_index)?,
        }
        if matches!(function.result, ResultRepr::Value(Type::Bytes(n)) if n > 256) {
            return Err("E-TYPED: invalid buffer capacity".into());
        }
        let mut scope = HashMap::new();
        for &(id, ty, mode) in &function.params {
            adts::validate_type(ty, &program.adts)?;
            logical_facts.in_scope(ty, function_index)?;
            program.native.validate_type(ty)?;
            if matches!(ty, Type::Bytes(n) if n > 256) {
                return Err("E-TYPED: invalid buffer capacity".into());
            }
            if !ids.insert(id) {
                return Err("E-TYPED: duplicate local identity".into());
            }
            scope.insert(
                id,
                Local {
                    id,
                    ty,
                    mutable: mode == ParamMode::Inout,
                    initialization: Initialization::full(ty, &program.adts)?,
                    loaned: false,
                    shared: 0,
                    field_loans: Vec::new(),
                    mode,
                },
            );
        }
        let actual_result = match &function.body {
            Computation::Discarded(value) => {
                ResultRepr::Discarded(discarded::verify(value, &mut scope, &mut ids, &context)?)
            }
            Computation::Value(value) => {
                ResultRepr::Value(verify_expr(value, &mut scope, &mut ids, &context)?)
            }
            Computation::NoNormal(value) => ResultRepr::NoNormal(results::verify_no_normal(
                value, &mut scope, &mut ids, &context,
            )?),
        };
        if actual_result != function.result {
            return Err("E-TYPED: wrong function result type".into());
        }
        let mut expected = Vec::new();
        for (id, ty, mode) in function.params.iter().rev() {
            if ty.is_owner() && *mode == ParamMode::Value {
                let local = scope.get(id).ok_or("E-TYPED-OWNER: lost parameter state")?;
                if let Some(cleanup) =
                    local
                        .initialization
                        .cleanup_entry(*id, *ty, &program.adts)?
                {
                    expected.push(cleanup);
                }
            }
        }
        if function.param_cleanup != expected {
            return Err("E-TYPED-OWNER: invalid parameter cleanup".into());
        }
    }
    Ok(())
}

struct VerifyContext<'a> {
    pattern_work: Option<&'a std::cell::RefCell<usize>>,
    logical: &'a logical::Facts,
    function: usize,
    functions: &'a [TypedFunction],
    adts: &'a [AdtType],
    native: &'a NativeManifest,
}

fn verify_call_arguments(
    id: usize,
    args: &[CallArg],
    scope: &mut HashMap<LocalId, Local>,
    ids: &mut HashSet<LocalId>,
    context: &VerifyContext<'_>,
) -> Result<ResultRepr, String> {
    let functions = context.functions;
    let adts = context.adts;
    let function = functions
        .get(id)
        .ok_or("E-TYPED: invalid function identity")?;
    if args.len() != function.params.len() {
        return Err("E-TYPED: wrong call arity".into());
    }
    let loans: Vec<_> = scope
        .iter()
        .map(|(id, local)| (*id, local.shared, local.loaned, local.field_loans.clone()))
        .collect();
    for (arg, (_, ty, mode)) in args.iter().zip(&function.params) {
        match (arg, ty, mode) {
            (CallArg::Move(id), ty, ParamMode::Value) if ty.is_owner() => {
                let owner = scope
                    .get_mut(id)
                    .ok_or("E-TYPED-OWNER: unknown move source")?;
                if owner.ty != *ty
                    || owner.mode != ParamMode::Value
                    || !owner.initialization.fully_initialized()
                    || owner.loaned
                    || owner.shared != 0
                {
                    return Err("E-TYPED-OWNER: invalid move argument".into());
                }
                places::take(owner, &[], adts)?;
            }
            (CallArg::ReadOwner(id), ty, ParamMode::Read) if ty.is_owner() => {
                let owner = scope
                    .get_mut(id)
                    .ok_or("E-TYPED-OWNER: unknown read root")?;
                if owner.ty != *ty || !owner.initialization.fully_initialized() || owner.loaned {
                    return Err("E-TYPED-OWNER: invalid shared loan".into());
                }
                owner.shared = owner
                    .shared
                    .checked_add(1)
                    .ok_or("E-TYPED-OWNER: too many shared loans")?;
            }
            (CallArg::InoutOwner(id), ty, ParamMode::Inout) if ty.is_owner() => {
                let owner = scope
                    .get_mut(id)
                    .ok_or("E-TYPED-OWNER: unknown inout root")?;
                if owner.ty != *ty
                    || !owner.initialization.fully_initialized()
                    || !owner.mutable
                    || owner.loaned
                    || owner.shared != 0
                {
                    return Err("E-TYPED-OWNER: invalid exclusive loan".into());
                }
                owner.loaned = true;
            }
            (CallArg::ReadCopy(id), ty, ParamMode::Read) if !ty.is_owner() => {
                copy_loans::verify_root(*id, *ty, ParamMode::Read, scope)?;
            }
            (CallArg::InoutCopy(id), ty, ParamMode::Inout) if !ty.is_owner() => {
                copy_loans::verify_root(*id, *ty, ParamMode::Inout, scope)?;
            }
            (CallArg::ReadField(place), ty, ParamMode::Read) if !ty.is_owner() => {
                field_loans::verify_field(place, *ty, ParamMode::Read, scope, adts)?;
            }
            (CallArg::InoutField(place), ty, ParamMode::Inout) if !ty.is_owner() => {
                field_loans::verify_field(place, *ty, ParamMode::Inout, scope, adts)?;
            }
            (CallArg::Value(value), ty, ParamMode::Value)
            | (CallArg::ReadTemporary(value), ty, ParamMode::Read)
                if !ty.is_owner() =>
            {
                if verify_expr(value, scope, ids, context)? != *ty {
                    return Err("E-TYPED: wrong call argument type".into());
                }
            }
            _ => return Err("E-TYPED-OWNER: invalid call argument mode".into()),
        }
    }
    for (id, shared, loaned, fields) in loans {
        let local = scope.get_mut(&id).ok_or("E-TYPED: lost call scope")?;
        local.shared = shared;
        local.loaned = loaned;
        local.field_loans = fields;
    }
    Ok(function.result.clone())
}

fn verify_expr(
    expr: &ExprNode,
    scope: &mut HashMap<LocalId, Local>,
    ids: &mut HashSet<LocalId>,
    context: &VerifyContext<'_>,
) -> Result<Type, String> {
    let adts = context.adts;
    if expr.ty.is_owner()
        && !matches!(
            expr.kind,
            ExprKind::TryAlloc { .. }
                | ExprKind::OwnerMove(_)
                | ExprKind::Call(..)
                | ExprKind::Constructor { .. }
                | ExprKind::Match { .. }
                | ExprKind::CopyMatch { .. }
                | ExprKind::If(..)
                | ExprKind::OwnerLet { .. }
                | ExprKind::Let { .. }
        )
    {
        return Err("E-TYPED-OWNER: unsupported owner result expression".into());
    }
    adts::validate_type(expr.ty, adts)?;
    context.logical.in_scope(expr.ty, context.function)?;
    context.native.validate_type(expr.ty)?;
    use BinaryOp::*;
    if matches!(expr.ty, Type::Bytes(n) if n > 256) {
        return Err("E-TYPED: invalid buffer capacity".into());
    }
    let ty = match &expr.kind {
        ExprKind::InterruptedCall(call) => {
            results::verify_interrupted(call, scope, ids, context)?;
            expr.ty
        }
        ExprKind::DiscardLet { value, body, .. } => {
            discarded::verify(value, scope, ids, context)?;
            verify_expr(body, scope, ids, context)?
        }
        ExprKind::NoStorageLet { value, body, .. } => {
            results::verify_no_normal(value, scope, ids, context)?;
            verify_expr(body, scope, ids, context)?
        }
        ExprKind::ForeignCall(id, args) => foreign::verify_call(*id, args, scope, ids, context)?,
        ExprKind::ForeignConst(id) => foreign::verify_constant(*id, context.native)?,
        ExprKind::ForeignConvert(conversion, input) => {
            if verify_expr(input, scope, ids, context)? != Type::Int {
                return Err("E-TYPED-FFI: conversion input must be i32".into());
            }
            conversion.result()
        }
        ExprKind::NullPointer => {
            if !matches!(expr.ty, Type::RawPointer(_)) {
                return Err("E-TYPED-FFI: null must have a concrete pointer type".into());
            }
            expr.ty
        }
        ExprKind::IsNull(input) => {
            if !matches!(
                verify_expr(input, scope, ids, context)?,
                Type::RawPointer(_)
            ) {
                return Err("E-TYPED-FFI: null test needs a pointer".into());
            }
            Type::Bool
        }
        ExprKind::Vector(op, args) => {
            let types = args
                .iter()
                .map(|arg| verify_expr(arg, scope, ids, context))
                .collect::<Result<Vec<_>, _>>()?;
            op.result(&types)?
        }
        ExprKind::TryAlloc { input, layout, .. } => {
            if verify_expr(input, scope, ids, context)? != Type::Int {
                return Err("E-TYPED-ALLOC: input must be i32".into());
            }
            Type::Adt {
                id: layout.result_adt,
                owned: true,
            }
        }
        ExprKind::CopyMatch { value, arms } => {
            copy_match::verify_match(value, arms, scope, ids, context)?
        }
        ExprKind::Constructor {
            adt,
            variant,
            fields,
        } => adt_verify::constructor(*adt, *variant, fields, scope, ids, context)?,
        ExprKind::Match { value, arms } => {
            adt_verify::match_value(value, arms, scope, ids, context)?
        }
        ExprKind::Int(_) => Type::Int,
        ExprKind::Str(_) => Type::StaticStr,
        ExprKind::Bool(_) => Type::Bool,
        ExprKind::Unit => Type::Unit,
        ExprKind::Record { adt, fields } => {
            records::verify_constructor(*adt, fields, scope, ids, context)?
        }
        ExprKind::FieldRead(place) => records::verify_place(place, scope, adts, false)?,
        ExprKind::FieldAssign(place, value) => {
            let expected = records::verify_place(place, scope, adts, true)?;
            if verify_expr(value, scope, ids, context)? != expected {
                return Err("E-TYPED-RECORD: incorrect assigned field type".into());
            }
            Type::Unit
        }
        ExprKind::Read(id) => {
            let local = scope.get(id).ok_or("E-TYPED: read outside local scope")?;
            if local.ty.is_owner()
                || !local.initialization.fully_initialized()
                || field_loans::conflicts(local, &[], false)
            {
                return Err("E-TYPED-OWNER: invalid ordinary read".into());
            }
            local.ty
        }
        ExprKind::OwnerMove(id) => {
            let source = scope
                .get_mut(id)
                .ok_or("E-TYPED-OWNER: unknown return move source")?;
            if !source.ty.is_owner()
                || source.mode != ParamMode::Value
                || !source.initialization.fully_initialized()
                || source.loaned
                || source.shared != 0
            {
                return Err("E-TYPED-OWNER: invalid return move".into());
            }
            places::take(source, &[], adts)?;
            source.ty
        }
        ExprKind::OwnerRead(id) => {
            let local = scope.get(id).ok_or("E-TYPED-OWNER: missing owner")?;
            if local.ty != Type::OwnedI32
                || !local.initialization.fully_initialized()
                || local.loaned
            {
                return Err("E-TYPED-OWNER: invalid owner read".into());
            }
            Type::Int
        }
        ExprKind::OwnerWrite(id, value) => {
            let local = scope.get(id).ok_or("E-TYPED-OWNER: missing owner")?.clone();
            if local.ty != Type::OwnedI32
                || !local.initialization.fully_initialized()
                || !local.mutable
                || local.loaned
                || local.shared != 0
            {
                return Err("E-TYPED-OWNER: invalid owner write".into());
            }
            scope.get_mut(id).unwrap().loaned = true;
            let value_type = verify_expr(value, scope, ids, context);
            scope.get_mut(id).unwrap().loaned = false;
            if value_type? != Type::Int {
                return Err("E-TYPED-OWNER: invalid written value".into());
            }
            Type::Unit
        }
        ExprKind::OwnerBytesLen(id)
        | ExprKind::OwnerBytesGet(id, _)
        | ExprKind::OwnerBytesSet(id, _, _) => {
            let local = scope
                .get(id)
                .ok_or("E-TYPED-OWNER: missing bytes owner")?
                .clone();
            let exclusive = matches!(expr.kind, ExprKind::OwnerBytesSet(..));
            if local.ty != Type::OwnedBytes
                || !local.initialization.fully_initialized()
                || local.loaned
                || (exclusive && (!local.mutable || local.shared != 0))
            {
                return Err("E-TYPED-OWNER: invalid bytes loan".into());
            }
            if exclusive {
                scope.get_mut(id).unwrap().loaned = true;
            } else {
                let owner = scope.get_mut(id).unwrap();
                owner.shared = owner
                    .shared
                    .checked_add(1)
                    .ok_or("E-TYPED-OWNER: too many loans")?;
            }
            let check = match &expr.kind {
                ExprKind::OwnerBytesLen(_) => Ok(()),
                ExprKind::OwnerBytesGet(_, index) => {
                    if verify_expr(index, scope, ids, context)? == Type::Int {
                        Ok(())
                    } else {
                        Err("E-TYPED-OWNER: invalid bytes index")
                    }
                }
                ExprKind::OwnerBytesSet(_, index, value) => {
                    if verify_expr(index, scope, ids, context)? == Type::Int
                        && verify_expr(value, scope, ids, context)? == Type::Int
                    {
                        Ok(())
                    } else {
                        Err("E-TYPED-OWNER: invalid bytes operands")
                    }
                }
                _ => unreachable!(),
            };
            if exclusive {
                scope.get_mut(id).unwrap().loaned = false;
            } else {
                scope.get_mut(id).unwrap().shared -= 1;
            }
            check?;
            if exclusive {
                Type::Unit
            } else {
                Type::Int
            }
        }
        ExprKind::OwnerLet {
            local,
            mutable,
            init,
            body,
            cleanup,
        } => {
            let owner_ty = match init {
                OwnerInit::Expr(value) => {
                    let ty = verify_expr(value, scope, ids, context)?;
                    if !ty.is_owner() {
                        return Err("E-TYPED-OWNER: expected affine initializer".into());
                    }
                    ty
                }
                OwnerInit::Call(value) => {
                    if !matches!(value.kind, ExprKind::Call(..)) {
                        return Err("E-TYPED-OWNER: initializer must be a direct call".into());
                    }
                    let ty = verify_expr(value, scope, ids, context)?;
                    if !ty.is_owner() {
                        return Err("E-TYPED-OWNER: call must return an owner".into());
                    }
                    ty
                }
                OwnerInit::Alloc(value) | OwnerInit::BytesAlloc(value) => {
                    if verify_expr(value, scope, ids, context)? != Type::Int {
                        return Err("E-TYPED-OWNER: allocation requires i32".into());
                    }
                    if matches!(init, OwnerInit::Alloc(_)) {
                        Type::OwnedI32
                    } else {
                        Type::OwnedBytes
                    }
                }
                OwnerInit::Move(source) => {
                    let source = scope
                        .get_mut(source)
                        .ok_or("E-TYPED-OWNER: unknown move source")?;
                    if !source.ty.is_owner()
                        || source.mode != ParamMode::Value
                        || !source.initialization.fully_initialized()
                        || source.loaned
                        || source.shared != 0
                    {
                        return Err("E-TYPED-OWNER: invalid move".into());
                    }
                    places::take(source, &[], adts)?;
                    source.ty
                }
            };
            if !ids.insert(*local) {
                return Err("E-TYPED: duplicate local identity".into());
            }
            scope.insert(
                *local,
                Local {
                    id: *local,
                    ty: owner_ty,
                    mutable: *mutable,
                    initialization: Initialization::full(owner_ty, adts)?,
                    loaned: false,
                    shared: 0,
                    field_loans: Vec::new(),
                    mode: ParamMode::Value,
                },
            );
            let result = verify_expr(body, scope, ids, context)?;
            let expected = scope
                .get(local)
                .ok_or("E-TYPED-OWNER: missing owner")?
                .initialization
                .cleanup_entry(*local, owner_ty, adts)?;
            if *cleanup != expected {
                return Err("E-TYPED-OWNER: invalid cleanup".into());
            }
            scope.remove(local);
            result
        }
        ExprKind::Call(id, args) => {
            verify_call_arguments(*id, args, scope, ids, context)?.value()?
        }
        ExprKind::BytesNew(n) if *n <= 256 => Type::Bytes(*n),
        ExprKind::BytesNew(_) => return Err("E-TYPED: invalid buffer capacity".into()),
        ExprKind::BytesLen(value) => {
            if !matches!(verify_expr(value, scope, ids, context)?, Type::Bytes(_)) {
                return Err("E-TYPED: invalid bytes_len operand".into());
            }
            Type::Int
        }
        ExprKind::BytesGet(value, index) => {
            if !matches!(verify_expr(value, scope, ids, context)?, Type::Bytes(_))
                || verify_expr(index, scope, ids, context)? != Type::Int
            {
                return Err("E-TYPED: invalid bytes_get operands".into());
            }
            Type::Int
        }
        ExprKind::BytesSet(value, index, byte) => {
            let ty = verify_expr(value, scope, ids, context)?;
            if !matches!(ty, Type::Bytes(_))
                || verify_expr(index, scope, ids, context)? != Type::Int
                || verify_expr(byte, scope, ids, context)? != Type::Int
            {
                return Err("E-TYPED: invalid bytes_set operands".into());
            }
            ty
        }
        ExprKind::ByteLen(value) => {
            if verify_expr(value, scope, ids, context)? != Type::StaticStr {
                return Err("E-TYPED: invalid byte_len operand".into());
            }
            Type::Int
        }
        ExprKind::ByteAt(value, index) => {
            if verify_expr(value, scope, ids, context)? != Type::StaticStr
                || verify_expr(index, scope, ids, context)? != Type::Int
            {
                return Err("E-TYPED: invalid byte_at operands".into());
            }
            Type::Int
        }
        ExprKind::Let {
            local,
            mutable,
            value,
            body,
        } => {
            let value_ty = verify_expr(value, scope, ids, context)?;
            if value_ty.is_owner() {
                return Err("E-TYPED-OWNER: owner needs explicit init".into());
            }
            if !ids.insert(*local) {
                return Err("E-TYPED: duplicate local identity".into());
            }
            scope.insert(
                *local,
                Local {
                    id: *local,
                    ty: value_ty,
                    mutable: *mutable,
                    initialization: Initialization::full(value_ty, adts)?,
                    loaned: false,
                    shared: 0,
                    field_loans: Vec::new(),
                    mode: ParamMode::Value,
                },
            );
            let result = verify_expr(body, scope, ids, context);
            scope.remove(local);
            result?
        }
        ExprKind::Assign(id, value) => {
            let local = scope
                .get(id)
                .ok_or("E-TYPED: assignment outside local scope")?
                .clone();
            if !local.mutable
                || local.ty.is_owner()
                || !local.initialization.fully_initialized()
                || field_loans::conflicts(&local, &[], true)
                || verify_expr(value, scope, ids, context)? != local.ty
            {
                return Err("E-TYPED: invalid assignment".into());
            }
            Type::Unit
        }
        ExprKind::Binary(op, a, b) => {
            let left = verify_expr(a, scope, ids, context)?;
            let snapshot = scope.clone();
            let right = verify_expr(b, scope, ids, context)?;
            if matches!(op, And | Or) && !same_owner_states(&snapshot, scope) {
                return Err("E-TYPED-OWNER: move in conditional expression".into());
            }
            match op {
                Add | Sub | Mul | Div | Rem if left == Type::Int && right == Type::Int => Type::Int,
                Lt | Le | Gt | Ge if left == Type::Int && right == Type::Int => Type::Bool,
                And | Or if left == Type::Bool && right == Type::Bool => Type::Bool,
                Eq | Ne
                    if left == right
                        && (matches!(left, Type::Int | Type::Bool | Type::Unit)
                            || left.is_raw()) =>
                {
                    Type::Bool
                }
                _ => return Err("E-TYPED: invalid binary operand types".into()),
            }
        }
        ExprKind::If(c, a, b) => {
            if verify_expr(c, scope, ids, context)? != Type::Bool {
                return Err("E-TYPED: invalid condition".into());
            }
            let mut yes_scope = scope.clone();
            let a = verify_expr(a, &mut yes_scope, ids, context)?;
            let mut no_scope = scope.clone();
            if verify_expr(b, &mut no_scope, ids, context)? != a {
                return Err("E-TYPED: inconsistent branches".into());
            }
            if !same_owner_states(&yes_scope, &no_scope) {
                return Err(
                    "E-TYPED-OWNER: move in conditional has incompatible branch ownership".into(),
                );
            }
            *scope = yes_scope;
            a
        }
        ExprKind::While(c, b) => {
            let before = scope.clone();
            if verify_expr(c, scope, ids, context)? != Type::Bool {
                return Err("E-TYPED: invalid loop condition".into());
            }
            if !same_owner_states(&before, scope) {
                return Err("E-TYPED-OWNER: move in loop".into());
            }
            if verify_expr(b, scope, ids, context)? != Type::Unit {
                return Err("E-TYPED: invalid loop body".into());
            }
            if !same_owner_states(&before, scope) {
                return Err("E-TYPED-OWNER: move in loop".into());
            }
            Type::Unit
        }
    };
    if expr.ty != ty {
        return Err("E-TYPED: incorrect expression type".into());
    }
    Ok(ty)
}

fn has_control_extension(program: &TypedMain) -> bool {
    control::has_extension(program)
}

fn any_computation(computation: &Computation, predicate: &dyn Fn(&ExprKind) -> bool) -> bool {
    match computation {
        Computation::Value(value) => any_expr(value, predicate),
        Computation::Discarded(value) => {
            value.any_value_root(&mut |value| any_expr(value, predicate))
        }
        Computation::NoNormal(value) => {
            value.any_value_root(&mut |value| any_expr(value, predicate))
        }
    }
}

fn any_expr(expr: &ExprNode, predicate: &dyn Fn(&ExprKind) -> bool) -> bool {
    if predicate(&expr.kind) {
        return true;
    }
    match &expr.kind {
        ExprKind::InterruptedCall(call) => call.any_value_root(&mut |value| any_expr(value, predicate)),
        ExprKind::DiscardLet { value, body, .. } => {
            value.any_value_root(&mut |value| any_expr(value, predicate)) || any_expr(body, predicate)
        }
        ExprKind::NoStorageLet { value, body, .. } => {
            value.any_value_root(&mut |value| any_expr(value, predicate)) || any_expr(body, predicate)
        }

        ExprKind::ForeignCall(_, args) => args.iter().any(|arg| matches!(arg, ForeignArg::Value(value) if any_expr(value, predicate))),
        ExprKind::Vector(_,args)=>args.iter().any(|arg|any_expr(arg,predicate)),
        ExprKind::CopyMatch { value, arms } => {
            any_expr(value, predicate) || arms.iter().any(|a| any_expr(&a.body, predicate))
        }
        ExprKind::Record{fields,..}=>fields.iter().any(|(_,value)|any_expr(value,predicate)),
        ExprKind::FieldAssign(_,value)=>any_expr(value,predicate),
        ExprKind::Constructor { fields, .. } => fields.iter().any(|e| any_expr(e, predicate)),
        ExprKind::Match { value, arms } => {
            any_expr(value, predicate) || arms.iter().any(|a| any_expr(&a.body, predicate))
        }
        ExprKind::OwnerLet { init, body, .. } => {
            matches!(init, OwnerInit::Alloc(value) | OwnerInit::BytesAlloc(value) | OwnerInit::Call(value) | OwnerInit::Expr(value) if any_expr(value, predicate))
                || any_expr(body, predicate)
        }
        ExprKind::OwnerBytesSet(_, a, b) => any_expr(a, predicate) || any_expr(b, predicate),
        ExprKind::ForeignConvert(_, value)
        | ExprKind::IsNull(value)
        | ExprKind::TryAlloc { input: value, .. }
        | ExprKind::OwnerBytesGet(_, value)
        | ExprKind::OwnerWrite(_, value)
        | ExprKind::Assign(_, value)
        | ExprKind::ByteLen(value)
        | ExprKind::BytesLen(value) => any_expr(value, predicate),
        ExprKind::Call(_, args) => args
            .iter()
            .any(|arg| matches!(arg, CallArg::Value(value) | CallArg::ReadTemporary(value) if any_expr(value, predicate))),
        ExprKind::Let { value, body, .. }
        | ExprKind::Binary(_, value, body)
        | ExprKind::While(value, body)
        | ExprKind::ByteAt(value, body)
        | ExprKind::BytesGet(value, body) => {
            any_expr(value, predicate) || any_expr(body, predicate)
        }
        ExprKind::If(a, b, c) | ExprKind::BytesSet(a, b, c) => {
            any_expr(a, predicate) || any_expr(b, predicate) || any_expr(c, predicate)
        }
        _ => false,
    }
}

fn collect_calls(expr: &ExprNode, out: &mut Vec<usize>) {
    match &expr.kind {
        ExprKind::InterruptedCall(call) => call.visit_calls(out),
        ExprKind::DiscardLet { value, body, .. } => {
            value.visit_calls(out);
            collect_calls(body, out);
        }
        ExprKind::NoStorageLet { value, body, .. } => {
            value.visit_calls(out);
            collect_calls(body, out);
        }

        ExprKind::ForeignCall(_, args) => {
            for arg in args {
                if let ForeignArg::Value(value) = arg {
                    collect_calls(value, out);
                }
            }
        }
        ExprKind::Vector(_, args) => {
            for arg in args {
                collect_calls(arg, out);
            }
        }
        ExprKind::CopyMatch { value, arms } => {
            collect_calls(value, out);
            for arm in arms {
                collect_calls(&arm.body, out);
            }
        }
        ExprKind::Record { fields, .. } => {
            for (_, field) in fields {
                collect_calls(field, out);
            }
        }
        ExprKind::FieldAssign(_, value) => collect_calls(value, out),
        ExprKind::Constructor { fields, .. } => {
            for field in fields {
                collect_calls(field, out);
            }
        }
        ExprKind::Match { value, arms } => {
            collect_calls(value, out);
            for arm in arms {
                collect_calls(&arm.body, out);
            }
        }
        ExprKind::Call(id, args) => {
            out.push(*id);
            for arg in args {
                if let CallArg::Value(value) | CallArg::ReadTemporary(value) = arg {
                    collect_calls(value, out);
                }
            }
        }
        ExprKind::OwnerLet { init, body, .. } => {
            if let OwnerInit::Alloc(value)
            | OwnerInit::BytesAlloc(value)
            | OwnerInit::Call(value)
            | OwnerInit::Expr(value) = init
            {
                collect_calls(value, out);
            }
            collect_calls(body, out);
        }
        ExprKind::OwnerBytesSet(_, a, b) => {
            collect_calls(a, out);
            collect_calls(b, out);
        }
        ExprKind::ForeignConvert(_, value)
        | ExprKind::IsNull(value)
        | ExprKind::TryAlloc { input: value, .. }
        | ExprKind::OwnerBytesGet(_, value)
        | ExprKind::OwnerWrite(_, value)
        | ExprKind::Assign(_, value)
        | ExprKind::ByteLen(value)
        | ExprKind::BytesLen(value) => collect_calls(value, out),
        ExprKind::Let { value, body, .. }
        | ExprKind::Binary(_, value, body)
        | ExprKind::While(value, body)
        | ExprKind::ByteAt(value, body)
        | ExprKind::BytesGet(value, body) => {
            collect_calls(value, out);
            collect_calls(body, out);
        }
        ExprKind::If(a, b, c) | ExprKind::BytesSet(a, b, c) => {
            collect_calls(a, out);
            collect_calls(b, out);
            collect_calls(c, out);
        }
        _ => {}
    }
}
fn verify_acyclic_calls(program: &TypedMain) -> Result<(), String> {
    let mut edges = Vec::new();
    let mut indegree = vec![0usize; program.functions.len()];
    for function in &program.functions {
        let mut targets = Vec::new();
        collect_calls(function.body.value_expr()?, &mut targets);
        for &target in &targets {
            let degree = indegree
                .get_mut(target)
                .ok_or("E-TYPED: invalid function identity")?;
            *degree = degree.checked_add(1).ok_or("E-TYPED: too many calls")?;
        }
        edges.push(targets);
    }
    let mut queue: std::collections::VecDeque<_> = indegree
        .iter()
        .enumerate()
        .filter_map(|(i, n)| (*n == 0).then_some(i))
        .collect();
    let mut visited = 0;
    while let Some(source) = queue.pop_front() {
        visited += 1;
        for &target in &edges[source] {
            indegree[target] -= 1;
            if indegree[target] == 0 {
                queue.push_back(target);
            }
        }
    }
    if visited != program.functions.len() {
        return Err("E-TYPED-OWNER: recursive owner programs are unsupported".into());
    }
    Ok(())
}

fn same_owner_states(a: &HashMap<LocalId, Local>, b: &HashMap<LocalId, Local>) -> bool {
    a.iter()
        .filter(|(_, local)| local.ty.is_owner())
        .all(|(id, local)| {
            b.get(id).is_some_and(|other| {
                other.ty == local.ty
                    && other.initialization == local.initialization
                    && other.loaned == local.loaned
                    && other.shared == local.shared
                    && other.field_loans == local.field_loans
            })
        })
}

/// Evaluate the provisional IR used by both supported execution backends. Verification is
/// repeated because callers can construct or modify IR nodes after lowering.
pub fn evaluate(program: &TypedMain) -> Result<crate::interpreter::Value, String> {
    verify(program)?;
    let mut state = EvalState::default();
    match evaluate_function(program, program.entry, Vec::new(), 1, &mut state)? {
        EvalArg::Value(value) => Ok(value),
        EvalArg::Owner(_) | EvalArg::CopyLoan(_) => {
            Err("E-TYPED-OWNER: main returned a non-value".into())
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct OwnerHandle {
    id: usize,
    mode: ParamMode,
}

#[derive(Debug)]
enum EvalArg {
    Value(crate::interpreter::Value),
    CopyLoan(CopySlot),
    Owner(OwnerHandle),
}

#[derive(Debug)]
enum FunctionReturn {
    Value(EvalArg),
    Discarded,
}
impl FunctionReturn {
    fn value(self) -> Result<EvalArg, String> {
        match self {
            Self::Value(value) => Ok(value),
            Self::Discarded => Err("E-TYPED-CONTROL: discarded result used as value".into()),
        }
    }
}
fn evaluate_function(
    program: &TypedMain,
    id: usize,
    args: Vec<EvalArg>,
    depth: usize,
    state: &mut EvalState,
) -> Result<EvalArg, String> {
    evaluate_function_outcome(program, id, args, depth, state)?.value()
}
fn evaluate_function_outcome(
    program: &TypedMain,
    id: usize,
    args: Vec<EvalArg>,
    depth: usize,
    state: &mut EvalState,
) -> Result<FunctionReturn, String> {
    let function = &program.functions[id];
    let mut locals = CopyLocals::default();
    let mut owners = HashMap::new();
    for ((id, ty, mode), value) in function.params.iter().zip(args) {
        match (ty, mode, value) {
            (ty, mode, EvalArg::Owner(handle)) if ty.is_owner() && *mode == handle.mode => {
                owners.insert(*id, handle);
            }
            (ty, ParamMode::Value, EvalArg::Value(value)) if !ty.is_owner() => {
                locals.insert(*id, value);
            }
            (ty, ParamMode::Read | ParamMode::Inout, EvalArg::CopyLoan(slot)) if !ty.is_owner() => {
                locals.bind_loan(*id, slot);
            }
            _ => return Err("E-TYPED-OWNER: invalid runtime argument".into()),
        }
    }
    let result = if let Computation::Discarded(body) = &function.body {
        discarded::evaluate(body, &mut locals, program, depth, state, &mut owners)
            .map(|()| FunctionReturn::Discarded)
    } else if let Computation::NoNormal(body) = &function.body {
        match results::evaluate_no_normal(body, &mut locals, program, depth, state, &mut owners) {
            Ok(never) => match never {},
            Err(error) => Err(error),
        }
    } else if function.result.value()?.is_owner() {
        evaluate_owner_expr(
            function.body.value_expr()?,
            &mut locals,
            program,
            depth,
            state,
            &mut owners,
        )
        .map(|value| FunctionReturn::Value(EvalArg::Owner(value)))
    } else {
        evaluate_expr(
            function.body.value_expr()?,
            &mut locals,
            program,
            depth,
            state,
            &mut owners,
        )
        .map(|value| FunctionReturn::Value(EvalArg::Value(value)))
    };
    if result.is_ok() {
        for cleanup in &function.param_cleanup {
            let handle = owners
                .remove(&cleanup.local)
                .ok_or("E-TYPED-OWNER: missing parameter cleanup")?;
            if handle.mode != ParamMode::Value {
                return Err("E-TYPED-OWNER: attempted borrowed cleanup".into());
            }
            state.release_cleanup(handle, &cleanup.tree)?;
        }
        if owners
            .values()
            .any(|handle| handle.mode == ParamMode::Value)
        {
            return Err("E-TYPED-OWNER: live owner escaped frame cleanup".into());
        }
    }
    result
}

#[inline(never)]
fn evaluate_expr(
    expr: &ExprNode,
    locals: &mut CopyLocals,
    program: &TypedMain,
    depth: usize,
    state: &mut EvalState,
    owners: &mut HashMap<LocalId, OwnerHandle>,
) -> Result<crate::interpreter::Value, String> {
    use crate::interpreter::Value;
    match &expr.kind {
        ExprKind::InterruptedCall(call) => {
            match results::evaluate_interrupted(call, locals, program, depth, state, owners) {
                Ok(never) => match never {},
                Err(error) => Err(error),
            }
        }
        ExprKind::DiscardLet { value, body, .. } => {
            discarded::evaluate(value, locals, program, depth, state, owners)?;
            evaluate_expr(body, locals, program, depth, state, owners)
        }
        ExprKind::NoStorageLet { value, .. } => {
            match results::evaluate_no_normal(value, locals, program, depth, state, owners) {
                Ok(never) => match never {},
                Err(error) => Err(error),
            }
        }

        ExprKind::ForeignCall(id, args) => {
            for arg in args {
                if let ForeignArg::Value(value) = arg {
                    evaluate_expr(value, locals, program, depth, state, owners)?;
                }
            }
            Err(format!(
                "E-INTERP-FFI: cannot call C function {} in the interpreter",
                program.native.functions[id.0 as usize].name
            ))
        }
        ExprKind::ForeignConst(_) => {
            Err("E-FFI: imported C constants require native C emission".into())
        }
        ExprKind::ForeignConvert(..) | ExprKind::NullPointer | ExprKind::IsNull(_) => {
            // The AST evaluator rejects raw operations before reaching operands.
            Err("E-FFI: raw C values require native C emission".into())
        }
        ExprKind::Vector(op, args) => {
            vectors::evaluate(*op, args, locals, program, depth, state, owners)
        }
        ExprKind::TryAlloc { .. } => Err("E-TYPED-ALLOC: affine allocation used as Copy".into()),
        ExprKind::CopyMatch { value, arms } => copy_match::evaluate_match(
            value,
            arms,
            locals,
            program,
            depth,
            state,
            owners,
            evaluate_expr,
        ),
        ExprKind::Constructor {
            adt,
            variant,
            fields,
        } => match adt_eval::construct(
            *adt, *variant, fields, locals, program, depth, state, owners,
        )? {
            EvalArg::Value(value) => Ok(value),
            _ => Err("E-TYPED-ADT: affine constructor used as Copy".into()),
        },
        ExprKind::Match { value, arms } => adt_eval::match_value(
            value,
            arms,
            locals,
            program,
            depth,
            state,
            owners,
            evaluate_expr,
        ),
        ExprKind::OwnerLet { .. } => {
            evaluate_owner_let(expr, locals, program, depth, state, owners, evaluate_expr)
        }
        ExprKind::OwnerMove(_) => Err("E-TYPED-OWNER: move cannot produce a scalar".into()),
        ExprKind::Call(id, args) => {
            match evaluate_call(*id, args, locals, program, depth, state, owners)? {
                EvalArg::Value(value) => Ok(value),
                EvalArg::Owner(_) | EvalArg::CopyLoan(_) => {
                    Err("E-TYPED-OWNER: non-value used as a scalar".into())
                }
            }
        }
        ExprKind::Let { .. } => {
            evaluate_lets(expr, locals, program, depth, state, owners, evaluate_expr)
        }
        ExprKind::Assign(id, value) => {
            let value = evaluate_expr(value, locals, program, depth, state, owners)?;
            locals.write(id, value)?;
            Ok(Value::Unit)
        }
        ExprKind::Binary(op, a, b) => {
            let a = evaluate_expr(a, locals, program, depth, state, owners)?;
            if *op == BinaryOp::And && a == Value::Bool(false) {
                return Ok(a);
            }
            if *op == BinaryOp::Or && a == Value::Bool(true) {
                return Ok(a);
            }
            let b = evaluate_expr(b, locals, program, depth, state, owners)?;
            crate::interpreter::binary(op, a, b)
        }
        ExprKind::If(c, a, b) => match evaluate_expr(c, locals, program, depth, state, owners)? {
            Value::Bool(true) => evaluate_expr(a, locals, program, depth, state, owners),
            Value::Bool(false) => evaluate_expr(b, locals, program, depth, state, owners),
            _ => Err("E-TYPED: invalid condition".into()),
        },
        ExprKind::While(c, body) => {
            loop {
                match evaluate_expr(c, locals, program, depth, state, owners)? {
                    Value::Bool(false) => break,
                    Value::Bool(true) => {
                        evaluate_expr(body, locals, program, depth, state, owners)?;
                    }
                    _ => return Err("E-TYPED: invalid condition".into()),
                }
            }
            Ok(Value::Unit)
        }
        _ => evaluate_leaf(expr, locals, program, depth, state, owners),
    }
}

// A source block becomes a chain of Copy lets. Walking its continuation does
// not consume source call depth, and must not retain one host frame per binding.
// OwnerLet remains a distinct operation with its own cleanup semantics.
#[inline(never)]
fn evaluate_lets<T>(
    mut expr: &ExprNode,
    locals: &mut CopyLocals,
    program: &TypedMain,
    depth: usize,
    state: &mut EvalState,
    owners: &mut HashMap<LocalId, OwnerHandle>,
    evaluate_body: EvaluateBody<T>,
) -> Result<T, String> {
    let mut introduced = Vec::new();
    let result = loop {
        let ExprKind::Let {
            local, value, body, ..
        } = &expr.kind
        else {
            break evaluate_body(expr, locals, program, depth, state, owners);
        };
        match evaluate_expr(value, locals, program, depth, state, owners) {
            Ok(value) => {
                locals.insert(*local, value);
                introduced.push(*local);
                expr = body;
            }
            Err(error) => break Err(error),
        }
    };
    for local in introduced.into_iter().rev() {
        locals.remove(&local);
    }
    result
}

// Keep builtin temporaries out of recursive control-flow frames. In particular,
// the JSON recursion regression must reach the unchanged call-depth trap on the
// default test-thread stack; a larger host stack is not its language semantics.
#[inline(never)]
fn evaluate_leaf(
    expr: &ExprNode,
    locals: &mut CopyLocals,
    program: &TypedMain,
    depth: usize,
    state: &mut EvalState,
    owners: &mut HashMap<LocalId, OwnerHandle>,
) -> Result<crate::interpreter::Value, String> {
    use crate::interpreter::Value;
    match &expr.kind {
        ExprKind::OwnerRead(id) => {
            let handle = owners.get(id).ok_or("E-TYPED-OWNER: missing live owner")?;
            state
                .storage
                .get(&handle.id)
                .and_then(|value| match value {
                    OwnerStorage::BoxI32(value) => value.first(),
                    _ => None,
                })
                .copied()
                .map(Value::Int)
                .ok_or_else(|| "E-TYPED-OWNER: missing allocation".into())
        }
        ExprKind::OwnerWrite(id, value) => {
            let Value::Int(value) = evaluate_expr(value, locals, program, depth, state, owners)?
            else {
                return Err("E-TYPED-OWNER: invalid value".into());
            };
            let handle = owners.get(id).ok_or("E-TYPED-OWNER: missing live owner")?;
            *state
                .storage
                .get_mut(&handle.id)
                .and_then(|owner| match owner {
                    OwnerStorage::BoxI32(value) => value.first_mut(),
                    _ => None,
                })
                .ok_or("E-TYPED-OWNER: missing allocation")? = value;
            Ok(Value::Unit)
        }
        ExprKind::OwnerBytesLen(id) => {
            let handle = owners.get(id).ok_or("E-TYPED-OWNER: missing bytes owner")?;
            match state.storage.get(&handle.id) {
                Some(OwnerStorage::Bytes(bytes)) => Ok(Value::Int(
                    i32::try_from(bytes.len()).map_err(|_| "runtime trap")?,
                )),
                _ => Err("E-TYPED-OWNER: wrong allocation kind".into()),
            }
        }
        ExprKind::OwnerBytesGet(id, index) => {
            let Value::Int(index) = evaluate_expr(index, locals, program, depth, state, owners)?
            else {
                return Err("E-TYPED-OWNER: invalid bytes index".into());
            };
            let handle = owners.get(id).ok_or("E-TYPED-OWNER: missing bytes owner")?;
            let index = usize::try_from(index).map_err(|_| "runtime trap")?;
            match state.storage.get(&handle.id) {
                Some(OwnerStorage::Bytes(bytes)) => bytes
                    .get(index)
                    .copied()
                    .map(|b| Value::Int(i32::from(b)))
                    .ok_or_else(|| "runtime trap".into()),
                _ => Err("E-TYPED-OWNER: wrong allocation kind".into()),
            }
        }
        ExprKind::OwnerBytesSet(id, index, value) => {
            let Value::Int(index) = evaluate_expr(index, locals, program, depth, state, owners)?
            else {
                return Err("E-TYPED-OWNER: invalid bytes index".into());
            };
            let Value::Int(value) = evaluate_expr(value, locals, program, depth, state, owners)?
            else {
                return Err("E-TYPED-OWNER: invalid byte".into());
            };
            let handle = owners.get(id).ok_or("E-TYPED-OWNER: missing bytes owner")?;
            let index = usize::try_from(index).map_err(|_| "runtime trap")?;
            let value = u8::try_from(value).map_err(|_| "runtime trap")?;
            match state.storage.get_mut(&handle.id) {
                Some(OwnerStorage::Bytes(bytes)) => {
                    *bytes.get_mut(index).ok_or("runtime trap")? = value;
                    Ok(Value::Unit)
                }
                _ => Err("E-TYPED-OWNER: wrong allocation kind".into()),
            }
        }
        ExprKind::Int(n) => Ok(Value::Int(*n)),
        ExprKind::Str(value) => Ok(Value::Str(value.clone())),
        ExprKind::Bool(value) => Ok(Value::Bool(*value)),
        ExprKind::Unit => Ok(Value::Unit),
        ExprKind::Read(id) => locals.read(id),
        ExprKind::Record { adt, fields } => {
            records::evaluate_constructor(*adt, fields, locals, program, depth, state, owners)
        }
        ExprKind::FieldRead(place) => locals.read_field(place),
        ExprKind::FieldAssign(place, value) => {
            let value = evaluate_expr(value, locals, program, depth, state, owners)?;
            locals.write_field(place, value)?;
            Ok(Value::Unit)
        }
        ExprKind::BytesNew(n) => Ok(Value::Bytes(vec![0; *n])),
        ExprKind::BytesLen(value) => {
            let value = evaluate_expr(value, locals, program, depth, state, owners)?;
            crate::interpreter::bytes_builtin("bytes_len", &[value])
        }
        ExprKind::BytesGet(value, index) => {
            let value = evaluate_expr(value, locals, program, depth, state, owners)?;
            let index = evaluate_expr(index, locals, program, depth, state, owners)?;
            crate::interpreter::bytes_builtin("bytes_get", &[value, index])
        }
        ExprKind::BytesSet(value, index, byte) => {
            let value = evaluate_expr(value, locals, program, depth, state, owners)?;
            let index = evaluate_expr(index, locals, program, depth, state, owners)?;
            let byte = evaluate_expr(byte, locals, program, depth, state, owners)?;
            crate::interpreter::bytes_builtin("bytes_set", &[value, index, byte])
        }
        ExprKind::ByteLen(value) => {
            let Value::Str(value) = evaluate_expr(value, locals, program, depth, state, owners)?
            else {
                return Err("E-TYPED: invalid string".into());
            };
            i32::try_from(value.len())
                .map(Value::Int)
                .map_err(|_| "runtime trap".into())
        }
        ExprKind::ByteAt(value, index) => {
            let Value::Str(value) = evaluate_expr(value, locals, program, depth, state, owners)?
            else {
                return Err("E-TYPED: invalid string".into());
            };
            let Value::Int(index) = evaluate_expr(index, locals, program, depth, state, owners)?
            else {
                return Err("E-TYPED: invalid index".into());
            };
            usize::try_from(index)
                .ok()
                .and_then(|i| value.as_bytes().get(i))
                .map(|b| Value::Int(i32::from(*b)))
                .ok_or_else(|| "runtime trap".into())
        }
        _ => Err("E-TYPED: invalid leaf operation".into()),
    }
}

enum OwnerStorage {
    Adt {
        adt: usize,
        variant: usize,
        fields: Vec<EvalArg>,
    },
    BoxI32(Vec<i32>),
    Bytes(Vec<u8>),
}

fn evaluate_call(
    id: usize,
    args: &[CallArg],
    locals: &mut CopyLocals,
    program: &TypedMain,
    depth: usize,
    state: &mut EvalState,
    owners: &mut HashMap<LocalId, OwnerHandle>,
) -> Result<EvalArg, String> {
    evaluate_call_outcome(id, args, locals, program, depth, state, owners)?.value()
}
fn evaluate_call_outcome(
    id: usize,
    args: &[CallArg],
    locals: &mut CopyLocals,
    program: &TypedMain,
    depth: usize,
    state: &mut EvalState,
    owners: &mut HashMap<LocalId, OwnerHandle>,
) -> Result<FunctionReturn, String> {
    let mut values = Vec::new();
    for arg in args {
        match arg {
            CallArg::Value(value) => values.push(EvalArg::Value(evaluate_expr(
                value, locals, program, depth, state, owners,
            )?)),
            CallArg::ReadCopy(id) | CallArg::InoutCopy(id) => {
                values.push(EvalArg::CopyLoan(locals.loan(id)?));
            }
            CallArg::ReadField(place) | CallArg::InoutField(place) => {
                values.push(EvalArg::CopyLoan(locals.loan_field(place)?));
            }
            CallArg::ReadTemporary(value) => {
                let value = evaluate_expr(value, locals, program, depth, state, owners)?;
                values.push(EvalArg::CopyLoan(copy_loans::slot(value)));
            }
            CallArg::Move(id) => values.push(EvalArg::Owner(
                owners
                    .remove(id)
                    .ok_or("E-TYPED-OWNER: missing move argument")?,
            )),
            CallArg::ReadOwner(id) | CallArg::InoutOwner(id) => {
                let handle = owners.get(id).ok_or("E-TYPED-OWNER: missing borrow root")?;
                let mode = if matches!(arg, CallArg::ReadOwner(_)) {
                    ParamMode::Read
                } else {
                    ParamMode::Inout
                };
                values.push(EvalArg::Owner(OwnerHandle {
                    id: handle.id,
                    mode,
                }));
            }
        }
    }
    if depth >= 32 {
        return Err("runtime trap".into());
    }
    evaluate_function_outcome(program, id, values, depth + 1, state)
}

fn evaluate_owner_expr(
    expr: &ExprNode,
    locals: &mut CopyLocals,
    program: &TypedMain,
    depth: usize,
    state: &mut EvalState,
    owners: &mut HashMap<LocalId, OwnerHandle>,
) -> Result<OwnerHandle, String> {
    match &expr.kind {
        ExprKind::TryAlloc {
            kind,
            input,
            layout,
        } => {
            let crate::interpreter::Value::Int(input) =
                evaluate_expr(input, locals, program, depth, state, owners)?
            else {
                return Err("E-TYPED-ALLOC: invalid input".into());
            };
            allocation::evaluate(*kind, input, layout, program, state)
        }
        ExprKind::CopyMatch { value, arms } => copy_match::evaluate_match(
            value,
            arms,
            locals,
            program,
            depth,
            state,
            owners,
            evaluate_owner_expr,
        ),
        ExprKind::Constructor {
            adt,
            variant,
            fields,
        } => match adt_eval::construct(
            *adt, *variant, fields, locals, program, depth, state, owners,
        )? {
            EvalArg::Owner(owner) => Ok(owner),
            _ => Err("E-TYPED-ADT: Copy constructor used as affine".into()),
        },
        ExprKind::Match { value, arms } => adt_eval::match_value(
            value,
            arms,
            locals,
            program,
            depth,
            state,
            owners,
            evaluate_owner_expr,
        ),
        ExprKind::If(condition, yes, no) => {
            match evaluate_expr(condition, locals, program, depth, state, owners)? {
                crate::interpreter::Value::Bool(true) => {
                    evaluate_owner_expr(yes, locals, program, depth, state, owners)
                }
                crate::interpreter::Value::Bool(false) => {
                    evaluate_owner_expr(no, locals, program, depth, state, owners)
                }
                _ => Err("E-TYPED-ADT: invalid condition".into()),
            }
        }
        ExprKind::OwnerMove(id) => {
            let handle = owners
                .remove(id)
                .ok_or("E-TYPED-OWNER: missing returned owner")?;
            if handle.mode != ParamMode::Value {
                return Err("E-TYPED-OWNER: returned borrowed owner".into());
            }
            Ok(handle)
        }
        ExprKind::Call(id, args) => {
            match evaluate_call(*id, args, locals, program, depth, state, owners)? {
                EvalArg::Owner(owner) => Ok(owner),
                EvalArg::Value(_) | EvalArg::CopyLoan(_) => {
                    Err("E-TYPED-OWNER: non-owner used as an owner".into())
                }
            }
        }
        ExprKind::OwnerLet { .. } => evaluate_owner_let(
            expr,
            locals,
            program,
            depth,
            state,
            owners,
            evaluate_owner_expr,
        ),
        ExprKind::Let { .. } => evaluate_lets(
            expr,
            locals,
            program,
            depth,
            state,
            owners,
            evaluate_owner_expr,
        ),
        _ => Err("E-TYPED-OWNER: unsupported owner result".into()),
    }
}

#[derive(Default)]
struct EvalState {
    storage: HashMap<usize, OwnerStorage>,
    next_id: usize,
    allocations: usize,
    frees: usize,
    #[cfg(test)]
    released: Vec<i32>,
    fail_after: Option<usize>,
    fail_at_attempt: Option<usize>,
    allocation_attempts: usize,
    #[cfg(test)]
    allocation_trace: Vec<(usize, SystemAllocationKind)>,
    #[cfg(test)]
    release_trace: Vec<(usize, SystemAllocationKind)>,
}
impl EvalState {
    fn release_cleanup(
        &mut self,
        handle: OwnerHandle,
        cleanup: &CleanupTree,
    ) -> Result<(), String> {
        if handle.mode != ParamMode::Value {
            return Err("E-TYPED-OWNER: attempted borrowed cleanup".into());
        }
        match cleanup {
            CleanupTree::DropWholeOwner => self.release(handle.id),
            _ => Err("E-TYPED-OWNER: unsupported aggregate cleanup".into()),
        }
    }

    fn begin_attempt(&mut self) -> Result<bool, String> {
        self.allocation_attempts = self
            .allocation_attempts
            .checked_add(1)
            .ok_or("E-TYPED-OWNER: allocation attempt overflow")?;
        Ok(self.fail_after == Some(self.allocations)
            || self.fail_at_attempt == Some(self.allocation_attempts))
    }
    fn allocate(&mut self, value: i32) -> Result<OwnerHandle, String> {
        self.try_allocate(value)?.map_err(|_| "runtime trap".into())
    }
    fn try_allocate(
        &mut self,
        value: i32,
    ) -> Result<Result<OwnerHandle, AllocationFailure>, String> {
        if self.begin_attempt()? {
            return Ok(Err(AllocationFailure::OutOfMemory));
        }
        self.storage
            .try_reserve(1)
            .map_err(|_| "E-TYPED-OWNER: interpreter storage exhausted")?;
        let id = self.next_id;
        let next = id
            .checked_add(1)
            .ok_or("E-TYPED-OWNER: identity overflow")?;
        let mut storage = Vec::new();
        if storage.try_reserve_exact(1).is_err() {
            return Ok(Err(AllocationFailure::OutOfMemory));
        }
        storage.push(value);
        self.storage.insert(id, OwnerStorage::BoxI32(storage));
        #[cfg(test)]
        self.allocation_trace
            .push((id, SystemAllocationKind::BoxI32));
        self.next_id = next;
        self.allocations += 1;
        Ok(Ok(OwnerHandle {
            id,
            mode: ParamMode::Value,
        }))
    }
    fn release(&mut self, id: usize) -> Result<(), String> {
        let storage = self
            .storage
            .remove(&id)
            .ok_or("E-TYPED-OWNER: missing allocation")?;
        if let OwnerStorage::Adt { fields, .. } = storage {
            for field in fields.into_iter().rev() {
                if let EvalArg::Owner(owner) = field {
                    self.release(owner.id)?;
                }
            }
            return Ok(());
        }
        self.frees += 1;
        #[cfg(test)]
        self.release_trace.push((
            id,
            match &storage {
                OwnerStorage::BoxI32(_) => SystemAllocationKind::BoxI32,
                OwnerStorage::Bytes(_) => SystemAllocationKind::BytesZeroed,
                OwnerStorage::Adt { .. } => unreachable!(),
            },
        ));
        #[cfg(test)]
        if let OwnerStorage::BoxI32(ref value) = storage {
            self.released.push(value[0]);
        }
        drop(storage);
        Ok(())
    }
    fn allocate_bytes(&mut self, len: i32) -> Result<OwnerHandle, String> {
        self.try_allocate_bytes(len)?
            .map_err(|_| "runtime trap".into())
    }
    fn try_allocate_bytes(
        &mut self,
        len: i32,
    ) -> Result<Result<OwnerHandle, AllocationFailure>, String> {
        if let Err(error) = allocation::checked_byte_size(
            len,
            std::mem::size_of::<usize>() as u128,
            usize::MAX as u128,
        ) {
            return Ok(Err(error));
        }
        let len = usize::try_from(len).map_err(|_| "E-TYPED-OWNER: invalid checked length")?;
        if self.begin_attempt()? {
            return Ok(Err(AllocationFailure::OutOfMemory));
        }
        self.storage
            .try_reserve(1)
            .map_err(|_| "E-TYPED-OWNER: interpreter storage exhausted")?;
        let id = self.next_id;
        let next = id
            .checked_add(1)
            .ok_or("E-TYPED-OWNER: identity overflow")?;
        let mut bytes = Vec::new();
        if bytes.try_reserve_exact(len.max(1)).is_err() {
            return Ok(Err(AllocationFailure::OutOfMemory));
        }
        bytes.resize(len, 0);
        self.storage.insert(id, OwnerStorage::Bytes(bytes));
        #[cfg(test)]
        self.allocation_trace
            .push((id, SystemAllocationKind::BytesZeroed));
        self.next_id = next;
        self.allocations += 1;
        Ok(Ok(OwnerHandle {
            id,
            mode: ParamMode::Value,
        }))
    }
}

type EvaluateBody<T> = fn(
    &ExprNode,
    &mut CopyLocals,
    &TypedMain,
    usize,
    &mut EvalState,
    &mut HashMap<LocalId, OwnerHandle>,
) -> Result<T, String>;

#[inline(never)]
fn evaluate_owner_let<T>(
    expr: &ExprNode,
    locals: &mut CopyLocals,
    program: &TypedMain,
    depth: usize,
    state: &mut EvalState,
    owners: &mut HashMap<LocalId, OwnerHandle>,
    evaluate_body: EvaluateBody<T>,
) -> Result<T, String> {
    use crate::interpreter::Value;
    let ExprKind::OwnerLet {
        local,
        init,
        body,
        cleanup,
        ..
    } = &expr.kind
    else {
        unreachable!()
    };
    let storage = match init {
        OwnerInit::Alloc(value) => {
            let Value::Int(value) = evaluate_expr(value, locals, program, depth, state, owners)?
            else {
                return Err("E-TYPED-OWNER: invalid allocation value".into());
            };
            state.allocate(value)?
        }
        OwnerInit::BytesAlloc(value) => {
            let Value::Int(value) = evaluate_expr(value, locals, program, depth, state, owners)?
            else {
                return Err("E-TYPED-OWNER: invalid allocation length".into());
            };
            state.allocate_bytes(value)?
        }
        OwnerInit::Call(value) | OwnerInit::Expr(value) => {
            evaluate_owner_expr(value, locals, program, depth, state, owners)?
        }
        OwnerInit::Move(source) => owners
            .remove(source)
            .ok_or("E-TYPED-OWNER: missing move source")?,
    };
    owners.insert(*local, storage);
    let result = evaluate_body(body, locals, program, depth, state, owners);
    if result.is_ok() {
        if let Some(cleanup) = cleanup {
            let storage = owners
                .remove(&cleanup.local)
                .ok_or("E-TYPED-OWNER: missing cleanup owner")?;
            if storage.mode != ParamMode::Value {
                return Err("E-TYPED-OWNER: attempted borrowed cleanup".into());
            }
            state.release_cleanup(storage, &cleanup.tree)?;
        }
    } else {
        owners.remove(local);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{check, parser};

    #[test]
    fn iterative_copy_lets_remove_scopes_on_success_and_traps() {
        use crate::interpreter::Value;
        for (source, expected) in [
            ("fn main() { let mut n = 0; let a = { n <- n * 10 + 1; n }; let b = { n <- n * 10 + 2; n }; let c = { n <- n * 10 + 3; n }; n }", Ok(Value::Int(123))),
            ("fn main() { let n = 1; let ignored = { let n = 2; n }; n }", Ok(Value::Int(1))),
            ("fn main() { let a = 40; let b = 1 / 0; let c = 2; a + b + c }", Err("runtime trap".to_owned())),
            ("fn main() { let a = 40; let b = 2; a / (b - 2) }", Err("runtime trap".to_owned())),
            ("fn main() { let a = 40; let b = { let c = 2; let d = 1 / 0; c + d }; a + b }", Err("runtime trap".to_owned())),
        ] {
            let program = parser::parse(source).unwrap();
            let checked = check::analyze(&program).unwrap();
            let typed = lower_main(&checked).unwrap();
            assert_eq!(crate::interpreter::run_reference(&program), expected);
            let outer = LocalId(u32::MAX);
            let mut locals = CopyLocals::default();
            locals.insert(outer, Value::Int(99));
            let mut owners = HashMap::new();
            let actual = evaluate_expr(
                typed.functions[typed.entry].body.value_expr().unwrap(),
                &mut locals,
                &typed,
                1,
                &mut EvalState::default(),
                &mut owners,
            );
            assert_eq!(actual, expected);
            assert_eq!(locals.snapshot(), HashMap::from([(outer, Value::Int(99))]));
            assert!(owners.is_empty());
        }
    }

    #[test]
    fn copy_let_chains_preserve_owner_return_cleanup_and_abort_policy() {
        for functions in [31, 32] {
            // The deep-block stack regression lives in an isolated subprocess.
            // Here only cleanup counts and the abort policy are under test.
            let locals = (0..2)
                .map(|i| format!("let x{i} = 0; "))
                .collect::<String>();
            let mut source = String::new();
            for index in 0..functions {
                let tail = if index + 1 == functions {
                    "move owner".to_owned()
                } else {
                    format!("f{}(move owner)", index + 1)
                };
                source.push_str(&format!(
                    "fn f{index}(owner) {{ let held = box_read(owner); {locals}{tail} }}\n"
                ));
            }
            source.push_str("fn main() { let owner = system_box_i32_abort(42); let returned = f0(move owner); box_read(returned) }");
            let program = parser::parse(&source).unwrap();
            let checked = check::analyze(&program).unwrap();
            let typed = lower_main(&checked).unwrap();
            let mut state = EvalState::default();
            let result = evaluate_function(&typed, typed.entry, vec![], 1, &mut state);
            if functions == 31 {
                assert!(matches!(
                    result,
                    Ok(EvalArg::Value(crate::interpreter::Value::Int(42)))
                ));
                assert_eq!((state.allocations, state.frees), (1, 1));
            } else {
                assert_eq!(result.unwrap_err(), "runtime trap");
                assert_eq!((state.allocations, state.frees), (1, 0));
            }
        }
    }

    #[test]
    fn checked_mutation_and_shadowing_have_distinct_locals() {
        let p = parser::parse("let main = let mut x = 1 in let ignored = (let x = true in x) in let done = x <- x + 1 in x").unwrap();
        let checked = check::analyze(&p).unwrap();
        let ir = lower_main(&checked).unwrap();
        verify(&ir).unwrap();
    }

    #[test]
    fn raw_c_values_use_explicit_checked_transport() {
        let p = parser::parse("fn main() { u32(0) }").unwrap();
        let checked = check::analyze(&p).unwrap();
        let ir = lower_main(&checked).unwrap();
        assert_eq!(ir.functions[ir.entry].result, Type::CU32);
        assert!(matches!(
            ir.functions[ir.entry].body.value_expr().unwrap().kind,
            ExprKind::ForeignConvert(ForeignConvert::U32, _)
        ));
        assert!(!emit_c(&ir).unwrap().contains("rl_value"));
    }

    #[test]
    fn verifier_rejects_out_of_scope_and_bad_types() {
        let p = TypedMain {
            logical: Default::default(),
            native: NativeManifest::default(),
            allocation_contract: None,
            adts: Vec::new(),
            entry: 0,
            functions: vec![TypedFunction {
                params: vec![],
                param_cleanup: vec![],
                result: (Type::Int).into(),
                body: (ExprNode {
                    ty: Type::Int,
                    kind: ExprKind::Read(LocalId(0)),
                })
                .into(),
            }],
        };
        assert!(verify(&p).unwrap_err().contains("outside local scope"));
        let p = TypedMain {
            logical: Default::default(),
            native: NativeManifest::default(),
            allocation_contract: None,
            adts: Vec::new(),
            entry: 0,
            functions: vec![TypedFunction {
                params: vec![],
                param_cleanup: vec![],
                result: (Type::Bool).into(),
                body: (ExprNode {
                    ty: Type::Bool,
                    kind: ExprKind::Int(1),
                })
                .into(),
            }],
        };
        assert!(verify(&p)
            .unwrap_err()
            .contains("incorrect expression type"));
    }
    #[test]
    fn verifier_rejects_invalid_string_operations() {
        let unit = || ExprNode {
            ty: Type::Unit,
            kind: ExprKind::Unit,
        };
        let body = ExprNode {
            ty: Type::Int,
            kind: ExprKind::ByteLen(Box::new(unit())),
        };
        let program = TypedMain {
            logical: Default::default(),
            native: NativeManifest::default(),
            allocation_contract: None,
            adts: Vec::new(),
            entry: 0,
            functions: vec![TypedFunction {
                params: vec![],
                param_cleanup: vec![],
                result: (Type::Int).into(),
                body: body.into(),
            }],
        };
        assert!(verify(&program).unwrap_err().contains("invalid byte_len"));
        let string = || ExprNode {
            ty: Type::StaticStr,
            kind: ExprKind::Str("a".into()),
        };
        let body = ExprNode {
            ty: Type::Bool,
            kind: ExprKind::Binary(BinaryOp::Eq, Box::new(string()), Box::new(string())),
        };
        let program = TypedMain {
            logical: Default::default(),
            native: NativeManifest::default(),
            allocation_contract: None,
            adts: Vec::new(),
            entry: 0,
            functions: vec![TypedFunction {
                params: vec![],
                param_cleanup: vec![],
                result: (Type::Bool).into(),
                body: body.into(),
            }],
        };
        assert!(verify(&program).unwrap_err().contains("invalid binary"));
    }

    #[test]
    fn verifier_rejects_invalid_buffer_operations() {
        let bytes = ExprNode {
            ty: Type::Bytes(1),
            kind: ExprKind::BytesNew(1),
        };
        let bad_index = ExprNode {
            ty: Type::Bool,
            kind: ExprKind::Bool(true),
        };
        let body = ExprNode {
            ty: Type::Int,
            kind: ExprKind::BytesGet(Box::new(bytes), Box::new(bad_index)),
        };
        let program = TypedMain {
            logical: Default::default(),
            native: NativeManifest::default(),
            allocation_contract: None,
            adts: Vec::new(),
            entry: 0,
            functions: vec![TypedFunction {
                params: vec![],
                param_cleanup: vec![],
                result: (Type::Int).into(),
                body: body.into(),
            }],
        };
        assert!(verify(&program).unwrap_err().contains("invalid bytes_get"));
        let body = ExprNode {
            ty: Type::Bytes(257),
            kind: ExprKind::BytesNew(257),
        };
        let program = TypedMain {
            logical: Default::default(),
            native: NativeManifest::default(),
            allocation_contract: None,
            adts: Vec::new(),
            entry: 0,
            functions: vec![TypedFunction {
                params: vec![],
                param_cleanup: vec![],
                result: (Type::Bytes(257)).into(),
                body: body.into(),
            }],
        };
        assert!(verify(&program)
            .unwrap_err()
            .contains("invalid buffer capacity"));
    }

    fn owner_program(body: ExprNode) -> TypedMain {
        TypedMain {
            logical: Default::default(),
            native: NativeManifest::default(),
            allocation_contract: None,
            adts: Vec::new(),
            entry: 0,
            functions: vec![TypedFunction {
                params: vec![],
                param_cleanup: vec![],
                result: (body.ty).into(),
                body: body.into(),
            }],
        }
    }
    fn int(value: i32) -> ExprNode {
        ExprNode {
            ty: Type::Int,
            kind: ExprKind::Int(value),
        }
    }
    fn owned(local: u32, init: OwnerInit, body: ExprNode, cleanup: Option<u32>) -> ExprNode {
        ExprNode {
            ty: body.ty,
            kind: ExprKind::OwnerLet {
                local: LocalId(local),
                mutable: true,
                init,
                body: Box::new(body),
                cleanup: cleanup.map(|id| Cleanup {
                    local: LocalId(id),
                    tree: CleanupTree::DropWholeOwner,
                }),
            },
        }
    }
    fn alloc(value: i32) -> OwnerInit {
        OwnerInit::Alloc(Box::new(int(value)))
    }

    #[test]
    fn bytes_owner_verifier_rejects_wrong_kind_and_overlapping_operands() {
        let bytes_len = ExprNode {
            ty: Type::Int,
            kind: ExprKind::OwnerBytesLen(LocalId(0)),
        };
        assert!(verify(&owner_program(owned(0, alloc(1), bytes_len, Some(0)))).is_err());
        let box_read = ExprNode {
            ty: Type::Int,
            kind: ExprKind::OwnerRead(LocalId(0)),
        };
        let bytes_alloc = || OwnerInit::BytesAlloc(Box::new(int(2)));
        assert!(verify(&owner_program(owned(0, bytes_alloc(), box_read, Some(0)))).is_err());
        let nested_read = ExprNode {
            ty: Type::Int,
            kind: ExprKind::OwnerBytesLen(LocalId(0)),
        };
        let set = ExprNode {
            ty: Type::Unit,
            kind: ExprKind::OwnerBytesSet(LocalId(0), Box::new(nested_read), Box::new(int(0))),
        };
        assert!(verify(&owner_program(owned(0, bytes_alloc(), set, Some(0)))).is_err());

        let call = ExprNode {
            ty: Type::Int,
            kind: ExprKind::Call(1, vec![CallArg::Move(LocalId(0))]),
        };
        let program = TypedMain {
            logical: Default::default(),
            native: NativeManifest::default(),
            allocation_contract: None,
            adts: Vec::new(),
            entry: 0,
            functions: vec![
                TypedFunction {
                    params: vec![],
                    result: (Type::Int).into(),
                    body: (owned(0, bytes_alloc(), call, None)).into(),
                    param_cleanup: vec![],
                },
                TypedFunction {
                    params: vec![(LocalId(1), Type::OwnedI32, ParamMode::Value)],
                    result: (Type::Int).into(),
                    body: (int(0)).into(),
                    param_cleanup: vec![Cleanup {
                        local: LocalId(1),
                        tree: CleanupTree::DropWholeOwner,
                    }],
                },
            ],
        };
        assert!(verify(&program).is_err());

        for (mode, arg) in [
            (ParamMode::Read, CallArg::ReadOwner(LocalId(0))),
            (ParamMode::Inout, CallArg::InoutOwner(LocalId(0))),
        ] {
            let call = ExprNode {
                ty: Type::Int,
                kind: ExprKind::Call(1, vec![arg]),
            };
            let program = TypedMain {
                logical: Default::default(),
                native: NativeManifest::default(),
                allocation_contract: None,
                adts: Vec::new(),
                entry: 0,
                functions: vec![
                    TypedFunction {
                        params: vec![],
                        result: (Type::Int).into(),
                        body: (owned(0, bytes_alloc(), call, Some(0))).into(),
                        param_cleanup: vec![],
                    },
                    TypedFunction {
                        params: vec![(LocalId(1), Type::OwnedI32, mode)],
                        result: (Type::Int).into(),
                        body: (int(0)).into(),
                        param_cleanup: vec![],
                    },
                ],
            };
            assert!(verify(&program).is_err());
        }
    }

    #[test]
    fn ownership_verifier_rejects_forged_moves_and_cleanup() {
        let moved_then_dropped_twice = owned(
            0,
            alloc(1),
            owned(1, OwnerInit::Move(LocalId(0)), int(0), Some(1)),
            Some(0),
        );
        assert!(verify(&owner_program(moved_then_dropped_twice))
            .unwrap_err()
            .contains("invalid cleanup"));
        let second_move = owned(
            0,
            alloc(1),
            owned(
                1,
                OwnerInit::Move(LocalId(0)),
                owned(2, OwnerInit::Move(LocalId(0)), int(0), Some(2)),
                Some(1),
            ),
            None,
        );
        assert!(verify(&owner_program(second_move))
            .unwrap_err()
            .contains("invalid move"));
        let read_moved = owned(
            0,
            alloc(1),
            owned(
                1,
                OwnerInit::Move(LocalId(0)),
                ExprNode {
                    ty: Type::Int,
                    kind: ExprKind::OwnerRead(LocalId(0)),
                },
                Some(1),
            ),
            None,
        );
        assert!(verify(&owner_program(read_moved))
            .unwrap_err()
            .contains("invalid owner read"));
        let copy_owner = owned(
            0,
            alloc(1),
            ExprNode {
                ty: Type::OwnedI32,
                kind: ExprKind::Read(LocalId(0)),
            },
            Some(0),
        );
        assert!(verify(&owner_program(copy_owner)).is_err());
        let right = ExprNode {
            ty: Type::Int,
            kind: ExprKind::OwnerRead(LocalId(0)),
        };
        let write_during_loan = owned(
            0,
            alloc(1),
            ExprNode {
                ty: Type::Unit,
                kind: ExprKind::OwnerWrite(LocalId(0), Box::new(right)),
            },
            Some(0),
        );
        assert!(verify(&owner_program(write_during_loan))
            .unwrap_err()
            .contains("invalid owner read"));
    }

    #[test]
    fn ownership_verifier_rejects_conditional_and_loop_moves() {
        let moved_branch = owned(1, OwnerInit::Move(LocalId(0)), int(0), Some(1));
        let conditional = ExprNode {
            ty: Type::Int,
            kind: ExprKind::If(
                Box::new(ExprNode {
                    ty: Type::Bool,
                    kind: ExprKind::Bool(true),
                }),
                Box::new(moved_branch),
                Box::new(int(0)),
            ),
        };
        let p = owner_program(owned(0, alloc(1), conditional, None));
        assert!(verify(&p).unwrap_err().contains("move in conditional"));
        let body = owned(
            1,
            OwnerInit::Move(LocalId(0)),
            ExprNode {
                ty: Type::Unit,
                kind: ExprKind::Unit,
            },
            Some(1),
        );
        let loop_node = ExprNode {
            ty: Type::Unit,
            kind: ExprKind::While(
                Box::new(ExprNode {
                    ty: Type::Bool,
                    kind: ExprKind::Bool(false),
                }),
                Box::new(body),
            ),
        };
        let p = owner_program(owned(0, alloc(1), loop_node, None));
        assert!(verify(&p).unwrap_err().contains("move in loop"));
    }

    #[test]
    fn ownership_verifier_rejects_invalid_call_operands_and_parameter_cleanup() {
        let owner_params = |count: u32, cleanup: Vec<LocalId>| TypedFunction {
            params: (0..count)
                .map(|i| (LocalId(i), Type::OwnedI32, ParamMode::Value))
                .collect(),
            result: (Type::Int).into(),
            body: (int(0)).into(),
            param_cleanup: cleanup
                .into_iter()
                .map(|local| Cleanup {
                    local,
                    tree: CleanupTree::DropWholeOwner,
                })
                .collect(),
        };
        let call = |args| ExprNode {
            ty: Type::Int,
            kind: ExprKind::Call(0, args),
        };
        let main = |body: ExprNode| TypedFunction {
            params: vec![],
            result: (Type::Int).into(),
            body: body.into(),
            param_cleanup: vec![],
        };
        let p = TypedMain {
            logical: Default::default(),
            native: NativeManifest::default(),
            allocation_contract: None,
            adts: Vec::new(),
            entry: 1,
            functions: vec![
                owner_params(2, vec![LocalId(1), LocalId(0)]),
                main(owned(
                    2,
                    alloc(1),
                    call(vec![CallArg::Move(LocalId(2)), CallArg::Move(LocalId(2))]),
                    None,
                )),
            ],
        };
        assert!(verify(&p).unwrap_err().contains("invalid move argument"));
        let p = TypedMain {
            logical: Default::default(),
            native: NativeManifest::default(),
            allocation_contract: None,
            adts: Vec::new(),
            entry: 1,
            functions: vec![
                owner_params(1, vec![LocalId(0)]),
                main(call(vec![CallArg::Value(int(1))])),
            ],
        };
        assert!(verify(&p)
            .unwrap_err()
            .contains("invalid call argument mode"));
        let scalar = TypedFunction {
            params: vec![(LocalId(0), Type::Int, ParamMode::Value)],
            result: (Type::Int).into(),
            body: (int(0)).into(),
            param_cleanup: vec![],
        };
        let p = TypedMain {
            logical: Default::default(),
            native: NativeManifest::default(),
            allocation_contract: None,
            adts: Vec::new(),
            entry: 1,
            functions: vec![
                scalar,
                main(owned(
                    1,
                    alloc(1),
                    call(vec![CallArg::Move(LocalId(1))]),
                    None,
                )),
            ],
        };
        assert!(verify(&p)
            .unwrap_err()
            .contains("invalid call argument mode"));
        for cleanup in [vec![], vec![LocalId(0), LocalId(1)]] {
            let p = TypedMain {
                logical: Default::default(),
                native: NativeManifest::default(),
                allocation_contract: None,
                adts: Vec::new(),
                entry: 1,
                functions: vec![owner_params(2, cleanup), main(int(0))],
            };
            assert!(verify(&p)
                .unwrap_err()
                .contains("invalid parameter cleanup"));
        }
    }

    #[test]
    fn ownership_verifier_rejects_forged_borrow_aliases_and_cleanup() {
        let function = |modes: Vec<ParamMode>| TypedFunction {
            params: modes
                .into_iter()
                .enumerate()
                .map(|(i, mode)| (LocalId(i as u32 + 1), Type::OwnedI32, mode))
                .collect(),
            result: (Type::Int).into(),
            body: (int(0)).into(),
            param_cleanup: vec![],
        };
        let main = |body: ExprNode| TypedFunction {
            params: vec![],
            result: (Type::Int).into(),
            body: body.into(),
            param_cleanup: vec![],
        };
        let call = |id, args| ExprNode {
            ty: Type::Int,
            kind: ExprKind::Call(id, args),
        };
        for (modes, args) in [
            (
                vec![ParamMode::Inout, ParamMode::Inout],
                vec![
                    CallArg::InoutOwner(LocalId(0)),
                    CallArg::InoutOwner(LocalId(0)),
                ],
            ),
            (
                vec![ParamMode::Read, ParamMode::Inout],
                vec![
                    CallArg::ReadOwner(LocalId(0)),
                    CallArg::InoutOwner(LocalId(0)),
                ],
            ),
            (
                vec![ParamMode::Inout, ParamMode::Read],
                vec![
                    CallArg::InoutOwner(LocalId(0)),
                    CallArg::ReadOwner(LocalId(0)),
                ],
            ),
        ] {
            let program = TypedMain {
                logical: Default::default(),
                native: NativeManifest::default(),
                allocation_contract: None,
                adts: Vec::new(),
                entry: 1,
                functions: vec![
                    function(modes),
                    main(owned(0, alloc(1), call(0, args), Some(0))),
                ],
            };
            assert!(verify(&program).unwrap_err().contains("loan"));
        }
        let good = TypedMain {
            logical: Default::default(),
            native: NativeManifest::default(),
            allocation_contract: None,
            adts: Vec::new(),
            entry: 1,
            functions: vec![
                function(vec![ParamMode::Read, ParamMode::Read]),
                main(owned(
                    0,
                    alloc(1),
                    call(
                        0,
                        vec![
                            CallArg::ReadOwner(LocalId(0)),
                            CallArg::ReadOwner(LocalId(0)),
                        ],
                    ),
                    Some(0),
                )),
            ],
        };
        verify(&good).unwrap();
        let bad_move = TypedFunction {
            params: vec![(LocalId(1), Type::OwnedI32, ParamMode::Read)],
            result: (Type::Int).into(),
            body: (owned(2, OwnerInit::Move(LocalId(1)), int(0), Some(2))).into(),
            param_cleanup: vec![],
        };
        let program = TypedMain {
            logical: Default::default(),
            native: NativeManifest::default(),
            allocation_contract: None,
            adts: Vec::new(),
            entry: 1,
            functions: vec![bad_move, main(int(0))],
        };
        assert!(verify(&program).unwrap_err().contains("invalid move"));
        let bad_cleanup = TypedFunction {
            param_cleanup: vec![Cleanup {
                local: LocalId(1),
                tree: CleanupTree::DropWholeOwner,
            }],
            ..function(vec![ParamMode::Read])
        };
        let program = TypedMain {
            logical: Default::default(),
            native: NativeManifest::default(),
            allocation_contract: None,
            adts: Vec::new(),
            entry: 1,
            functions: vec![bad_cleanup, main(int(0))],
        };
        assert!(verify(&program)
            .unwrap_err()
            .contains("invalid parameter cleanup"));
    }

    #[test]
    fn ownership_verifier_rejects_nested_argument_reborrow() {
        let outer = TypedFunction {
            params: vec![
                (LocalId(1), Type::OwnedI32, ParamMode::Read),
                (LocalId(2), Type::Int, ParamMode::Value),
            ],
            result: (Type::Int).into(),
            body: (int(0)).into(),
            param_cleanup: vec![],
        };
        let inner = TypedFunction {
            params: vec![(LocalId(3), Type::OwnedI32, ParamMode::Inout)],
            result: (Type::Int).into(),
            body: (int(0)).into(),
            param_cleanup: vec![],
        };
        let inner_call = ExprNode {
            ty: Type::Int,
            kind: ExprKind::Call(1, vec![CallArg::InoutOwner(LocalId(0))]),
        };
        let outer_call = ExprNode {
            ty: Type::Int,
            kind: ExprKind::Call(
                0,
                vec![CallArg::ReadOwner(LocalId(0)), CallArg::Value(inner_call)],
            ),
        };
        let main = TypedFunction {
            params: vec![],
            result: (Type::Int).into(),
            body: (owned(0, alloc(1), outer_call, Some(0))).into(),
            param_cleanup: vec![],
        };
        let program = TypedMain {
            logical: Default::default(),
            native: NativeManifest::default(),
            allocation_contract: None,
            adts: Vec::new(),
            entry: 2,
            functions: vec![outer, inner, main],
        };
        assert!(verify(&program).unwrap_err().contains("exclusive loan"));
    }

    #[test]
    fn ownership_verifier_rejects_recursive_call_graphs() {
        let call = |id| ExprNode {
            ty: Type::Int,
            kind: ExprKind::Call(id, vec![]),
        };
        let function = |body: ExprNode| TypedFunction {
            params: vec![],
            result: (Type::Int).into(),
            body: body.into(),
            param_cleanup: vec![],
        };
        let p = TypedMain {
            logical: Default::default(),
            native: NativeManifest::default(),
            allocation_contract: None,
            adts: Vec::new(),
            entry: 0,
            functions: vec![function(owned(0, alloc(1), call(0), Some(0)))],
        };
        assert!(verify(&p).unwrap_err().contains("recursive owner programs"));
        let p = TypedMain {
            logical: Default::default(),
            native: NativeManifest::default(),
            allocation_contract: None,
            adts: Vec::new(),
            entry: 0,
            functions: vec![
                function(owned(0, alloc(1), call(1), Some(0))),
                function(call(0)),
            ],
        };
        assert!(verify(&p).unwrap_err().contains("recursive owner programs"));
    }

    #[test]
    fn returned_owner_runtime_transfers_cleanup_and_preserves_traps() {
        let source = parser::parse("let make n = let returned = system_box_i32_abort(n) in let scratch = system_box_i32_abort(7) in move returned\nlet main = let owner = make(42) in box_read(owner)").unwrap();
        let checked = check::analyze(&source).unwrap();
        let typed = lower_main(&checked).unwrap();
        let mut state = EvalState::default();
        assert!(matches!(
            evaluate_function(&typed, typed.entry, Vec::new(), 1, &mut state).unwrap(),
            EvalArg::Value(crate::interpreter::Value::Int(42))
        ));
        assert_eq!(state.allocations, 2);
        assert_eq!(state.released, vec![7, 42]);
        assert!(state.storage.is_empty());
        for failed in [0, 1] {
            let mut state = EvalState {
                fail_after: Some(failed),
                ..EvalState::default()
            };
            assert_eq!(
                evaluate_function(&typed, typed.entry, Vec::new(), 1, &mut state).unwrap_err(),
                "runtime trap"
            );
            assert_eq!(state.allocations, failed);
            // Abort policy promises no Rill cleanup after an allocation trap.
            assert_eq!(state.frees, 0);
        }
    }

    #[test]
    fn verifier_rejects_forged_owner_returns_and_initializers() {
        let source = parser::parse("let pass owner = let n = box_read(owner) in move owner\nlet main = let original = system_box_i32_abort(42) in let returned = pass(move original) in box_read(returned)").unwrap();
        let checked = check::analyze(&source).unwrap();
        let typed = lower_main(&checked).unwrap();
        verify(&typed).unwrap();
        let returned_param = typed.functions[0].params[0].0;

        let mut cleanup_return = typed.clone();
        cleanup_return.functions[0].param_cleanup.push(Cleanup {
            local: returned_param,
            tree: CleanupTree::DropWholeOwner,
        });
        assert!(verify(&cleanup_return).is_err());

        let mut borrow_return = typed.clone();
        borrow_return.functions[0].params[0].2 = ParamMode::Read;
        assert!(verify(&borrow_return).is_err());

        let mut copied_return = typed.clone();
        copied_return.functions[0].body = (ExprNode {
            ty: Type::OwnedI32,
            kind: ExprKind::Read(returned_param),
        })
        .into();
        assert!(verify(&copied_return).is_err());

        let mut recursive = typed.clone();
        let owner_call = ExprNode {
            ty: Type::OwnedI32,
            kind: ExprKind::Call(0, vec![CallArg::Move(returned_param)]),
        };
        recursive.functions[0].body = (ExprNode {
            ty: Type::OwnedI32,
            kind: ExprKind::OwnerLet {
                local: LocalId(900),
                mutable: false,
                init: OwnerInit::Call(Box::new(owner_call)),
                body: Box::new(ExprNode {
                    ty: Type::OwnedI32,
                    kind: ExprKind::OwnerMove(LocalId(900)),
                }),
                cleanup: None,
            },
        })
        .into();
        assert!(verify(&recursive).unwrap_err().contains("recursive"));

        let mut invalid_initializer = typed.clone();
        invalid_initializer.functions[0].body = (ExprNode {
            ty: Type::OwnedI32,
            kind: ExprKind::OwnerLet {
                local: LocalId(900),
                mutable: false,
                init: OwnerInit::Call(Box::new(ExprNode {
                    ty: Type::OwnedI32,
                    kind: ExprKind::OwnerMove(returned_param),
                })),
                body: Box::new(ExprNode {
                    ty: Type::OwnedI32,
                    kind: ExprKind::OwnerMove(LocalId(900)),
                }),
                cleanup: None,
            },
        })
        .into();
        assert!(verify(&invalid_initializer)
            .unwrap_err()
            .contains("direct call"));

        let mut ordinary_let = typed;
        ordinary_let.functions[0].body = (ExprNode {
            ty: Type::OwnedI32,
            kind: ExprKind::Let {
                local: LocalId(900),
                mutable: false,
                value: Box::new(ExprNode {
                    ty: Type::OwnedI32,
                    kind: ExprKind::OwnerMove(returned_param),
                }),
                body: Box::new(ExprNode {
                    ty: Type::OwnedI32,
                    kind: ExprKind::OwnerMove(LocalId(900)),
                }),
            },
        })
        .into();
        assert!(verify(&ordinary_let).unwrap_err().contains("explicit init"));
    }

    #[test]
    fn owner_allocator_tracks_cleanup_and_oom() {
        for (source, allocations, releases) in [
            ("let main = let a = system_box_i32_abort(1) in let b = system_box_i32_abort(2) in box_read(a) + box_read(b)", 2, vec![2, 1]),
            ("let main = let a = system_box_i32_abort(1) in let b = move a in box_read(b)", 1, vec![1]),
            ("let main = let mut i = 0 in let done = while i < 3 do let b = system_box_i32_abort(i) in i <- i + 1 in i", 3, vec![0, 1, 2]),
        ] {
            let source = parser::parse(source).unwrap();
            let checked = check::analyze(&source).unwrap();
            let typed = lower_main(&checked).unwrap();
            let mut state = EvalState::default();
            evaluate_function(&typed, typed.entry, Vec::new(), 1, &mut state).unwrap();
            assert_eq!(state.allocations, allocations);
            assert_eq!(state.frees, releases.len());
            assert_eq!(state.released, releases);
        }
        let source =
            parser::parse("let main = let b = system_box_i32_abort(1) in box_read(b)").unwrap();
        let checked = check::analyze(&source).unwrap();
        let typed = lower_main(&checked).unwrap();
        let mut state = EvalState {
            fail_after: Some(0),
            ..EvalState::default()
        };
        assert_eq!(
            evaluate_function(&typed, typed.entry, Vec::new(), 1, &mut state).unwrap_err(),
            "runtime trap"
        );
        assert_eq!(state.allocations, 0);
        assert_eq!(state.frees, 0);
    }

    #[test]
    fn typed_evaluator_matches_checked_ast_interpreter() {
        for source in [
            "let main = 2 + 3 * 4",
            "let main = 2147483647 + 1",
            "let main = false && (1 / 0 == 0)",
            "let main = if true then 42 else 1 / 0",
            "let main = let mut x = 0 in let ignored = while x < 100 do x <- x + 1 in x",
            "let main = let mut x = 1 in let ignored = (let x = true in x) in let done = x <- x + 1 in x",
            "let main = let mut x = 0 in let ignored = x <- (let done = x <- 1 in x + 1) in x",
        ] {
            let program = parser::parse(source).unwrap();
            let checked = check::analyze(&program).unwrap();
            let ir = lower_main(&checked).unwrap();
            assert_eq!(evaluate(&ir), crate::interpreter::run_reference(&program), "{source}");
        }
    }
}

#[cfg(test)]
#[path = "typed/independent_interrupted_tests.rs"]
mod independent_interrupted_tests;

#[cfg(test)]
#[path = "typed/independent_discarded_proof_tests.rs"]
mod independent_discarded_proof_tests;

#[cfg(test)]
#[path = "typed/independent_discarded_internal_tests.rs"]
mod independent_discarded_internal_tests;

#[cfg(test)]
#[path = "typed/independent_discarded_suffix_tests.rs"]
mod independent_discarded_suffix_tests;

#[cfg(test)]
#[path = "typed/independent_discarded_record_tests.rs"]
mod independent_discarded_record_tests;
