//! Independent, bounded inspection of the final Copy function-body IR.
//!
//! This module never consults construction traces or source syntax. Slots are
//! real stored occurrences (including operands represented by attributes), not
//! invented expression nodes. Layouts are checked for the Copy boundary but do
//! not receive body-node identities.
use super::BodyBudget;
use crate::{
    ast::{BinaryOp, ParamMode},
    typed::{
        CallArg, CopyArm, CopyPattern, ExprKind, ExprNode, FieldPlace, FieldStep, LocalId, Type,
        TypedMain, VectorOp,
    },
};
use std::iter::FusedIterator;

/// An edge in the actual typed function-body structure. Ordinals use stored
/// order, which is written/evaluation order for record values and operands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TypedChildRole {
    FunctionRoot,
    Parameter(usize),
    Result,
    FunctionBody,
    LetValue,
    LetBody,
    BinaryLeft,
    BinaryRight,
    Condition,
    Then,
    Else,
    LoopBody,
    AssignmentValue,
    BuiltinOperand(usize),
    ConstructorField(usize),
    MatchScrutinee,
    MatchArm(usize),
    ArmPattern,
    ArmBody,
    PatternField(usize),
    CallArgument(usize),
    ArgumentValue,
    RecordField(usize),
    RecordValue,
    Capacity,
}

/// Structural occurrence category. Exact operation/type attributes remain
/// available through the immutable corresponding `TypedNodeRef`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TypedNodeKind {
    Function,
    Parameter,
    Result,
    Expression,
    Argument,
    RecordField,
    Arm,
    Pattern,
    Capacity,
}

/// A shared borrow of the exact paired IR, never a reconstructed node.
/// Raw `Type` and `LocalId` attributes remain compilation-local IR values.
#[derive(Clone, Copy, Debug)]
pub enum TypedNodeRef<'a> {
    Function {
        params: &'a [(LocalId, Type, ParamMode)],
        result: &'a Type,
        body: &'a ExprNode,
    },
    Parameter(&'a (LocalId, Type, ParamMode)),
    Result(&'a Type),
    Expression(&'a ExprNode),
    Argument(&'a CallArg),
    RecordField {
        written: usize,
        declaration: usize,
        value: &'a ExprNode,
    },
    Arm(&'a CopyArm),
    Pattern(&'a CopyPattern),
    Capacity(&'a usize),
}

impl<'a> TypedNodeRef<'a> {
    pub fn kind(&self) -> TypedNodeKind {
        match self {
            Self::Function { .. } => TypedNodeKind::Function,
            Self::Parameter(_) => TypedNodeKind::Parameter,
            Self::Result(_) => TypedNodeKind::Result,
            Self::Expression(_) => TypedNodeKind::Expression,
            Self::Argument(_) => TypedNodeKind::Argument,
            Self::RecordField { .. } => TypedNodeKind::RecordField,
            Self::Arm(_) => TypedNodeKind::Arm,
            Self::Pattern(_) => TypedNodeKind::Pattern,
            Self::Capacity(_) => TypedNodeKind::Capacity,
        }
    }

    /// Direct children only, in semantic preorder. This iterator allocates
    /// nothing and does not expand descendants, call targets or shared shapes.
    /// Owner/native operations are outside this schema and have no children;
    /// the final enumerator rejects them before publishing any result.
    pub fn children(
        self,
    ) -> impl DoubleEndedIterator<Item = (TypedChildRole, TypedNodeRef<'a>)>
           + ExactSizeIterator
           + FusedIterator {
        Children {
            node: self,
            front: 0,
            back: child_count(self),
        }
    }
}

struct Children<'a> {
    node: TypedNodeRef<'a>,
    front: usize,
    back: usize,
}
impl<'a> Iterator for Children<'a> {
    type Item = (TypedChildRole, TypedNodeRef<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.front == self.back {
            return None;
        }
        let index = self.front;
        self.front += 1;
        child_at(self.node, index)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.back - self.front;
        (remaining, Some(remaining))
    }
}
impl DoubleEndedIterator for Children<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.front == self.back {
            return None;
        }
        self.back -= 1;
        child_at(self.node, self.back)
    }
}
impl ExactSizeIterator for Children<'_> {}
impl FusedIterator for Children<'_> {}

fn child_count(node: TypedNodeRef<'_>) -> usize {
    match node {
        TypedNodeRef::Function { params, .. } => params.len() + 2,
        TypedNodeRef::Parameter(_) | TypedNodeRef::Result(_) | TypedNodeRef::Capacity(_) => 0,
        TypedNodeRef::RecordField { .. } => 1,
        TypedNodeRef::Arm(_) => 2,
        TypedNodeRef::Pattern(pattern) => match pattern {
            CopyPattern::Constructor { fields, .. } => fields.len(),
            CopyPattern::Wildcard
            | CopyPattern::Binding(_)
            | CopyPattern::Int(_)
            | CopyPattern::Bool(_)
            | CopyPattern::Unit => 0,
        },
        TypedNodeRef::Argument(argument) => match argument {
            CallArg::Value(_) | CallArg::ReadTemporary(_) => 1,
            CallArg::ReadCopy(_)
            | CallArg::InoutCopy(_)
            | CallArg::ReadField(_)
            | CallArg::InoutField(_)
            | CallArg::Move(_)
            | CallArg::ReadOwner(_)
            | CallArg::InoutOwner(_) => 0,
        },
        TypedNodeRef::Expression(expression) => match &expression.kind {
            ExprKind::NoStorageLet { .. }
            | ExprKind::DiscardLet { .. }
            | ExprKind::InterruptedCall(_) => 0,
            ExprKind::Record { fields, .. } => fields.len(),
            ExprKind::Call(_, arguments) => arguments.len(),
            ExprKind::Constructor { fields, .. } => fields.len(),
            ExprKind::CopyMatch { arms, .. } => arms.len() + 1,
            ExprKind::Vector(_, operands) => operands.len(),
            ExprKind::FieldAssign(_, _)
            | ExprKind::Assign(_, _)
            | ExprKind::ByteLen(_)
            | ExprKind::BytesLen(_)
            | ExprKind::BytesNew(_) => 1,
            ExprKind::Let { .. }
            | ExprKind::Binary(_, _, _)
            | ExprKind::While(_, _)
            | ExprKind::ByteAt(_, _)
            | ExprKind::BytesGet(_, _) => 2,
            ExprKind::If(_, _, _) | ExprKind::BytesSet(_, _, _) => 3,
            ExprKind::Int(_)
            | ExprKind::Str(_)
            | ExprKind::Bool(_)
            | ExprKind::Unit
            | ExprKind::Read(_)
            | ExprKind::FieldRead(_)
            | ExprKind::TryAlloc { .. }
            | ExprKind::ForeignCall(_, _)
            | ExprKind::ForeignConst(_)
            | ExprKind::ForeignConvert(_, _)
            | ExprKind::NullPointer
            | ExprKind::IsNull(_)
            | ExprKind::Match { .. }
            | ExprKind::OwnerLet { .. }
            | ExprKind::OwnerBytesLen(_)
            | ExprKind::OwnerBytesGet(_, _)
            | ExprKind::OwnerBytesSet(_, _, _)
            | ExprKind::OwnerRead(_)
            | ExprKind::OwnerMove(_)
            | ExprKind::OwnerWrite(_, _) => 0,
        },
    }
}

fn child_at(node: TypedNodeRef<'_>, index: usize) -> Option<(TypedChildRole, TypedNodeRef<'_>)> {
    use TypedChildRole as Role;
    use TypedNodeRef as Node;
    let expression = |role, value| Some((role, Node::Expression(value)));
    match node {
        Node::Function {
            params,
            result,
            body,
        } => {
            if let Some(param) = params.get(index) {
                Some((Role::Parameter(index), Node::Parameter(param)))
            } else if index == params.len() {
                Some((Role::Result, Node::Result(result)))
            } else if index == params.len() + 1 {
                expression(Role::FunctionBody, body)
            } else {
                None
            }
        }
        Node::Parameter(_) | Node::Result(_) | Node::Capacity(_) => None,
        Node::RecordField { value, .. } => {
            (index == 0).then_some((Role::RecordValue, Node::Expression(value)))
        }
        Node::Arm(arm) => match index {
            0 => Some((Role::ArmPattern, Node::Pattern(&arm.pattern))),
            1 => expression(Role::ArmBody, &arm.body),
            _ => None,
        },
        Node::Pattern(pattern) => match pattern {
            CopyPattern::Constructor { fields, .. } => fields
                .get(index)
                .map(|field| (Role::PatternField(index), Node::Pattern(field))),
            CopyPattern::Wildcard
            | CopyPattern::Binding(_)
            | CopyPattern::Int(_)
            | CopyPattern::Bool(_)
            | CopyPattern::Unit => None,
        },
        Node::Argument(argument) => match argument {
            CallArg::Value(value) | CallArg::ReadTemporary(value) => {
                (index == 0).then_some((Role::ArgumentValue, Node::Expression(value)))
            }
            CallArg::ReadCopy(_)
            | CallArg::InoutCopy(_)
            | CallArg::ReadField(_)
            | CallArg::InoutField(_)
            | CallArg::Move(_)
            | CallArg::ReadOwner(_)
            | CallArg::InoutOwner(_) => None,
        },
        Node::Expression(value) => match &value.kind {
            ExprKind::NoStorageLet { .. }
            | ExprKind::DiscardLet { .. }
            | ExprKind::InterruptedCall(_) => None,
            ExprKind::Record { fields, .. } => fields.get(index).map(|(declaration, value)| {
                (
                    Role::RecordField(index),
                    Node::RecordField {
                        written: index,
                        declaration: *declaration,
                        value,
                    },
                )
            }),
            ExprKind::Call(_, args) => args
                .get(index)
                .map(|arg| (Role::CallArgument(index), Node::Argument(arg))),
            ExprKind::Constructor { fields, .. } => fields
                .get(index)
                .map(|field| (Role::ConstructorField(index), Node::Expression(field))),
            ExprKind::CopyMatch { value, arms } => {
                if index == 0 {
                    expression(Role::MatchScrutinee, value)
                } else {
                    arms.get(index - 1)
                        .map(|arm| (Role::MatchArm(index - 1), Node::Arm(arm)))
                }
            }
            ExprKind::Let { value, body, .. } => match index {
                0 => expression(Role::LetValue, value),
                1 => expression(Role::LetBody, body),
                _ => None,
            },
            ExprKind::Binary(_, left, right) => match index {
                0 => expression(Role::BinaryLeft, left),
                1 => expression(Role::BinaryRight, right),
                _ => None,
            },
            ExprKind::If(condition, then, otherwise) => match index {
                0 => expression(Role::Condition, condition),
                1 => expression(Role::Then, then),
                2 => expression(Role::Else, otherwise),
                _ => None,
            },
            ExprKind::While(condition, body) => match index {
                0 => expression(Role::Condition, condition),
                1 => expression(Role::LoopBody, body),
                _ => None,
            },
            ExprKind::Assign(_, value) | ExprKind::FieldAssign(_, value) => {
                (index == 0).then_some((Role::AssignmentValue, Node::Expression(value)))
            }
            ExprKind::ByteLen(value) | ExprKind::BytesLen(value) => {
                (index == 0).then_some((Role::BuiltinOperand(0), Node::Expression(value)))
            }
            ExprKind::ByteAt(first, second) | ExprKind::BytesGet(first, second) => match index {
                0 => expression(Role::BuiltinOperand(0), first),
                1 => expression(Role::BuiltinOperand(1), second),
                _ => None,
            },
            ExprKind::BytesSet(first, second, third) => match index {
                0 => expression(Role::BuiltinOperand(0), first),
                1 => expression(Role::BuiltinOperand(1), second),
                2 => expression(Role::BuiltinOperand(2), third),
                _ => None,
            },
            ExprKind::Vector(_, operands) => operands
                .get(index)
                .map(|value| (Role::BuiltinOperand(index), Node::Expression(value))),
            ExprKind::BytesNew(capacity) => {
                (index == 0).then_some((Role::Capacity, Node::Capacity(capacity)))
            }
            ExprKind::Int(_)
            | ExprKind::Str(_)
            | ExprKind::Bool(_)
            | ExprKind::Unit
            | ExprKind::Read(_)
            | ExprKind::FieldRead(_)
            | ExprKind::TryAlloc { .. }
            | ExprKind::ForeignCall(_, _)
            | ExprKind::ForeignConst(_)
            | ExprKind::ForeignConvert(_, _)
            | ExprKind::NullPointer
            | ExprKind::IsNull(_)
            | ExprKind::Match { .. }
            | ExprKind::OwnerLet { .. }
            | ExprKind::OwnerBytesLen(_)
            | ExprKind::OwnerBytesGet(_, _)
            | ExprKind::OwnerBytesSet(_, _, _)
            | ExprKind::OwnerRead(_)
            | ExprKind::OwnerMove(_)
            | ExprKind::OwnerWrite(_, _) => None,
        },
    }
}

/// Resolve one exact typed edge without scanning siblings or allocating a path.
pub(super) fn child<'a>(node: TypedNodeRef<'a>, role: &TypedChildRole) -> Option<TypedNodeRef<'a>> {
    use TypedChildRole as Role;
    let index = match role {
        Role::FunctionRoot => return None,
        Role::Parameter(index)
        | Role::BuiltinOperand(index)
        | Role::ConstructorField(index)
        | Role::PatternField(index)
        | Role::CallArgument(index)
        | Role::RecordField(index) => *index,
        Role::Result | Role::FunctionBody => {
            let TypedNodeRef::Function { params, .. } = node else {
                return None;
            };
            params.len() + usize::from(*role == Role::FunctionBody)
        }
        Role::MatchArm(index) => index.checked_add(1)?,
        Role::LetValue
        | Role::BinaryLeft
        | Role::Condition
        | Role::AssignmentValue
        | Role::MatchScrutinee
        | Role::ArmPattern
        | Role::ArgumentValue
        | Role::RecordValue
        | Role::Capacity => 0,
        Role::LetBody | Role::BinaryRight | Role::Then | Role::LoopBody | Role::ArmBody => 1,
        Role::Else => 2,
    };
    let (actual, value) = child_at(node, index)?;
    (actual == *role).then_some(value)
}

pub(super) struct Occurrence<'a> {
    pub(super) instance: usize,
    /// Global ordinal in the result vector, not an instance-local ordinal.
    pub(super) parent: Option<usize>,
    pub(super) role: TypedChildRole,
    pub(super) node: TypedNodeRef<'a>,
}

fn outside(detail: &str) -> String {
    format!("E-TYPED-SOURCE-MAP: outside Copy function-body coverage: {detail}")
}

fn copy_type(ty: Type, typed: &TypedMain, budget: &mut BodyBudget) -> Result<(), String> {
    budget.work(1)?;
    match ty {
        Type::Int | Type::Bool | Type::Unit | Type::StaticStr | Type::U8x16 | Type::Bytes(_) => {
            Ok(())
        }
        Type::CU8 | Type::CU32 | Type::CU64 | Type::CF32 | Type::RawPointer(_) => {
            Err(outside("native type"))
        }
        Type::OwnedI32 | Type::OwnedBytes | Type::Adt { owned: true, .. } => {
            Err(outside("owner type"))
        }
        Type::Adt { id, owned: false } => {
            let definition = typed
                .adts
                .get(id)
                .ok_or("E-TYPED-SOURCE-MAP: invalid Copy nominal identity")?;
            if definition.owned {
                Err(outside("owner nominal definition"))
            } else {
                Ok(())
            }
        }
    }
}

fn copy_operation(kind: &ExprKind) -> Result<(), String> {
    match kind {
        ExprKind::NoStorageLet { .. }
        | ExprKind::DiscardLet { .. }
        | ExprKind::InterruptedCall(_) => Err(outside("no-normal control result")),
        ExprKind::Int(_)
        | ExprKind::Str(_)
        | ExprKind::Bool(_)
        | ExprKind::Unit
        | ExprKind::Read(_)
        | ExprKind::Record { .. }
        | ExprKind::FieldRead(_)
        | ExprKind::FieldAssign(_, _)
        | ExprKind::Call(_, _)
        | ExprKind::Constructor { .. }
        | ExprKind::CopyMatch { .. }
        | ExprKind::ByteLen(_)
        | ExprKind::ByteAt(_, _)
        | ExprKind::BytesNew(_)
        | ExprKind::BytesLen(_)
        | ExprKind::BytesGet(_, _)
        | ExprKind::BytesSet(_, _, _)
        | ExprKind::Let { .. }
        | ExprKind::Assign(_, _)
        | ExprKind::If(_, _, _)
        | ExprKind::While(_, _) => Ok(()),
        ExprKind::Vector(operation, _) => match operation {
            VectorOp::Load
            | VectorOp::Lane
            | VectorOp::And
            | VectorOp::Or
            | VectorOp::Xor
            | VectorOp::Eq
            | VectorOp::Any
            | VectorOp::All => Ok(()),
        },
        ExprKind::Binary(operation, _, _) => match operation {
            BinaryOp::Add
            | BinaryOp::Sub
            | BinaryOp::Mul
            | BinaryOp::Div
            | BinaryOp::Rem
            | BinaryOp::Eq
            | BinaryOp::Ne
            | BinaryOp::Lt
            | BinaryOp::Le
            | BinaryOp::Gt
            | BinaryOp::Ge
            | BinaryOp::And
            | BinaryOp::Or => Ok(()),
        },
        ExprKind::ForeignCall(_, _)
        | ExprKind::ForeignConst(_)
        | ExprKind::ForeignConvert(_, _)
        | ExprKind::NullPointer
        | ExprKind::IsNull(_) => Err(outside("native operation")),
        ExprKind::TryAlloc { .. }
        | ExprKind::Match { .. }
        | ExprKind::OwnerLet { .. }
        | ExprKind::OwnerBytesLen(_)
        | ExprKind::OwnerBytesGet(_, _)
        | ExprKind::OwnerBytesSet(_, _, _)
        | ExprKind::OwnerRead(_)
        | ExprKind::OwnerMove(_)
        | ExprKind::OwnerWrite(_, _) => Err(outside("owner operation or consuming match")),
    }
}

fn copy_argument(argument: &CallArg) -> Result<(), String> {
    match argument {
        CallArg::Value(_)
        | CallArg::ReadCopy(_)
        | CallArg::InoutCopy(_)
        | CallArg::ReadTemporary(_)
        | CallArg::ReadField(_)
        | CallArg::InoutField(_) => Ok(()),
        CallArg::Move(_) | CallArg::ReadOwner(_) | CallArg::InoutOwner(_) => {
            Err(outside("owner call argument"))
        }
    }
}

fn validate_node(
    node: TypedNodeRef<'_>,
    typed: &TypedMain,
    budget: &mut BodyBudget,
) -> Result<(), String> {
    match node {
        TypedNodeRef::Parameter((_, ty, mode)) => {
            match mode {
                ParamMode::Value | ParamMode::Read | ParamMode::Inout => {}
            }
            copy_type(*ty, typed, budget)
        }
        TypedNodeRef::Result(ty) => copy_type(*ty, typed, budget),
        TypedNodeRef::Expression(expression) => {
            copy_type(expression.ty, typed, budget)?;
            copy_operation(&expression.kind)
        }
        TypedNodeRef::Argument(argument) => copy_argument(argument),
        TypedNodeRef::Function { .. }
        | TypedNodeRef::RecordField { .. }
        | TypedNodeRef::Arm(_)
        | TypedNodeRef::Pattern(_)
        | TypedNodeRef::Capacity(_) => Ok(()),
    }
}

/// Enumerate final instances, then root/parameters/result/body semantic preorder.
/// Every queued occurrence is charged before allocation and moved, not cloned,
/// into the output. Child edges/work are charged before their target is read.
pub(super) fn enumerate<'a>(
    typed: &'a TypedMain,
    budget: &mut BodyBudget,
) -> Result<Vec<Occurrence<'a>>, String> {
    budget.work(1)?;
    if typed.native.has_foreign() || !typed.native.pointers.is_empty() {
        return Err(outside("native declarations or includes"));
    }
    // Inspect each stored type dependency once. Do not recursively expand a
    // nominal DAG, and do not manufacture public layout-origin occurrences.
    for definition in &typed.adts {
        budget.work(1)?;
        if definition.owned {
            return Err(outside("owner nominal definition"));
        }
        for argument in &definition.arguments {
            copy_type(*argument, typed, budget)?;
        }
        for variant in &definition.variants {
            budget.work(1)?;
            for field in &variant.fields {
                copy_type(*field, typed, budget)?;
            }
        }
    }
    let mut occurrences = Vec::new();
    let mut pending = Vec::new();
    for (instance, function) in typed.functions.iter().enumerate() {
        budget.work(1)?;
        if !function.param_cleanup.is_empty() {
            return Err(outside("parameter cleanup"));
        }
        budget.records(1)?;
        pending.push(Occurrence {
            instance,
            parent: None,
            role: TypedChildRole::FunctionRoot,
            node: TypedNodeRef::Function {
                params: &function.params,
                result: function.result.value_ref()?,
                body: function.body.value_expr()?,
            },
        });
        while let Some(occurrence) = pending.pop() {
            budget.work(1)?;
            let ordinal = occurrences.len();
            validate_node(occurrence.node, typed, budget).map_err(|error| {
                format!("{error} at function instance {instance}, typed node {ordinal}")
            })?;
            let node = occurrence.node;
            occurrences.push(occurrence);
            // Reverse scheduling yields forward semantic preorder on pop.
            for index in (0..child_count(node)).rev() {
                budget.edges(1)?;
                budget.work(1)?;
                budget.records(1)?;
                let (role, node) = child_at(node, index)
                    .ok_or("E-TYPED-SOURCE-MAP: missing independently enumerated child")?;
                pending.push(Occurrence {
                    instance,
                    parent: Some(ordinal),
                    role,
                    node,
                });
            }
        }
    }
    Ok(occurrences)
}

/// Exact, nonrecursive construction attributes. Children are separate receipts;
/// this is not a cloned IR, a hash, or a source-correspondence reconstruction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Fingerprint {
    Function { parameters: usize },
    Parameter(LocalId, Type, ParamMode),
    Result(Type),
    Expression(Type, ExpressionFingerprint),
    Argument(ArgumentFingerprint),
    RecordField { written: usize, declaration: usize },
    Arm,
    Pattern(PatternFingerprint),
    Capacity(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PlaceFingerprint {
    pub(super) root: LocalId,
    pub(super) path: Vec<FieldStep>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ExpressionFingerprint {
    Int(i32),
    Str(String),
    Bool(bool),
    Unit,
    Read(LocalId),
    Record {
        adt: usize,
        fields: usize,
    },
    FieldRead(PlaceFingerprint),
    FieldAssign(PlaceFingerprint),
    Call {
        function: usize,
        arguments: usize,
    },
    Constructor {
        adt: usize,
        variant: usize,
        fields: usize,
    },
    CopyMatch {
        arms: usize,
    },
    ByteLen,
    ByteAt,
    BytesNew(usize),
    Vector(VectorOp, usize),
    BytesLen,
    BytesGet,
    BytesSet,
    Let {
        local: LocalId,
        mutable: bool,
    },
    Assign(LocalId),
    Binary(BinaryOp),
    If,
    While,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ArgumentFingerprint {
    Value,
    ReadCopy(LocalId),
    InoutCopy(LocalId),
    ReadTemporary,
    ReadField(PlaceFingerprint),
    InoutField(PlaceFingerprint),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PatternFingerprint {
    Wildcard,
    Binding(LocalId),
    Int(i32),
    Bool(bool),
    Unit,
    Constructor {
        adt: usize,
        variant: usize,
        fields: usize,
    },
}

fn place_fingerprint(
    place: &FieldPlace,
    budget: &mut BodyBudget,
) -> Result<PlaceFingerprint, String> {
    budget.work(place.path.len())?;
    Ok(PlaceFingerprint {
        root: place.root,
        path: place.path.clone(),
    })
}

/// Charge one receipt before allocating it; string bytes and stored projection
/// steps also charge work before copying. Callers must budget retained child
/// edges separately, and must not use an uncharged `clone` to retain receipts.
pub(super) fn fingerprint(
    node: TypedNodeRef<'_>,
    budget: &mut BodyBudget,
) -> Result<Fingerprint, String> {
    budget.records(1)?;
    budget.work(1)?;
    Ok(match node {
        TypedNodeRef::Function { params, .. } => Fingerprint::Function {
            parameters: params.len(),
        },
        TypedNodeRef::Parameter((local, ty, mode)) => Fingerprint::Parameter(*local, *ty, *mode),
        TypedNodeRef::Result(ty) => Fingerprint::Result(*ty),
        TypedNodeRef::RecordField {
            written,
            declaration,
            ..
        } => Fingerprint::RecordField {
            written,
            declaration,
        },
        TypedNodeRef::Arm(_) => Fingerprint::Arm,
        TypedNodeRef::Capacity(capacity) => Fingerprint::Capacity(*capacity),
        TypedNodeRef::Pattern(pattern) => Fingerprint::Pattern(match pattern {
            CopyPattern::Wildcard => PatternFingerprint::Wildcard,
            CopyPattern::Binding(local) => PatternFingerprint::Binding(*local),
            CopyPattern::Int(value) => PatternFingerprint::Int(*value),
            CopyPattern::Bool(value) => PatternFingerprint::Bool(*value),
            CopyPattern::Unit => PatternFingerprint::Unit,
            CopyPattern::Constructor {
                adt,
                variant,
                fields,
            } => PatternFingerprint::Constructor {
                adt: *adt,
                variant: *variant,
                fields: fields.len(),
            },
        }),
        TypedNodeRef::Argument(argument) => Fingerprint::Argument(match argument {
            CallArg::Value(_) => ArgumentFingerprint::Value,
            CallArg::ReadCopy(local) => ArgumentFingerprint::ReadCopy(*local),
            CallArg::InoutCopy(local) => ArgumentFingerprint::InoutCopy(*local),
            CallArg::ReadTemporary(_) => ArgumentFingerprint::ReadTemporary,
            CallArg::ReadField(place) => {
                ArgumentFingerprint::ReadField(place_fingerprint(place, budget)?)
            }
            CallArg::InoutField(place) => {
                ArgumentFingerprint::InoutField(place_fingerprint(place, budget)?)
            }
            CallArg::Move(_) | CallArg::ReadOwner(_) | CallArg::InoutOwner(_) => {
                return Err(outside("owner call argument"))
            }
        }),
        TypedNodeRef::Expression(expression) => Fingerprint::Expression(
            expression.ty,
            match &expression.kind {
                ExprKind::NoStorageLet { .. }
                | ExprKind::DiscardLet { .. }
                | ExprKind::InterruptedCall(_) => return Err(outside("no-normal control result")),
                ExprKind::Int(value) => ExpressionFingerprint::Int(*value),
                ExprKind::Str(value) => {
                    budget.work(value.len())?;
                    ExpressionFingerprint::Str(value.clone())
                }
                ExprKind::Bool(value) => ExpressionFingerprint::Bool(*value),
                ExprKind::Unit => ExpressionFingerprint::Unit,
                ExprKind::Read(local) => ExpressionFingerprint::Read(*local),
                ExprKind::Record { adt, fields } => ExpressionFingerprint::Record {
                    adt: *adt,
                    fields: fields.len(),
                },
                ExprKind::FieldRead(place) => {
                    ExpressionFingerprint::FieldRead(place_fingerprint(place, budget)?)
                }
                ExprKind::FieldAssign(place, _) => {
                    ExpressionFingerprint::FieldAssign(place_fingerprint(place, budget)?)
                }
                ExprKind::Call(function, arguments) => ExpressionFingerprint::Call {
                    function: *function,
                    arguments: arguments.len(),
                },
                ExprKind::Constructor {
                    adt,
                    variant,
                    fields,
                } => ExpressionFingerprint::Constructor {
                    adt: *adt,
                    variant: *variant,
                    fields: fields.len(),
                },
                ExprKind::CopyMatch { arms, .. } => {
                    ExpressionFingerprint::CopyMatch { arms: arms.len() }
                }
                ExprKind::ByteLen(_) => ExpressionFingerprint::ByteLen,
                ExprKind::ByteAt(_, _) => ExpressionFingerprint::ByteAt,
                ExprKind::BytesNew(capacity) => ExpressionFingerprint::BytesNew(*capacity),
                ExprKind::Vector(operation, operands) => {
                    ExpressionFingerprint::Vector(*operation, operands.len())
                }
                ExprKind::BytesLen(_) => ExpressionFingerprint::BytesLen,
                ExprKind::BytesGet(_, _) => ExpressionFingerprint::BytesGet,
                ExprKind::BytesSet(_, _, _) => ExpressionFingerprint::BytesSet,
                ExprKind::Let { local, mutable, .. } => ExpressionFingerprint::Let {
                    local: *local,
                    mutable: *mutable,
                },
                ExprKind::Assign(local, _) => ExpressionFingerprint::Assign(*local),
                ExprKind::Binary(operation, _, _) => ExpressionFingerprint::Binary(*operation),
                ExprKind::If(_, _, _) => ExpressionFingerprint::If,
                ExprKind::While(_, _) => ExpressionFingerprint::While,
                ExprKind::ForeignCall(_, _)
                | ExprKind::ForeignConst(_)
                | ExprKind::ForeignConvert(_, _)
                | ExprKind::NullPointer
                | ExprKind::IsNull(_) => return Err(outside("native operation")),
                ExprKind::TryAlloc { .. }
                | ExprKind::Match { .. }
                | ExprKind::OwnerLet { .. }
                | ExprKind::OwnerBytesLen(_)
                | ExprKind::OwnerBytesGet(_, _)
                | ExprKind::OwnerBytesSet(_, _, _)
                | ExprKind::OwnerRead(_)
                | ExprKind::OwnerMove(_)
                | ExprKind::OwnerWrite(_, _) => {
                    return Err(outside("owner operation or consuming match"))
                }
            },
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typed::{
        AdtOrigin, AdtType, AdtVariant, AllocationResultLayout, Cleanup, CleanupTree,
        ForeignConstantId, ForeignFunctionId, OwnerInit, SystemAllocationKind, TypedFunction,
    };

    fn int(value: i32) -> ExprNode {
        ExprNode {
            ty: Type::Int,
            kind: ExprKind::Int(value),
        }
    }

    fn expression(kind: ExprKind) -> ExprNode {
        ExprNode {
            ty: Type::Int,
            kind,
        }
    }

    fn function(body: ExprNode) -> TypedFunction {
        TypedFunction {
            params: Vec::new(),
            result: (body.ty).into(),
            body: body.into(),
            param_cleanup: Vec::new(),
        }
    }

    fn program(body: ExprNode) -> TypedMain {
        TypedMain {
            logical: Default::default(),
            native: Default::default(),
            allocation_contract: None,
            adts: Vec::new(),
            functions: vec![function(body)],
            entry: 0,
        }
    }

    fn place() -> FieldPlace {
        FieldPlace {
            root: LocalId(0),
            path: vec![
                FieldStep {
                    record: 1,
                    field: 2,
                },
                FieldStep {
                    record: 3,
                    field: 4,
                },
            ],
        }
    }

    fn fp(node: TypedNodeRef<'_>) -> Fingerprint {
        fingerprint(node, &mut BodyBudget::default()).unwrap()
    }

    fn same_reference(left: TypedNodeRef<'_>, right: TypedNodeRef<'_>) -> bool {
        use TypedNodeRef as Node;
        match (left, right) {
            (
                Node::Function {
                    params: a,
                    result: b,
                    body: c,
                },
                Node::Function {
                    params: x,
                    result: y,
                    body: z,
                },
            ) => std::ptr::eq(a, x) && std::ptr::eq(b, y) && std::ptr::eq(c, z),
            (Node::Parameter(a), Node::Parameter(b)) => std::ptr::eq(a, b),
            (Node::Result(a), Node::Result(b)) => std::ptr::eq(a, b),
            (Node::Expression(a), Node::Expression(b)) => std::ptr::eq(a, b),
            (Node::Argument(a), Node::Argument(b)) => std::ptr::eq(a, b),
            (
                Node::RecordField {
                    written: a,
                    declaration: b,
                    value: c,
                },
                Node::RecordField {
                    written: x,
                    declaration: y,
                    value: z,
                },
            ) => a == x && b == y && std::ptr::eq(c, z),
            (Node::Arm(a), Node::Arm(b)) => std::ptr::eq(a, b),
            (Node::Pattern(a), Node::Pattern(b)) => std::ptr::eq(a, b),
            (Node::Capacity(a), Node::Capacity(b)) => std::ptr::eq(a, b),
            _ => false,
        }
    }

    fn assert_children(node: TypedNodeRef<'_>, expected: &[TypedChildRole]) {
        let children = node.children().collect::<Vec<_>>();
        assert_eq!(
            children.iter().map(|(role, _)| *role).collect::<Vec<_>>(),
            expected
        );
        assert_eq!(node.children().len(), expected.len());
        for (role, value) in &children {
            assert!(same_reference(child(node, role).unwrap(), *value));
        }
        let reversed = node.children().rev().collect::<Vec<_>>();
        for ((role, value), (other_role, other_value)) in children.iter().zip(reversed.iter().rev())
        {
            assert_eq!(role, other_role);
            assert!(same_reference(*value, *other_value));
        }
        let mut iterator = node.children();
        for index in 0..children.len() {
            let (role, value) = if index % 2 == 0 {
                iterator.next().unwrap()
            } else {
                iterator.next_back().unwrap()
            };
            let expected_index = if index % 2 == 0 {
                index / 2
            } else {
                children.len() - 1 - index / 2
            };
            assert_eq!(role, children[expected_index].0);
            assert!(same_reference(value, children[expected_index].1));
            assert_eq!(iterator.len(), children.len() - 1 - index);
        }
        assert!(iterator.next().is_none());
        assert!(iterator.next_back().is_none());
        assert!(iterator.next().is_none());
        assert!(child(node, &TypedChildRole::FunctionRoot).is_none());
        assert!(child(node, &TypedChildRole::MatchArm(usize::MAX)).is_none());
        assert!(child(node, &TypedChildRole::Parameter(usize::MAX)).is_none());
    }

    #[test]
    fn final_function_order_and_all_signature_slots_are_exact() {
        use TypedChildRole as Role;
        let body = expression(ExprKind::Binary(
            BinaryOp::Add,
            Box::new(int(1)),
            Box::new(int(1)),
        ));
        let mut typed = program(body);
        typed.functions[0].params = vec![
            (LocalId(0), Type::Int, ParamMode::Value),
            (LocalId(1), Type::Int, ParamMode::Read),
        ];
        typed.functions.push(function(ExprNode {
            ty: Type::Bytes(4),
            kind: ExprKind::BytesNew(4),
        }));
        typed.entry = 1; // Entry does not select enumeration order.
        let rows = enumerate(&typed, &mut BodyBudget::default()).unwrap();
        let actual = rows
            .iter()
            .map(|row| (row.instance, row.parent, row.role, row.node.kind()))
            .collect::<Vec<_>>();
        use TypedNodeKind as Kind;
        assert_eq!(
            actual,
            vec![
                (0, None, Role::FunctionRoot, Kind::Function),
                (0, Some(0), Role::Parameter(0), Kind::Parameter),
                (0, Some(0), Role::Parameter(1), Kind::Parameter),
                (0, Some(0), Role::Result, Kind::Result),
                (0, Some(0), Role::FunctionBody, Kind::Expression),
                (0, Some(4), Role::BinaryLeft, Kind::Expression),
                (0, Some(4), Role::BinaryRight, Kind::Expression),
                (1, None, Role::FunctionRoot, Kind::Function),
                (1, Some(7), Role::Result, Kind::Result),
                (1, Some(7), Role::FunctionBody, Kind::Expression),
                (1, Some(9), Role::Capacity, Kind::Capacity),
            ]
        );
        assert_eq!(fp(rows[5].node), fp(rows[6].node));
        assert!(!same_reference(rows[5].node, rows[6].node));
        for (ordinal, row) in rows.iter().enumerate() {
            if let Some(parent) = row.parent {
                assert!(parent < ordinal);
                assert_eq!(rows[parent].instance, row.instance);
                assert!(same_reference(
                    child(rows[parent].node, &row.role).unwrap(),
                    row.node
                ));
            }
        }
        assert_children(
            rows[0].node,
            &[
                Role::Parameter(0),
                Role::Parameter(1),
                Role::Result,
                Role::FunctionBody,
            ],
        );
        assert_children(rows[1].node, &[]);
        assert_children(rows[3].node, &[]);
        assert_children(rows[10].node, &[]);
    }

    #[test]
    fn every_copy_expression_schema_has_exact_direct_children() {
        use TypedChildRole as Role;
        // These are structural unit fixtures, not a replacement for semantic
        // IR verification. Deliberately inspect every operation independently.
        let cases = vec![
            (ExprKind::Int(1), vec![]),
            (ExprKind::Str("é".into()), vec![]),
            (ExprKind::Bool(true), vec![]),
            (ExprKind::Unit, vec![]),
            (ExprKind::Read(LocalId(0)), vec![]),
            (
                ExprKind::Record {
                    adt: 0,
                    fields: vec![(1, int(1)), (0, int(2))],
                },
                vec![Role::RecordField(0), Role::RecordField(1)],
            ),
            (ExprKind::FieldRead(place()), vec![]),
            (
                ExprKind::FieldAssign(place(), Box::new(int(1))),
                vec![Role::AssignmentValue],
            ),
            (
                ExprKind::Call(
                    0,
                    vec![CallArg::Value(int(1)), CallArg::ReadCopy(LocalId(0))],
                ),
                vec![Role::CallArgument(0), Role::CallArgument(1)],
            ),
            (
                ExprKind::Constructor {
                    adt: 0,
                    variant: 0,
                    fields: vec![int(1), int(2)],
                },
                vec![Role::ConstructorField(0), Role::ConstructorField(1)],
            ),
            (
                ExprKind::CopyMatch {
                    value: Box::new(int(1)),
                    arms: vec![CopyArm {
                        pattern: CopyPattern::Wildcard,
                        body: int(2),
                    }],
                },
                vec![Role::MatchScrutinee, Role::MatchArm(0)],
            ),
            (
                ExprKind::ByteLen(Box::new(int(1))),
                vec![Role::BuiltinOperand(0)],
            ),
            (
                ExprKind::ByteAt(Box::new(int(1)), Box::new(int(2))),
                vec![Role::BuiltinOperand(0), Role::BuiltinOperand(1)],
            ),
            (ExprKind::BytesNew(4), vec![Role::Capacity]),
            (
                ExprKind::Vector(VectorOp::And, vec![int(1), int(2)]),
                vec![Role::BuiltinOperand(0), Role::BuiltinOperand(1)],
            ),
            (
                ExprKind::BytesLen(Box::new(int(1))),
                vec![Role::BuiltinOperand(0)],
            ),
            (
                ExprKind::BytesGet(Box::new(int(1)), Box::new(int(2))),
                vec![Role::BuiltinOperand(0), Role::BuiltinOperand(1)],
            ),
            (
                ExprKind::BytesSet(Box::new(int(1)), Box::new(int(2)), Box::new(int(3))),
                vec![
                    Role::BuiltinOperand(0),
                    Role::BuiltinOperand(1),
                    Role::BuiltinOperand(2),
                ],
            ),
            (
                ExprKind::Let {
                    local: LocalId(0),
                    mutable: false,
                    value: Box::new(int(1)),
                    body: Box::new(int(2)),
                },
                vec![Role::LetValue, Role::LetBody],
            ),
            (
                ExprKind::Assign(LocalId(0), Box::new(int(1))),
                vec![Role::AssignmentValue],
            ),
            (
                ExprKind::Binary(BinaryOp::Add, Box::new(int(1)), Box::new(int(2))),
                vec![Role::BinaryLeft, Role::BinaryRight],
            ),
            (
                ExprKind::If(Box::new(int(1)), Box::new(int(2)), Box::new(int(3))),
                vec![Role::Condition, Role::Then, Role::Else],
            ),
            (
                ExprKind::While(Box::new(int(1)), Box::new(int(2))),
                vec![Role::Condition, Role::LoopBody],
            ),
        ];
        for (kind, roles) in cases {
            copy_operation(&kind).unwrap();
            let value = expression(kind);
            assert_children(TypedNodeRef::Expression(&value), &roles);
        }
        for operation in [
            VectorOp::Load,
            VectorOp::Lane,
            VectorOp::And,
            VectorOp::Or,
            VectorOp::Xor,
            VectorOp::Eq,
            VectorOp::Any,
            VectorOp::All,
        ] {
            copy_operation(&ExprKind::Vector(operation, vec![])).unwrap();
        }
        for operation in [
            BinaryOp::Add,
            BinaryOp::Sub,
            BinaryOp::Mul,
            BinaryOp::Div,
            BinaryOp::Rem,
            BinaryOp::Eq,
            BinaryOp::Ne,
            BinaryOp::Lt,
            BinaryOp::Le,
            BinaryOp::Gt,
            BinaryOp::Ge,
            BinaryOp::And,
            BinaryOp::Or,
        ] {
            copy_operation(&ExprKind::Binary(
                operation,
                Box::new(int(1)),
                Box::new(int(2)),
            ))
            .unwrap();
        }
    }

    #[test]
    fn record_written_slots_arms_and_nested_patterns_keep_preorder() {
        use TypedChildRole as Role;
        let record = expression(ExprKind::Record {
            adt: 0,
            fields: vec![(1, int(7)), (0, int(8))],
        });
        let body = expression(ExprKind::CopyMatch {
            value: Box::new(int(0)),
            arms: vec![
                CopyArm {
                    pattern: CopyPattern::Constructor {
                        adt: 0,
                        variant: 1,
                        fields: vec![
                            CopyPattern::Constructor {
                                adt: 1,
                                variant: 2,
                                fields: vec![CopyPattern::Binding(LocalId(4))],
                            },
                            CopyPattern::Wildcard,
                        ],
                    },
                    body: record,
                },
                CopyArm {
                    pattern: CopyPattern::Int(0),
                    body: int(9),
                },
            ],
        });
        let typed = program(body);
        let rows = enumerate(&typed, &mut BodyBudget::default()).unwrap();
        let actual = rows
            .iter()
            .map(|row| (row.parent, row.role))
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            vec![
                (None, Role::FunctionRoot),
                (Some(0), Role::Result),
                (Some(0), Role::FunctionBody),
                (Some(2), Role::MatchScrutinee),
                (Some(2), Role::MatchArm(0)),
                (Some(4), Role::ArmPattern),
                (Some(5), Role::PatternField(0)),
                (Some(6), Role::PatternField(0)),
                (Some(5), Role::PatternField(1)),
                (Some(4), Role::ArmBody),
                (Some(9), Role::RecordField(0)),
                (Some(10), Role::RecordValue),
                (Some(9), Role::RecordField(1)),
                (Some(12), Role::RecordValue),
                (Some(2), Role::MatchArm(1)),
                (Some(14), Role::ArmPattern),
                (Some(14), Role::ArmBody),
            ]
        );
        assert!(matches!(
            rows[10].node,
            TypedNodeRef::RecordField {
                written: 0,
                declaration: 1,
                ..
            }
        ));
        assert!(matches!(
            rows[12].node,
            TypedNodeRef::RecordField {
                written: 1,
                declaration: 0,
                ..
            }
        ));
        assert_children(rows[4].node, &[Role::ArmPattern, Role::ArmBody]);
        assert_children(
            rows[5].node,
            &[Role::PatternField(0), Role::PatternField(1)],
        );
        assert_children(rows[6].node, &[Role::PatternField(0)]);
        assert_children(rows[10].node, &[Role::RecordValue]);
        for pattern in [
            CopyPattern::Wildcard,
            CopyPattern::Binding(LocalId(0)),
            CopyPattern::Int(1),
            CopyPattern::Bool(true),
            CopyPattern::Unit,
        ] {
            assert_children(TypedNodeRef::Pattern(&pattern), &[]);
        }
    }

    #[test]
    fn call_slots_do_not_invent_expressions_for_root_or_field_loans() {
        use TypedChildRole as Role;
        let arguments = vec![
            CallArg::Value(int(1)),
            CallArg::ReadCopy(LocalId(2)),
            CallArg::InoutCopy(LocalId(3)),
            CallArg::ReadTemporary(int(4)),
            CallArg::ReadField(place()),
            CallArg::InoutField(place()),
        ];
        let typed = program(expression(ExprKind::Call(0, arguments)));
        let rows = enumerate(&typed, &mut BodyBudget::default()).unwrap();
        assert_eq!(
            rows.iter()
                .map(|row| (row.parent, row.role))
                .collect::<Vec<_>>(),
            vec![
                (None, Role::FunctionRoot),
                (Some(0), Role::Result),
                (Some(0), Role::FunctionBody),
                (Some(2), Role::CallArgument(0)),
                (Some(3), Role::ArgumentValue),
                (Some(2), Role::CallArgument(1)),
                (Some(2), Role::CallArgument(2)),
                (Some(2), Role::CallArgument(3)),
                (Some(7), Role::ArgumentValue),
                (Some(2), Role::CallArgument(4)),
                (Some(2), Role::CallArgument(5)),
            ]
        );
        for index in [5, 6, 9, 10] {
            assert_children(rows[index].node, &[]);
        }
        for index in [3, 7] {
            assert_children(rows[index].node, &[Role::ArgumentValue]);
        }
        assert_eq!(
            rows.iter()
                .filter(|row| row.node.kind() == TypedNodeKind::Expression)
                .count(),
            3
        );
    }

    #[test]
    fn ordinary_verified_lowering_exposes_every_occurrence_category() {
        let source = r#"
            type Pair = { left: i32; right: i32 }
            type Choice = None | Some(i32)
            fn use_fields(read a, inout b) { b <- b + a; b }
            fn main() {
                let mut pair = record Pair { right = 2; left = 1 };
                use_fields(pair.left, inout pair.right);
                match Some(bytes_len(bytes_new(4))) {
                    Some(n) => n + pair.right | None => 0
                }
            }
        "#;
        let parsed = crate::parser::parse(source).unwrap();
        let checked = crate::check::analyze(&parsed).unwrap();
        let typed = crate::typed::lower_main(&checked).unwrap();
        let rows = enumerate(&typed, &mut BodyBudget::default()).unwrap();
        for kind in [
            TypedNodeKind::Function,
            TypedNodeKind::Parameter,
            TypedNodeKind::Result,
            TypedNodeKind::Expression,
            TypedNodeKind::Argument,
            TypedNodeKind::RecordField,
            TypedNodeKind::Arm,
            TypedNodeKind::Pattern,
            TypedNodeKind::Capacity,
        ] {
            assert!(
                rows.iter().any(|row| row.node.kind() == kind),
                "missing {kind:?}"
            );
        }
        let fields = rows
            .iter()
            .filter_map(|row| match row.node {
                TypedNodeRef::RecordField {
                    written,
                    declaration,
                    ..
                } => Some((written, declaration)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(fields, vec![(0, 1), (1, 0)]);
        assert!(rows
            .iter()
            .any(|row| matches!(row.node, TypedNodeRef::Argument(CallArg::ReadField(_)))));
        assert!(rows
            .iter()
            .any(|row| matches!(row.node, TypedNodeRef::Argument(CallArg::InoutField(_)))));
        let capacities = rows
            .iter()
            .filter_map(|row| match row.node {
                TypedNodeRef::Capacity(value) => Some(*value),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(capacities, vec![4]);
        for row in &rows {
            if let Some(parent) = row.parent {
                assert!(same_reference(
                    child(rows[parent].node, &row.role).unwrap(),
                    row.node
                ));
            }
        }
    }

    #[test]
    fn traversal_uses_an_explicit_worklist_beyond_source_recursion_limits() {
        const DEPTH: usize = 4096;
        let mut body = int(0);
        for _ in 0..DEPTH {
            body = expression(ExprKind::Assign(LocalId(0), Box::new(body)));
        }
        let mut typed = program(body);
        let rows = enumerate(&typed, &mut BodyBudget::default()).unwrap();
        assert_eq!(rows.len(), DEPTH + 3);
        for (ordinal, row) in rows.iter().enumerate().skip(3) {
            assert_eq!(row.parent, Some(ordinal - 1));
            assert_eq!(row.role, TypedChildRole::AssignmentValue);
        }
        drop(rows);
        // Also dismantle this deliberately deep test fixture iteratively;
        // recursive Box destruction must not obscure the visitor's behavior.
        let crate::typed::Computation::Value(mut body) = typed.functions.pop().unwrap().body else {
            panic!("expected value body")
        };
        while let ExprKind::Assign(_, next) = body.kind {
            body = *next;
        }
    }

    #[test]
    fn exact_minimal_record_edge_and_work_caps_are_enforced() {
        let typed = program(int(1));
        let mut exact = BodyBudget::with_limits(3, 2, 9);
        assert_eq!(enumerate(&typed, &mut exact).unwrap().len(), 3);
        assert_eq!((exact.records, exact.edges, exact.work.get()), (0, 0, 0));
        // The first unit past each exact cap fails without wrapping its counter.
        assert!(exact.records(1).unwrap_err().contains("record budget"));
        assert!(exact.edges(1).unwrap_err().contains("edge budget"));
        assert!(exact.work(1).unwrap_err().contains("work budget"));
        assert_eq!((exact.records, exact.edges, exact.work.get()), (0, 0, 0));
        for (records, edges, work, expected) in [
            (2, 2, 9, "record budget"),
            (3, 1, 9, "edge budget"),
            (3, 2, 8, "work budget"),
        ] {
            let mut budget = BodyBudget::with_limits(records, edges, work);
            let error = enumerate(&typed, &mut budget)
                .err()
                .expect("one missing unit must fail");
            assert!(error.starts_with("E-TYPED-SOURCE-MAP-LIMIT:"), "{error}");
            assert!(error.contains(expected), "{error}");
        }
        let mut spare = BodyBudget::with_limits(4, 3, 10);
        enumerate(&typed, &mut spare).unwrap();
        assert_eq!((spare.records, spare.edges, spare.work.get()), (1, 1, 1));
        let mut none = BodyBudget::with_limits(0, 2, 9);
        assert!(enumerate(&typed, &mut none).is_err());
        assert_eq!(none.edges, 2); // No function child is followed before its root is reserved.
    }

    fn assert_boundary(typed: &TypedMain, expected: &str) {
        let error = enumerate(typed, &mut BodyBudget::default())
            .err()
            .expect("outside Copy coverage");
        assert!(error.starts_with("E-TYPED-SOURCE-MAP:"), "{error}");
        assert!(error.contains(expected), "{error}");
    }

    #[test]
    fn every_owner_and_native_operation_or_argument_is_rejected() {
        let cases = vec![
            ExprKind::TryAlloc {
                kind: SystemAllocationKind::BoxI32,
                input: Box::new(int(1)),
                layout: AllocationResultLayout {
                    result_adt: 0,
                    ok_variant: 0,
                    err_variant: 1,
                    error_adt: 1,
                    invalid_length_variant: 0,
                    size_overflow_variant: 1,
                    out_of_memory_variant: 2,
                },
            },
            ExprKind::Match {
                value: Box::new(int(1)),
                arms: vec![],
            },
            ExprKind::OwnerLet {
                local: LocalId(0),
                mutable: false,
                init: OwnerInit::Move(LocalId(1)),
                body: Box::new(int(1)),
                cleanup: None,
            },
            ExprKind::OwnerBytesLen(LocalId(0)),
            ExprKind::OwnerBytesGet(LocalId(0), Box::new(int(1))),
            ExprKind::OwnerBytesSet(LocalId(0), Box::new(int(1)), Box::new(int(2))),
            ExprKind::OwnerRead(LocalId(0)),
            ExprKind::OwnerMove(LocalId(0)),
            ExprKind::OwnerWrite(LocalId(0), Box::new(int(1))),
            ExprKind::ForeignCall(ForeignFunctionId(0), vec![]),
            ExprKind::ForeignConst(ForeignConstantId(0)),
            ExprKind::ForeignConvert(crate::typed::ForeignConvert::U32, Box::new(int(1))),
            ExprKind::NullPointer,
            ExprKind::IsNull(Box::new(int(1))),
        ];
        for kind in cases {
            assert!(copy_operation(&kind).is_err());
            let value = expression(kind);
            assert_children(TypedNodeRef::Expression(&value), &[]);
            assert!(
                fingerprint(TypedNodeRef::Expression(&value), &mut BodyBudget::default()).is_err()
            );
            assert_boundary(&program(value), "outside Copy function-body coverage");
        }
        for argument in [
            CallArg::Move(LocalId(0)),
            CallArg::ReadOwner(LocalId(0)),
            CallArg::InoutOwner(LocalId(0)),
        ] {
            assert!(fingerprint(
                TypedNodeRef::Argument(&argument),
                &mut BodyBudget::default()
            )
            .is_err());
            assert_boundary(
                &program(expression(ExprKind::Call(0, vec![argument]))),
                "owner call argument",
            );
        }
    }

    #[test]
    fn raw_types_in_all_signature_and_registry_positions_are_explicitly_excluded() {
        for ty in [
            Type::CU8,
            Type::CU32,
            Type::CU64,
            Type::CF32,
            Type::RawPointer(crate::typed::PointerTypeId(0)),
        ] {
            let mut typed = program(int(1));
            typed.functions[0].body.value_expr_mut().unwrap().ty = ty;
            assert_boundary(&typed, "native type");
            let mut typed = program(int(1));
            typed.functions[0].result = (ty).into();
            assert_boundary(&typed, "native type");
            for mode in [ParamMode::Value, ParamMode::Read, ParamMode::Inout] {
                let mut typed = program(int(1));
                typed.functions[0].params.push((LocalId(0), ty, mode));
                assert_boundary(&typed, "native type");
            }
            for phantom in [false, true] {
                let mut typed = program(int(1));
                typed.adts.push(AdtType {
                    symbolic: None,
                    origin: AdtOrigin::Source,
                    record_fields: None,
                    name: "Hidden".into(),
                    arguments: if phantom { vec![ty] } else { vec![] },
                    variants: vec![crate::typed::AdtVariant {
                        name: "Hidden".into(),
                        fields: if phantom { vec![] } else { vec![ty] },
                    }],
                    owned: false,
                });
                assert_boundary(&typed, "native type");
            }
        }
    }

    #[test]
    fn signatures_cleanup_and_hidden_nominal_dependencies_obey_copy_boundary() {
        for ty in [
            Type::OwnedI32,
            Type::OwnedBytes,
            Type::Adt { id: 0, owned: true },
        ] {
            let mut typed = program(int(1));
            typed.functions[0].body.value_expr_mut().unwrap().ty = ty;
            assert_boundary(&typed, "owner type");
            let mut typed = program(int(1));
            typed.functions[0].result = (ty).into();
            assert_boundary(&typed, "owner type");
            for mode in [ParamMode::Value, ParamMode::Read, ParamMode::Inout] {
                let mut typed = program(int(1));
                typed.functions[0].params.push((LocalId(0), ty, mode));
                assert_boundary(&typed, "owner type");
            }
        }
        let mut typed = program(int(1));
        typed.functions[0].param_cleanup.push(Cleanup {
            local: LocalId(0),
            tree: CleanupTree::SkipCopy,
        });
        assert_boundary(&typed, "parameter cleanup");
        let definition = AdtType {
            symbolic: None,
            origin: AdtOrigin::Source,
            record_fields: None,
            name: "D".into(),
            arguments: vec![],
            variants: vec![AdtVariant {
                name: "D".into(),
                fields: vec![],
            }],
            owned: false,
        };
        for (owned, arguments, fields) in [
            (true, vec![], vec![]),
            (false, vec![Type::OwnedI32], vec![]),
            (false, vec![], vec![Type::OwnedBytes]),
        ] {
            let mut typed = program(int(1));
            let mut declaration = definition.clone();
            declaration.owned = owned;
            declaration.arguments = arguments;
            declaration.variants[0].fields = fields;
            typed.adts.push(declaration);
            assert_boundary(&typed, "owner");
        }
        let mut typed = program(int(1));
        typed.native.config.includes.push("native.h".into());
        assert_boundary(&typed, "native declarations or includes");
        let mut typed = program(int(1));
        typed.native.functions.push(crate::ast::ExternFunction {
            name: "native".into(),
            params: vec![],
            result: crate::ast::ExternType::I32,
        });
        assert_boundary(&typed, "native declarations or includes");
        let mut typed = program(int(1));
        typed.native.constants.push(crate::ast::ExternConst {
            name: "NATIVE".into(),
            ty: crate::ast::ExternType::I32,
        });
        assert_boundary(&typed, "native declarations or includes");
        let mut typed = program(int(1));
        typed.functions[0].body.value_expr_mut().unwrap().ty = Type::Adt {
            id: usize::MAX,
            owned: false,
        };
        assert_boundary(&typed, "invalid Copy nominal identity");
    }

    fn different_expressions(left: ExprNode, right: ExprNode) {
        assert_ne!(
            fp(TypedNodeRef::Expression(&left)),
            fp(TypedNodeRef::Expression(&right))
        );
    }

    #[test]
    fn fingerprints_distinguish_exact_expression_attributes_without_copying_children() {
        for (left, right) in [
            (ExprKind::Int(1), ExprKind::Int(2)),
            (ExprKind::Str("é".into()), ExprKind::Str("e".into())),
            (ExprKind::Bool(false), ExprKind::Bool(true)),
            (ExprKind::Unit, ExprKind::Int(0)),
            (ExprKind::Read(LocalId(0)), ExprKind::Read(LocalId(1))),
            (
                ExprKind::Record {
                    adt: 0,
                    fields: vec![],
                },
                ExprKind::Record {
                    adt: 1,
                    fields: vec![],
                },
            ),
            (
                ExprKind::Record {
                    adt: 0,
                    fields: vec![],
                },
                ExprKind::Record {
                    adt: 0,
                    fields: vec![(0, int(1))],
                },
            ),
            (ExprKind::Call(0, vec![]), ExprKind::Call(1, vec![])),
            (
                ExprKind::Call(0, vec![]),
                ExprKind::Call(0, vec![CallArg::Value(int(1))]),
            ),
            (
                ExprKind::Constructor {
                    adt: 0,
                    variant: 0,
                    fields: vec![],
                },
                ExprKind::Constructor {
                    adt: 1,
                    variant: 0,
                    fields: vec![],
                },
            ),
            (
                ExprKind::Constructor {
                    adt: 0,
                    variant: 0,
                    fields: vec![],
                },
                ExprKind::Constructor {
                    adt: 0,
                    variant: 1,
                    fields: vec![],
                },
            ),
            (
                ExprKind::Constructor {
                    adt: 0,
                    variant: 0,
                    fields: vec![],
                },
                ExprKind::Constructor {
                    adt: 0,
                    variant: 0,
                    fields: vec![int(1)],
                },
            ),
            (
                ExprKind::CopyMatch {
                    value: Box::new(int(1)),
                    arms: vec![],
                },
                ExprKind::CopyMatch {
                    value: Box::new(int(1)),
                    arms: vec![CopyArm {
                        pattern: CopyPattern::Wildcard,
                        body: int(1),
                    }],
                },
            ),
            (ExprKind::BytesNew(1), ExprKind::BytesNew(2)),
            (
                ExprKind::Vector(VectorOp::And, vec![]),
                ExprKind::Vector(VectorOp::Or, vec![]),
            ),
            (
                ExprKind::Vector(VectorOp::And, vec![]),
                ExprKind::Vector(VectorOp::And, vec![int(1)]),
            ),
            (
                ExprKind::Assign(LocalId(0), Box::new(int(1))),
                ExprKind::Assign(LocalId(1), Box::new(int(1))),
            ),
            (
                ExprKind::Binary(BinaryOp::Add, Box::new(int(1)), Box::new(int(2))),
                ExprKind::Binary(BinaryOp::Sub, Box::new(int(1)), Box::new(int(2))),
            ),
        ] {
            different_expressions(expression(left), expression(right));
        }
        different_expressions(
            int(1),
            ExprNode {
                ty: Type::Bool,
                kind: ExprKind::Int(1),
            },
        );
        for (local, mutable) in [(LocalId(1), false), (LocalId(0), true)] {
            different_expressions(
                expression(ExprKind::Let {
                    local: LocalId(0),
                    mutable: false,
                    value: Box::new(int(1)),
                    body: Box::new(int(2)),
                }),
                expression(ExprKind::Let {
                    local,
                    mutable,
                    value: Box::new(int(1)),
                    body: Box::new(int(2)),
                }),
            );
        }
        for changed in [
            FieldPlace {
                root: LocalId(1),
                ..place()
            },
            FieldPlace {
                path: vec![
                    FieldStep {
                        record: 9,
                        field: 2,
                    },
                    FieldStep {
                        record: 3,
                        field: 4,
                    },
                ],
                ..place()
            },
            FieldPlace {
                path: vec![
                    FieldStep {
                        record: 1,
                        field: 9,
                    },
                    FieldStep {
                        record: 3,
                        field: 4,
                    },
                ],
                ..place()
            },
            FieldPlace {
                path: vec![
                    FieldStep {
                        record: 3,
                        field: 4,
                    },
                    FieldStep {
                        record: 1,
                        field: 2,
                    },
                ],
                ..place()
            },
        ] {
            different_expressions(
                expression(ExprKind::FieldRead(place())),
                expression(ExprKind::FieldRead(changed.clone())),
            );
            different_expressions(
                expression(ExprKind::FieldAssign(place(), Box::new(int(1)))),
                expression(ExprKind::FieldAssign(changed, Box::new(int(1)))),
            );
        }
        let left = expression(ExprKind::Binary(
            BinaryOp::Add,
            Box::new(int(1)),
            Box::new(int(2)),
        ));
        let right = expression(ExprKind::Binary(
            BinaryOp::Add,
            Box::new(int(2)),
            Box::new(int(1)),
        ));
        // Heads intentionally exclude child payloads. Ordered child receipts
        // must detect this swap; recursively cloning the IR is not the scheme.
        assert_eq!(
            fp(TypedNodeRef::Expression(&left)),
            fp(TypedNodeRef::Expression(&right))
        );
    }

    #[test]
    fn fingerprints_distinguish_signature_operand_record_and_pattern_attributes() {
        let body = int(1);
        let base = (LocalId(0), Type::Int, ParamMode::Value);
        for changed in [
            (LocalId(1), Type::Int, ParamMode::Value),
            (LocalId(0), Type::Bool, ParamMode::Value),
            (LocalId(0), Type::Int, ParamMode::Read),
            (LocalId(0), Type::Int, ParamMode::Inout),
        ] {
            assert_ne!(
                fp(TypedNodeRef::Parameter(&base)),
                fp(TypedNodeRef::Parameter(&changed))
            );
        }
        assert_ne!(
            fp(TypedNodeRef::Result(&Type::Int)),
            fp(TypedNodeRef::Result(&Type::Bool))
        );
        assert_ne!(
            fp(TypedNodeRef::Function {
                params: &[],
                result: &Type::Int,
                body: &body
            }),
            fp(TypedNodeRef::Function {
                params: &[base],
                result: &Type::Int,
                body: &body
            })
        );
        assert_ne!(
            fp(TypedNodeRef::Capacity(&4)),
            fp(TypedNodeRef::Capacity(&5))
        );
        for (written, declaration) in [(1, 0), (0, 1)] {
            assert_ne!(
                fp(TypedNodeRef::RecordField {
                    written: 0,
                    declaration: 0,
                    value: &body
                }),
                fp(TypedNodeRef::RecordField {
                    written,
                    declaration,
                    value: &body
                })
            );
        }
        let arguments = [
            CallArg::Value(int(1)),
            CallArg::ReadTemporary(int(1)),
            CallArg::ReadCopy(LocalId(0)),
            CallArg::InoutCopy(LocalId(0)),
            CallArg::ReadField(place()),
            CallArg::InoutField(place()),
        ];
        for (index, left) in arguments.iter().enumerate() {
            for right in &arguments[index + 1..] {
                assert_ne!(
                    fp(TypedNodeRef::Argument(left)),
                    fp(TypedNodeRef::Argument(right))
                );
            }
        }
        for (left, right) in [
            (CallArg::ReadCopy(LocalId(0)), CallArg::ReadCopy(LocalId(1))),
            (
                CallArg::InoutCopy(LocalId(0)),
                CallArg::InoutCopy(LocalId(1)),
            ),
            (
                CallArg::ReadField(place()),
                CallArg::ReadField(FieldPlace {
                    root: LocalId(1),
                    ..place()
                }),
            ),
            (
                CallArg::InoutField(place()),
                CallArg::InoutField(FieldPlace {
                    path: vec![],
                    ..place()
                }),
            ),
        ] {
            assert_ne!(
                fp(TypedNodeRef::Argument(&left)),
                fp(TypedNodeRef::Argument(&right))
            );
        }
        let patterns = [
            CopyPattern::Wildcard,
            CopyPattern::Binding(LocalId(0)),
            CopyPattern::Int(1),
            CopyPattern::Bool(true),
            CopyPattern::Unit,
            CopyPattern::Constructor {
                adt: 0,
                variant: 0,
                fields: vec![],
            },
        ];
        for (index, left) in patterns.iter().enumerate() {
            for right in &patterns[index + 1..] {
                assert_ne!(
                    fp(TypedNodeRef::Pattern(left)),
                    fp(TypedNodeRef::Pattern(right))
                );
            }
        }
        for (left, right) in [
            (
                CopyPattern::Binding(LocalId(0)),
                CopyPattern::Binding(LocalId(1)),
            ),
            (CopyPattern::Int(1), CopyPattern::Int(2)),
            (CopyPattern::Bool(false), CopyPattern::Bool(true)),
            (
                CopyPattern::Constructor {
                    adt: 0,
                    variant: 0,
                    fields: vec![],
                },
                CopyPattern::Constructor {
                    adt: 1,
                    variant: 0,
                    fields: vec![],
                },
            ),
            (
                CopyPattern::Constructor {
                    adt: 0,
                    variant: 0,
                    fields: vec![],
                },
                CopyPattern::Constructor {
                    adt: 0,
                    variant: 1,
                    fields: vec![],
                },
            ),
            (
                CopyPattern::Constructor {
                    adt: 0,
                    variant: 0,
                    fields: vec![],
                },
                CopyPattern::Constructor {
                    adt: 0,
                    variant: 0,
                    fields: vec![CopyPattern::Wildcard],
                },
            ),
        ] {
            assert_ne!(
                fp(TypedNodeRef::Pattern(&left)),
                fp(TypedNodeRef::Pattern(&right))
            );
        }
    }

    #[test]
    fn fingerprints_charge_before_owned_string_and_projection_copies() {
        let text = ExprNode {
            ty: Type::StaticStr,
            kind: ExprKind::Str("é".into()),
        };
        let projection = expression(ExprKind::FieldRead(place()));
        for node in [
            TypedNodeRef::Expression(&text),
            TypedNodeRef::Expression(&projection),
        ] {
            let mut exact = BodyBudget::with_limits(1, 0, 3);
            fingerprint(node, &mut exact).unwrap();
            assert_eq!((exact.records, exact.edges, exact.work.get()), (0, 0, 0));
            let mut short = BodyBudget::with_limits(1, 0, 2);
            assert!(fingerprint(node, &mut short)
                .unwrap_err()
                .contains("work budget"));
            assert_eq!(short.work.get(), 1); // Failed bulk reservation does not partially consume work.
            let mut no_record = BodyBudget::with_limits(0, 0, 3);
            assert!(fingerprint(node, &mut no_record)
                .unwrap_err()
                .contains("record budget"));
            assert_eq!(no_record.work.get(), 3);
        }
    }
}
