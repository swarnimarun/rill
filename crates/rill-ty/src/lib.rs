//! `rill-ty` — type and effect checking (PLAN.md §3, §7.1, M1).
//!
//! Hindley–Milner style inference over the HIR:
//!
//! * one unification variable per unknown, with an occurs check;
//! * function signatures come from the declared parameter/return types;
//! * `.`-selected names are resolved with types: a field of a record, or a
//!   method/projection item;
//! * effect rows are collected from the body and checked against the declared
//!   row (`<computes>` functions may not call `<io>` ones), using the
//!   transacts/alloc/suspends vocabulary of PLAN §5.
//!
//! The result is a [`Checked`] table set: a type per expression
//! ([`Checked::expr_ty`]), per local ([`Checked::local_ty`]) and a signature or
//! nominal definition per item ([`Checked::item_ty`]) — exactly what code
//! generation needs to emit typed code.

use std::collections::HashMap;

use rill_ast::{BinaryOp, EffectRow, Type as AstType, TypeBody, UnaryOp};
use rill_diagnostics::{Diagnostic, Diagnostics};
use rill_hir::{
    Block, Cond, ElseBranch, Expr, ExprId, ExprKind, Hir, ItemId, ItemKind, LocalId, Namespace,
    Pattern, Res, Stmt,
};
use rill_span::{Span, Symbol};

pub use rill_diagnostics::Severity;

/// A built-in effect.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Effect {
    Io,
    Fs,
    Net,
    Time,
    Random,
    Alloc,
    Decides,
    Reads,
    Writes,
    Suspends,
    Custom(Symbol),
}

impl Effect {
    fn from_name(name: &str, sym: Symbol) -> Effect {
        match name {
            "io" => Effect::Io,
            "fs" => Effect::Fs,
            "net" => Effect::Net,
            "time" => Effect::Time,
            "random" => Effect::Random,
            "alloc" => Effect::Alloc,
            "decides" => Effect::Decides,
            "reads" => Effect::Reads,
            "writes" => Effect::Writes,
            "suspends" => Effect::Suspends,
            "transacts" => Effect::Writes,
            _ => Effect::Custom(sym),
        }
    }

    pub fn name(self) -> String {
        match self {
            Effect::Io => "io".into(),
            Effect::Fs => "fs".into(),
            Effect::Net => "net".into(),
            Effect::Time => "time".into(),
            Effect::Random => "random".into(),
            Effect::Alloc => "alloc".into(),
            Effect::Decides => "decides".into(),
            Effect::Reads => "reads".into(),
            Effect::Writes => "writes".into(),
            Effect::Suspends => "suspends".into(),
            Effect::Custom(sym) => format!("effect({})", sym.as_u32()),
        }
    }
}

/// A type. `Var` is a unification variable, resolved by [`Checked::resolve`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Ty {
    Var(u32),
    Int {
        bits: u8,
        signed: bool,
    },
    Float {
        bits: u8,
    },
    Bool,
    Unit,
    Text,
    Tuple(Vec<Ty>),
    Array {
        elem: Box<Ty>,
        len: Option<u64>,
    },
    Range(Box<Ty>),
    Record(NominalId),
    /// A nominal type by name (enum payloads, opaque `view`/`dyn` types).
    Nominal(NominalId),
    Fn {
        params: Vec<Ty>,
        ret: Box<Ty>,
        effects: Vec<Effect>,
    },
    /// The type of a value produced by a lenient (`|`) expression; unifying two
    /// choices requires their types to agree.
    Choice(Box<Ty>),
    Error,
}

impl Ty {
    pub fn i32() -> Ty {
        Ty::Int {
            bits: 32,
            signed: true,
        }
    }

    pub fn usize_() -> Ty {
        Ty::Int {
            bits: 64,
            signed: false,
        }
    }

    pub fn is_error(&self) -> bool {
        matches!(self, Ty::Error)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct NominalId(pub u32);

#[derive(Clone, Debug)]
pub struct FieldTy {
    pub name: Symbol,
    pub mutable: bool,
    pub ty: Ty,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct VariantTy {
    pub name: Symbol,
    pub payload: Vec<Ty>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum NominalKind {
    Record(Vec<FieldTy>),
    Enum(Vec<VariantTy>),
    /// A `view` type: fields, but no heap storage (§4.3).
    View(Vec<FieldTy>),
    Alias(Ty),
    /// `dyn (…)`, raw pointers, shaped tensors: checked opaquely for now.
    Opaque,
}

#[derive(Clone, Debug)]
pub struct NominalDef {
    pub id: NominalId,
    pub name: Symbol,
    pub kind: NominalKind,
    pub span: Span,
}

/// The checked signature of an item.
#[derive(Clone, Debug)]
pub enum ItemTy {
    Fn {
        params: Vec<Ty>,
        ret: Ty,
        effects: Vec<Effect>,
    },
    Nominal(NominalId),
    Incomplete,
}

/// Everything later phases need: types by expr/local/item id plus the nominal
/// definitions they refer to.
#[derive(Default, Clone, Debug)]
pub struct Checked {
    local_types: Vec<Option<Ty>>,
    item_types: Vec<ItemTy>,
    nominals: Vec<NominalDef>,
    /// Resolved types after unification, by expression id.
    resolved: HashMap<ExprId, Ty>,
    pub diagnostics: Diagnostics,
}

impl Checked {
    pub fn expr_ty(&self, id: ExprId) -> Option<&Ty> {
        self.resolved.get(&id)
    }

    pub fn local_ty(&self, id: LocalId) -> Option<&Ty> {
        self.local_types.get(id.0 as usize).and_then(|t| t.as_ref())
    }

    pub fn item_ty(&self, id: ItemId) -> Option<&ItemTy> {
        self.item_types.get(id.0 as usize)
    }

    pub fn nominal(&self, id: NominalId) -> &NominalDef {
        &self.nominals[id.0 as usize]
    }

    pub fn nominals(&self) -> &[NominalDef] {
        &self.nominals
    }
}

/// Type-checks a resolved module.
pub fn check(frontend: &rill_ast::Frontend, hir: &Hir) -> Checked {
    let mut checker = Checker::new(frontend, hir);
    checker.module();
    checker.finish()
}

struct Checker<'a> {
    frontend: &'a rill_ast::Frontend,
    hir: &'a Hir,
    subst: HashMap<u32, Ty>,
    next_var: u32,
    nominals: Vec<NominalDef>,
    nominal_by_item: HashMap<ItemId, NominalId>,
    item_types: Vec<ItemTy>,
    local_types: Vec<Option<Ty>>,
    expr_types: HashMap<ExprId, Ty>,
    diagnostics: Diagnostics,
    /// Effects performed by the function currently being checked.
    body_effects: Vec<Effect>,
    /// Effects per function body, filled in as bodies are checked.
    effects_of: HashMap<ExprId, Vec<Effect>>,
    /// Set while checking the body of a lenient handler (`option`/`one`/`all`),
    /// where `x := e` iterates over `e` instead of binding it whole.
    handler: Option<rill_ast::HandlerKind>,
    /// The nominal standing in for `Option` results.
    option_id: NominalId,
}

impl<'a> Checker<'a> {
    fn new(frontend: &'a rill_ast::Frontend, hir: &'a Hir) -> Self {
        Checker {
            frontend,
            hir,
            subst: HashMap::new(),
            next_var: 0,
            nominals: Vec::new(),
            nominal_by_item: HashMap::new(),
            item_types: vec![ItemTy::Incomplete; hir.items().len()],
            local_types: vec![None; hir.locals().len()],
            expr_types: HashMap::new(),
            diagnostics: Diagnostics::new(),
            body_effects: Vec::new(),
            effects_of: HashMap::new(),
            handler: None,
            option_id: NominalId(0),
        }
    }

    fn finish(mut self) -> Checked {
        let mut resolved = HashMap::new();
        for (id, ty) in self.expr_types.iter() {
            resolved.insert(*id, self.zonk(ty));
        }
        for index in 0..self.local_types.len() {
            if let Some(ty) = self.local_types[index].clone() {
                self.local_types[index] = Some(self.zonk(&ty));
            }
        }
        Checked {
            local_types: self.local_types,
            item_types: self.item_types,
            nominals: self.nominals,
            resolved,
            diagnostics: self.diagnostics,
        }
    }

    fn text(&self, sym: Symbol) -> &str {
        self.frontend.interner.get(sym)
    }

    fn err(&mut self, span: Span, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic::error(span, message));
    }

    fn fresh(&mut self) -> Ty {
        let id = self.next_var;
        self.next_var += 1;
        Ty::Var(id)
    }

    // -- unification -------------------------------------------------------

    fn resolve_deep(&self, ty: &Ty) -> Ty {
        match ty {
            Ty::Var(id) => match self.subst.get(id) {
                Some(bound) => self.resolve_deep(bound),
                None => ty.clone(),
            },
            Ty::Tuple(elems) => Ty::Tuple(elems.iter().map(|e| self.resolve_deep(e)).collect()),
            Ty::Array { elem, len } => Ty::Array {
                elem: Box::new(self.resolve_deep(elem)),
                len: *len,
            },
            Ty::Range(elem) => Ty::Range(Box::new(self.resolve_deep(elem))),
            Ty::Fn {
                params,
                ret,
                effects,
            } => Ty::Fn {
                params: params.iter().map(|p| self.resolve_deep(p)).collect(),
                ret: Box::new(self.resolve_deep(ret)),
                effects: effects.clone(),
            },
            Ty::Choice(inner) => Ty::Choice(Box::new(self.resolve_deep(inner))),
            other => other.clone(),
        }
    }

    /// Fully resolves a type (used when a table entry is stored).
    fn zonk(&self, ty: &Ty) -> Ty {
        let resolved = self.resolve_deep(ty);
        match resolved {
            // An unbound variable that reached the end of checking stands for
            // "unknown"; it is not an error by itself.
            Ty::Var(_) => Ty::Error,
            Ty::Tuple(elems) => Ty::Tuple(elems.iter().map(|e| self.zonk(e)).collect()),
            Ty::Array { elem, len } => Ty::Array {
                elem: Box::new(self.zonk(&elem)),
                len,
            },
            Ty::Range(elem) => Ty::Range(Box::new(self.zonk(&elem))),
            Ty::Fn {
                params,
                ret,
                effects,
            } => Ty::Fn {
                params: params.iter().map(|p| self.zonk(p)).collect(),
                ret: Box::new(self.zonk(&ret)),
                effects,
            },
            Ty::Choice(inner) => self.zonk(&inner),
            other => other,
        }
    }

    fn occurs(&self, id: u32, ty: &Ty) -> bool {
        match self.resolve_deep(ty) {
            Ty::Var(other) => other == id,
            Ty::Tuple(elems) => elems.iter().any(|e| self.occurs(id, e)),
            Ty::Array { elem, .. } => self.occurs(id, &elem),
            Ty::Range(elem) => self.occurs(id, &elem),
            Ty::Fn { params, ret, .. } => {
                params.iter().any(|p| self.occurs(id, p)) || self.occurs(id, &ret)
            }
            Ty::Choice(inner) => self.occurs(id, &inner),
            _ => false,
        }
    }

    /// Unifies two types, reporting a diagnostic on failure. `context`
    /// explains what was being checked.
    fn unify(&mut self, expected: &Ty, found: &Ty, span: Span, context: &str) -> bool {
        let expected = self.resolve_deep(expected);
        let found = self.resolve_deep(found);
        match (&expected, &found) {
            (Ty::Error, _) | (_, Ty::Error) => true,
            // A choice unifies with either of its alternatives.
            (Ty::Choice(inner), other) | (other, Ty::Choice(inner)) => {
                self.unify(inner, other, span, context)
            }
            (Ty::Var(id), other) => self.bind(*id, other, span, context),
            (other, Ty::Var(id)) => self.bind(*id, other, span, context),
            (
                Ty::Int {
                    bits: a,
                    signed: sa,
                },
                Ty::Int {
                    bits: b,
                    signed: sb,
                },
            ) => {
                if a == b && sa == sb {
                    true
                } else if matches!(found, Ty::Int { .. }) && matches!(expected, Ty::Int { .. }) {
                    self.int_mismatch(&expected, &found, span, context)
                } else {
                    false
                }
            }
            (Ty::Float { bits: a }, Ty::Float { bits: b }) if a == b => true,
            (Ty::Bool, Ty::Bool)
            | (Ty::Unit, Ty::Unit)
            | (Ty::Text, Ty::Text)
            | (Ty::Record(_), Ty::Record(_)) => {
                expected == found || self.nominal_compat(&expected, &found, span, context)
            }
            (Ty::Nominal(a), Ty::Nominal(b))
            | (Ty::Record(a), Ty::Nominal(b))
            | (Ty::Nominal(a), Ty::Record(b)) => {
                let _ = (a, b);
                self.nominal_compat(&expected, &found, span, context)
            }
            (Ty::Tuple(a), Ty::Tuple(b)) if a.len() == b.len() => {
                let pairs: Vec<(Ty, Ty)> = a.iter().cloned().zip(b.iter().cloned()).collect();
                for (i, (x, y)) in pairs.into_iter().enumerate() {
                    self.unify(&x, &y, span, &format!("{context} (tuple element {i})"));
                }
                true
            }
            (Ty::Array { elem: a, len: la }, Ty::Array { elem: b, len: lb }) => {
                if la.is_some() && lb.is_some() && la != lb {
                    self.err(
                        span,
                        format!(
                            "array length mismatch: expected `{}`, found `{}`",
                            la.map(|n| n.to_string()).unwrap_or_else(|| "_".into()),
                            lb.map(|n| n.to_string()).unwrap_or_else(|| "_".into())
                        ),
                    );
                    return false;
                }
                self.unify(a, b, span, context)
            }
            (Ty::Range(a), Ty::Range(b)) => self.unify(a, b, span, context),
            (
                Ty::Fn {
                    params: ap,
                    ret: ar,
                    effects: ae,
                },
                Ty::Fn {
                    params: bp,
                    ret: br,
                    effects: be,
                },
            ) => {
                if ap.len() != bp.len() {
                    self.err(
                        span,
                        format!(
                            "argument count mismatch: expected {} parameter(s), found {}",
                            ap.len(),
                            bp.len()
                        ),
                    );
                    return false;
                }
                let pairs: Vec<(Ty, Ty)> = ap.iter().cloned().zip(bp.iter().cloned()).collect();
                for (x, y) in pairs {
                    self.unify(&x, &y, span, context);
                }
                // Effect rows must be *covered* by the expected row.
                let missing: Vec<String> = be
                    .iter()
                    .filter(|e| !ae.contains(e))
                    .map(|e| e.name())
                    .collect();
                if !missing.is_empty() {
                    self.err(
                        span,
                        format!(
                            "effect mismatch: `{}` is not allowed here (missing {})",
                            describe_effects(&missing),
                            describe_effects(&missing)
                        ),
                    );
                }
                self.unify(ar, br, span, context)
            }
            _ => {
                self.err(
                    span,
                    format!(
                        "type mismatch: expected `{}`, found `{}`",
                        self.display(&expected),
                        self.display(&found)
                    ),
                );
                self.diagnostics.push(
                    Diagnostic::error(span, format!("found `{}` here", self.display(&found)))
                        .note(format!("while {context}")),
                );
                false
            }
        }
    }

    fn int_mismatch(&mut self, expected: &Ty, found: &Ty, span: Span, context: &str) -> bool {
        self.err(
            span,
            format!(
                "integer width/signedness mismatch: expected `{}`, found `{}`",
                self.display(expected),
                self.display(found)
            ),
        );
        self.diagnostics.push(
            Diagnostic::error(span, format!("found `{}` here", self.display(found)))
                .note(format!("while {context}"))
                .note("insert a cast (`as`) to convert between integer types".to_string()),
        );
        false
    }

    fn nominal_compat(&mut self, expected: &Ty, found: &Ty, span: Span, context: &str) -> bool {
        if expected == found {
            return true;
        }
        self.err(
            span,
            format!(
                "type mismatch: expected `{}`, found `{}`",
                self.display(expected),
                self.display(found)
            ),
        );
        self.diagnostics.push(
            Diagnostic::error(span, format!("found `{}` here", self.display(found)))
                .note(format!("while {context}")),
        );
        false
    }

    fn bind(&mut self, id: u32, ty: &Ty, span: Span, context: &str) -> bool {
        if let Ty::Var(other) = ty {
            if *other == id {
                return true;
            }
        }
        if self.occurs(id, ty) {
            self.err(span, "cannot construct an infinite type");
            return false;
        }
        let _ = context;
        self.subst.insert(id, ty.clone());
        true
    }

    fn display(&self, ty: &Ty) -> String {
        match self.resolve_deep(ty) {
            Ty::Var(id) => format!("?{id}"),
            Ty::Int { bits, signed } => {
                format!("{}{bits}", if signed { 'i' } else { 'u' })
            }
            Ty::Float { bits } => format!("f{bits}"),
            Ty::Bool => "Bool".into(),
            Ty::Unit => "()".into(),
            Ty::Text => "String".into(),
            Ty::Tuple(elems) => {
                let parts: Vec<String> = elems.iter().map(|e| self.display(e)).collect();
                format!("({})", parts.join(", "))
            }
            Ty::Array { elem, len } => match len {
                Some(n) => format!("[{}; {n}]", self.display(&elem)),
                None => format!("[{}]", self.display(&elem)),
            },
            Ty::Range(elem) => format!("{}..{}", self.display(&elem), self.display(&elem)),
            Ty::Record(id) | Ty::Nominal(id) => self.nominal_name(id),
            Ty::Fn { params, ret, .. } => {
                let parts: Vec<String> = params.iter().map(|p| self.display(p)).collect();
                format!("({}) -> {}", parts.join(", "), self.display(&ret))
            }
            Ty::Choice(inner) => self.display(&inner),
            Ty::Error => "<error>".into(),
        }
    }

    fn nominal_name(&self, id: NominalId) -> String {
        match self.nominals.get(id.0 as usize) {
            Some(def) => self.text(def.name).to_string(),
            None => format!("<nominal {}>", id.0),
        }
    }

    // -- declarations ------------------------------------------------------

    fn module(&mut self) {
        // `option { … }` results are modelled with a built-in nominal; its
        // payload typing arrives with the handler lowering in M2/M3.
        debug_assert_eq!(self.option_id, NominalId(0));
        self.nominals.push(NominalDef {
            id: NominalId(0),
            name: Symbol::EMPTY,
            kind: NominalKind::Opaque,
            span: Span::DUMMY,
        });
        // Nominal types and signatures first, so order does not matter.
        for item in self.hir.items() {
            if let ItemKind::Type(def) = &item.kind {
                self.declare_nominal(item, def);
            }
        }
        for item in self.hir.items() {
            if let ItemKind::Type(def) = &item.kind {
                self.finish_nominal(item, def);
            }
        }
        for item in self.hir.items() {
            match &item.kind {
                ItemKind::Fn(def) => {
                    let params: Vec<Ty> = def.params.iter().map(|p| self.param_ty(p)).collect();
                    let ret = def
                        .ret
                        .as_ref()
                        .map(|slot| self.ast_ty(&slot.ty))
                        .unwrap_or(Ty::Unit);
                    let effects = self.row_effects(&def.effects);
                    self.item_types[item.id.0 as usize] = ItemTy::Fn {
                        params,
                        ret,
                        effects,
                    };
                }
                ItemKind::Project(def) => {
                    let params: Vec<Ty> = def.params.iter().map(|p| self.param_ty(p)).collect();
                    let ret = def
                        .ret
                        .as_ref()
                        .map(|slot| self.ast_ty(&slot.ty))
                        .unwrap_or(Ty::Unit);
                    let effects = self.row_effects(&def.effects);
                    self.item_types[item.id.0 as usize] = ItemTy::Fn {
                        params,
                        ret,
                        effects,
                    };
                }
                _ => {}
            }
        }
        // Bodies.
        for item in self.hir.items() {
            self.item_body(item);
        }
        // Declared effect rows must cover what the bodies do.
        for item in self.hir.items() {
            self.check_declared_effects(item);
        }
    }

    fn declare_nominal(&mut self, item: &rill_hir::Item, def: &rill_hir::TypeDef) {
        let id = NominalId(self.nominals.len() as u32);
        self.nominals.push(NominalDef {
            id,
            name: item.name.unwrap_or(Symbol::EMPTY),
            kind: NominalKind::Opaque,
            span: item.span,
        });
        self.nominal_by_item.insert(item.id, id);
        self.item_types[item.id.0 as usize] = ItemTy::Nominal(id);
        let _ = def;
    }

    fn finish_nominal(&mut self, _item: &rill_hir::Item, def: &rill_hir::TypeDef) {
        let id = *self.nominal_by_item.get(&_item.id).expect("declared above");
        let kind = match &def.body {
            TypeBody::Record(record) | TypeBody::View(record) => {
                let fields = record
                    .fields
                    .iter()
                    .map(|field| FieldTy {
                        name: field.name.sym,
                        mutable: field.mutable,
                        ty: self.ast_ty(&field.ty),
                        span: field.span,
                    })
                    .collect();
                if matches!(def.body, TypeBody::View(_)) {
                    NominalKind::View(fields)
                } else {
                    NominalKind::Record(fields)
                }
            }
            TypeBody::Variants(variants) => NominalKind::Enum(
                variants
                    .iter()
                    .map(|variant| VariantTy {
                        name: variant.name.sym,
                        payload: variant.payload.iter().map(|t| self.ast_ty(t)).collect(),
                        span: variant.span,
                    })
                    .collect(),
            ),
            TypeBody::Alias(ty) => NominalKind::Alias(self.ast_ty(ty)),
            TypeBody::Error(_) => NominalKind::Opaque,
        };
        self.nominals[id.0 as usize].kind = kind;
    }

    fn param_ty(&mut self, param: &rill_hir::Param) -> Ty {
        match &param.ty {
            Some(ty) => self.ast_ty(ty),
            None => Ty::Error,
        }
    }

    /// Type syntax -> [`Ty`], resolving names in the type namespace.
    fn ast_ty(&mut self, ty: &AstType) -> Ty {
        match ty {
            AstType::Path { segments, span } => self.path_ty(segments, *span),
            AstType::App { ctor, args, span } => {
                let name = match &**ctor {
                    AstType::Path { segments, .. } => {
                        segments.last().map(|s| self.text(s.sym).to_string())
                    }
                    _ => None,
                };
                match name.as_deref() {
                    Some("Slice") => {
                        let elem = args.first().map(|a| self.ast_ty(a)).unwrap_or(Ty::Error);
                        Ty::Array {
                            elem: Box::new(elem),
                            len: None,
                        }
                    }
                    Some("Array") => {
                        let elem = args.first().map(|a| self.ast_ty(a)).unwrap_or(Ty::Error);
                        Ty::Array {
                            elem: Box::new(elem),
                            len: None,
                        }
                    }
                    Some("Box") => args.first().map(|a| self.ast_ty(a)).unwrap_or(Ty::Error),
                    _ => {
                        let id = self.lookup_nominal_in(ctor);
                        match id {
                            Some(id) => Ty::Nominal(id),
                            None => {
                                self.err(*span, "unknown type");
                                Ty::Error
                            }
                        }
                    }
                }
            }
            AstType::Tuple { elems, .. } if elems.is_empty() => Ty::Unit,
            AstType::Tuple { elems, .. } => {
                Ty::Tuple(elems.iter().map(|e| self.ast_ty(e)).collect())
            }
            AstType::Array { elem, len, .. } => {
                let len = len.as_ref().and_then(|expr| self.const_eval(expr));
                Ty::Array {
                    elem: Box::new(self.ast_ty(elem)),
                    len,
                }
            }
            AstType::Fn {
                params,
                ret,
                effects,
                ..
            } => Ty::Fn {
                params: params.iter().map(|p| self.ast_ty(&p.ty)).collect(),
                ret: Box::new(ret.as_ref().map(|r| self.ast_ty(r)).unwrap_or(Ty::Unit)),
                effects: self.row_effects(effects),
            },
            AstType::Ptr { pointee, .. } => self.ast_ty(pointee),
            AstType::Dyn { .. } => Ty::Text,
            AstType::Error(_) => Ty::Error,
        }
    }

    fn path_ty(&mut self, segments: &[rill_ast::Ident], span: Span) -> Ty {
        let name = self.text(segments[0].sym).to_string();
        match name.as_str() {
            "i8" => Ty::Int {
                bits: 8,
                signed: true,
            },
            "i16" => Ty::Int {
                bits: 16,
                signed: true,
            },
            "i32" => Ty::i32(),
            "i64" | "isize" => Ty::Int {
                bits: 64,
                signed: true,
            },
            "i128" => Ty::Int {
                bits: 128,
                signed: true,
            },
            "u8" => Ty::Int {
                bits: 8,
                signed: false,
            },
            "u16" => Ty::Int {
                bits: 16,
                signed: false,
            },
            "u32" => Ty::Int {
                bits: 32,
                signed: false,
            },
            "u64" | "usize" => Ty::usize_(),
            "u128" => Ty::Int {
                bits: 128,
                signed: false,
            },
            "f32" => Ty::Float { bits: 32 },
            "f64" => Ty::Float { bits: 64 },
            "Bool" => Ty::Bool,
            "String" | "Text" => Ty::Text,
            _ => {
                // A declared nominal: the item name resolves to a type.
                let item = self
                    .hir
                    .named_items(segments[0].sym)
                    .into_iter()
                    .find(|id| matches!(self.hir.item(*id).kind.namespace(), Namespace::Type));
                match item.and_then(|id| self.nominal_by_item.get(&id).copied()) {
                    Some(id) => match &self.nominals[id.0 as usize].kind {
                        NominalKind::Alias(ty) => ty.clone(),
                        _ => Ty::Nominal(id),
                    },
                    None => {
                        self.err(span, format!("unknown type `{name}`"));
                        Ty::Error
                    }
                }
            }
        }
    }

    fn lookup_nominal_in(&self, ty: &AstType) -> Option<NominalId> {
        let segments = match ty {
            AstType::Path { segments, .. } => segments,
            _ => return None,
        };
        self.hir
            .named_items(segments[0].sym)
            .into_iter()
            .find_map(|id| self.nominal_by_item.get(&id).copied())
    }

    fn row_effects(&mut self, row: &EffectRow) -> Vec<Effect> {
        row.items
            .iter()
            .flat_map(|item| {
                let name = self.text(item.name.sym).to_string();
                // `transacts` is a lattice row: reads | writes | alloc (§5.2).
                if name == "transacts" {
                    vec![
                        Effect::Reads,
                        Effect::Writes,
                        Effect::Alloc,
                        Effect::Decides,
                    ]
                } else {
                    vec![Effect::from_name(&name, item.name.sym)]
                }
            })
            .collect()
    }

    /// Constant-folds the small `comptime` expressions we need for shapes.
    fn const_eval(&mut self, expr: &rill_ast::Expr) -> Option<u64> {
        match expr {
            rill_ast::Expr::Literal { text, .. } => {
                let cleaned = text.trim_end_matches(|c: char| c.is_alphabetic());
                cleaned.parse::<u64>().ok()
            }
            _ => None,
        }
    }
}

impl Checker<'_> {
    /// Checks one item's body, in its own effect context.
    fn item_body(&mut self, item: &rill_hir::Item) {
        match &item.kind {
            ItemKind::Fn(def) => {
                self.bind_params(def.params.iter().map(|p| (p.local, p.ty.as_ref())));
                if let Some(ret) = &def.ret {
                    let expected = self.ast_ty(&ret.ty);
                    if let Some(body) = &def.body {
                        self.body_effects.clear();
                        let found = self.expr(body);
                        self.unify(&expected, &found, body.span, "checking the function's body");
                    }
                } else if let Some(body) = &def.body {
                    self.body_effects.clear();
                    let found = self.expr(body);
                    self.unify(&Ty::Unit, &found, body.span, "checking the function's body");
                }
            }
            ItemKind::Project(def) => {
                self.bind_params(def.params.iter().map(|p| (p.local, p.ty.as_ref())));
                let expected = def
                    .ret
                    .as_ref()
                    .map(|slot| self.ast_ty(&slot.ty))
                    .unwrap_or(Ty::Unit);
                self.body_effects.clear();
                let found = self.expr(&def.body);
                self.unify(
                    &expected,
                    &found,
                    def.body.span,
                    "checking the projection's body",
                );
            }
            ItemKind::Stmt(stmt) => {
                self.body_effects.clear();
                self.stmt(stmt);
            }
            _ => {}
        }
    }

    fn bind_params<'p>(
        &mut self,
        params: impl Iterator<Item = (Option<LocalId>, Option<&'p AstType>)>,
    ) {
        for (local, ty) in params {
            let ty = ty.map(|t| self.ast_ty(t)).unwrap_or(Ty::Error);
            if let Some(local) = local {
                self.local_types[local.0 as usize] = Some(ty);
            }
        }
    }

    fn check_declared_effects(&mut self, item: &rill_hir::Item) {
        let (declared, body) = match &item.kind {
            ItemKind::Fn(def) => (
                self.row_effects(&def.effects),
                def.body.as_ref().map(|body| body.id),
            ),
            ItemKind::Project(def) => (self.row_effects(&def.effects), Some(def.body.id)),
            _ => return,
        };
        let Some(body_id) = body else { return };
        let Some(used) = self.effects_of.get(&body_id).cloned() else {
            return;
        };
        let missing: Vec<String> = used
            .iter()
            .filter(|effect| !declared.contains(effect))
            .map(|effect| effect.name())
            .collect();
        if !missing.is_empty() {
            let name = item
                .name
                .map(|n| self.text(n).to_string())
                .unwrap_or_default();
            let declared_text = if declared.is_empty() {
                "computes".to_string()
            } else {
                declared
                    .iter()
                    .map(|e| e.name())
                    .collect::<Vec<_>>()
                    .join(" | ")
            };
            self.diagnostics.push(
                Diagnostic::error(
                    item.span,
                    format!(
                        "`{name}` performs {} but its signature declares <{declared_text}>",
                        describe_effects(&missing)
                    ),
                )
                .note(
                    "add the effect to the declaration, or call a function that does not need it"
                        .to_string(),
                ),
            );
        }
    }

    fn add_effects(&mut self, effects: &[Effect]) {
        for effect in effects {
            if !self.body_effects.contains(effect) {
                self.body_effects.push(*effect);
            }
        }
    }

    // -- expressions -------------------------------------------------------

    fn expr(&mut self, expr: &Expr) -> Ty {
        let ty = self.expr_kind(expr);
        self.expr_types.insert(expr.id, ty.clone());
        ty
    }

    fn expr_kind(&mut self, expr: &Expr) -> Ty {
        match &expr.kind {
            ExprKind::Error => Ty::Error,
            ExprKind::Literal { kind, text } => match kind {
                rill_ast::LiteralKind::Int => {
                    // A literal takes the type required by context; the default
                    // is `i32` (PLAN's examples are all `i32`/`i64`).
                    match text.rsplit_once(|c: char| c.is_alphabetic()) {
                        Some((_, suffix)) if !suffix.is_empty() => match suffix {
                            "i8" => Ty::Int {
                                bits: 8,
                                signed: true,
                            },
                            "i16" => Ty::Int {
                                bits: 16,
                                signed: true,
                            },
                            "i32" => Ty::i32(),
                            "i64" | "isize" => Ty::Int {
                                bits: 64,
                                signed: true,
                            },
                            "u8" => Ty::Int {
                                bits: 8,
                                signed: false,
                            },
                            "u16" => Ty::Int {
                                bits: 16,
                                signed: false,
                            },
                            "u32" => Ty::Int {
                                bits: 32,
                                signed: false,
                            },
                            "u64" | "usize" => Ty::usize_(),
                            _ => Ty::i32(),
                        },
                        _ => Ty::i32(),
                    }
                }
                rill_ast::LiteralKind::Float => match text.rsplit_once(|c: char| c.is_alphabetic())
                {
                    Some((_, "f32")) => Ty::Float { bits: 32 },
                    _ => Ty::Float { bits: 64 },
                },
                rill_ast::LiteralKind::Bool => Ty::Bool,
                rill_ast::LiteralKind::Text | rill_ast::LiteralKind::RawText => Ty::Text,
                rill_ast::LiteralKind::Char => Ty::Int {
                    bits: 8,
                    signed: false,
                },
            },
            ExprKind::Path { segments, res } => match res {
                Res::Local(id) => self
                    .local_types
                    .get(id.0 as usize)
                    .and_then(|t| t.clone())
                    .unwrap_or(Ty::Error),
                Res::Item(id) => match self.item_types.get(id.0 as usize).cloned() {
                    Some(ItemTy::Fn {
                        params,
                        ret,
                        effects,
                    }) => {
                        self.add_effects(&effects);
                        Ty::Fn {
                            params,
                            ret: Box::new(ret),
                            effects,
                        }
                    }
                    Some(ItemTy::Nominal(nominal)) => {
                        // A nominal in value position is its constructor; the
                        // path's last segment selects the variant.
                        let last = segments.last().map(|s| self.text(*s).to_string());
                        let payload =
                            last.and_then(|name| match &self.nominals[nominal.0 as usize].kind {
                                NominalKind::Enum(variants) => variants
                                    .iter()
                                    .find(|v| self.text(v.name) == name)
                                    .map(|v| v.payload.clone()),
                                NominalKind::Record(fields) | NominalKind::View(fields) => {
                                    Some(fields.iter().map(|f| f.ty.clone()).collect())
                                }
                                _ => None,
                            });
                        match payload {
                            // A payload-less variant is a value, not a function.
                            Some(params) if !params.is_empty() => Ty::Fn {
                                params,
                                ret: Box::new(Ty::Nominal(nominal)),
                                effects: Vec::new(),
                            },
                            _ => Ty::Nominal(nominal),
                        }
                    }
                    _ => match self.nominal_by_item.get(id).copied() {
                        Some(nominal) => Ty::Nominal(nominal),
                        None => Ty::Error,
                    },
                },
                Res::Import(_) => {
                    // `Res::Import` records the *import's* path; the reference's
                    // own segments name the item being used (`io::println`).
                    self.add_effects(&[Effect::Io]);
                    let last = segments.last().map(|s| self.text(*s).to_string());
                    if last.as_deref() == Some("println") {
                        Ty::Fn {
                            params: vec![Ty::Text],
                            ret: Box::new(Ty::Unit),
                            effects: vec![Effect::Io],
                        }
                    } else {
                        Ty::Fn {
                            params: Vec::new(),
                            ret: Box::new(Ty::Error),
                            effects: vec![Effect::Io],
                        }
                    }
                }
                Res::Field => Ty::Error,
                Res::Error => Ty::Error,
            },
            ExprKind::Tuple(elems) => Ty::Tuple(elems.iter().map(|e| self.expr(e)).collect()),
            ExprKind::Array(elems) => {
                let elem = self.fresh();
                for e in elems {
                    let found = self.expr(e);
                    self.unify(&elem, &found, e.span, "checking an array literal");
                }
                Ty::Array {
                    elem: Box::new(elem),
                    len: Some(elems.len() as u64),
                }
            }
            ExprKind::Block(block) => self.block(block),
            ExprKind::Lambda { params, body } => {
                let mut tys = Vec::new();
                for param in params {
                    let ty = self.fresh();
                    self.pattern(param, &ty);
                    tys.push(ty);
                }
                let ret = self.block(body);
                Ty::Fn {
                    params: tys,
                    ret: Box::new(ret),
                    effects: Vec::new(),
                }
            }
            ExprKind::Call { callee, args } => self.call(expr, callee, args),
            ExprKind::Index { base, args } => {
                let base_ty = self.expr(base);
                match self.resolve_deep(&base_ty) {
                    Ty::Array { elem, len } => {
                        if args.len() != 1 {
                            self.err(expr.span, "indexing takes exactly one index");
                        }
                        for arg in args {
                            let index = self.expr(&arg.value);
                            self.unify(&Ty::usize_(), &index, arg.span, "checking an index");
                        }
                        let _ = len;
                        (*elem).clone()
                    }
                    Ty::Error => Ty::Error,
                    other => {
                        // `f[x]` — a failable call on a function value.
                        let display = self.display(&other);
                        self.err(
                            expr.span,
                            format!("cannot index `{display}`: it is not an array"),
                        );
                        for arg in args {
                            self.expr(&arg.value);
                        }
                        Ty::Error
                    }
                }
            }
            ExprKind::Try { base } => {
                self.add_effects(&[Effect::Decides]);
                self.expr(base)
            }
            ExprKind::Unary { op, operand } => {
                let operand_ty = self.expr(operand);
                match op {
                    UnaryOp::Neg => {
                        self.numeric(&operand_ty, expr.span, "negation");
                        operand_ty
                    }
                    UnaryOp::Not => {
                        self.unify(&Ty::Bool, &operand_ty, expr.span, "checking `not`");
                        Ty::Bool
                    }
                    UnaryOp::Deref => operand_ty,
                    UnaryOp::Size => {
                        self.add_effects(&[]);
                        match self.resolve_deep(&operand_ty) {
                            Ty::Array { .. } | Ty::Error => {}
                            other => {
                                let display = self.display(&other);
                                self.err(
                                    expr.span,
                                    format!("`size` expects an array, found `{display}`"),
                                );
                            }
                        }
                        Ty::usize_()
                    }
                    UnaryOp::Shapeof | UnaryOp::Reshape => operand_ty,
                }
            }
            ExprKind::Binary { op, lhs, rhs } => self.binary(expr.span, *op, lhs, rhs),
            ExprKind::Cast { expr: inner, ty } => {
                self.expr(inner);
                self.ast_ty(ty)
            }
            ExprKind::Is { expr: inner, .. } => {
                self.expr(inner);
                Ty::Bool
            }
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let cond_ty = self.cond(cond);
                self.unify(
                    &Ty::Bool,
                    &cond_ty,
                    cond.span(),
                    "checking an `if` condition",
                );
                let then_ty = self.block(then_branch);
                match else_branch {
                    Some(ElseBranch::Block(block)) => {
                        let else_ty = self.block(block);
                        self.unify(
                            &then_ty,
                            &else_ty,
                            expr.span,
                            "checking `if`/`else` branches",
                        );
                        then_ty
                    }
                    Some(ElseBranch::If(other)) => {
                        let else_ty = self.expr(other);
                        self.unify(
                            &then_ty,
                            &else_ty,
                            expr.span,
                            "checking `if`/`else` branches",
                        );
                        then_ty
                    }
                    None => Ty::Unit,
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                let scrutinee_ty = self.expr(scrutinee);
                let result = self.fresh();
                if arms.is_empty() {
                    self.err(expr.span, "`match` without arms");
                    return Ty::Error;
                }
                for arm in arms {
                    self.pattern(&arm.pattern, &scrutinee_ty);
                    if let Some(guard) = &arm.guard {
                        let guard_ty = self.expr(guard);
                        self.unify(&Ty::Bool, &guard_ty, guard.span, "checking an arm guard");
                    }
                    let body_ty = self.expr(&arm.body);
                    self.unify(&result, &body_ty, arm.body.span, "checking match arms");
                }
                result
            }
            ExprKind::Comptime { body } => self.block(body),
            ExprKind::Unsafe { body } => self.block(body),
            ExprKind::Handler { kind, body } => {
                let previous = self.handler;
                self.handler = Some(*kind);
                if matches!(kind, rill_ast::HandlerKind::All) {
                    self.add_effects(&[Effect::Alloc]);
                }
                let inner = self.block(body);
                self.handler = previous;
                match kind {
                    rill_ast::HandlerKind::Option => Ty::Nominal(self.option_id),
                    rill_ast::HandlerKind::One => inner,
                    rill_ast::HandlerKind::All => Ty::Array {
                        elem: Box::new(inner),
                        len: None,
                    },
                }
            }
            ExprKind::Concurrent { kind, arms } => {
                self.add_effects(&[Effect::Suspends]);
                let mut tys = Vec::new();
                for arm in arms {
                    tys.push(self.expr(arm));
                }
                let _ = kind;
                match tys.len() {
                    0 => Ty::Unit,
                    1 => tys.pop().expect("one element"),
                    _ => Ty::Tuple(tys),
                }
            }
        }
    }

    fn numeric(&mut self, ty: &Ty, span: Span, context: &str) {
        match self.resolve_deep(ty) {
            Ty::Int { .. } | Ty::Float { .. } | Ty::Var(_) | Ty::Error => {}
            other => {
                let display = self.display(&other);
                self.err(
                    span,
                    format!("`{context}` expects a numeric type, found `{display}`"),
                );
            }
        }
    }

    fn binary(&mut self, span: Span, op: BinaryOp, lhs: &Expr, rhs: &Expr) -> Ty {
        let left = self.expr(lhs);
        // A `.`-selected name is a field or a method of the left operand.
        if op == BinaryOp::Flow {
            if let Some((_, Res::Field)) = rhs.path_res() {
                let field = self.rhs_field(rhs, &left, span);
                self.expr_types.insert(rhs.id, field.clone());
                // `items.fold 0 { … }` composes into a call; the checker reports
                // the composed form so the result type comes from the callee.
                return field;
            }
        }
        let right = self.expr(rhs);
        match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
                self.numeric(&left, span, "arithmetic");
                self.unify(&left, &right, span, "checking an arithmetic operator");
                left
            }
            BinaryOp::BitAnd
            | BinaryOp::BitXor
            | BinaryOp::BitOr
            | BinaryOp::Shl
            | BinaryOp::Shr => {
                match self.resolve_deep(&left) {
                    Ty::Int { .. } | Ty::Var(_) | Ty::Error => {}
                    other => {
                        let display = self.display(&other);
                        self.err(
                            span,
                            format!("bitwise operators expect integers, found `{display}`"),
                        );
                    }
                }
                self.unify(&left, &right, span, "checking a bitwise operator");
                left
            }
            BinaryOp::And | BinaryOp::Or => {
                self.unify(&Ty::Bool, &left, span, "checking a logical operator");
                self.unify(&Ty::Bool, &right, span, "checking a logical operator");
                Ty::Bool
            }
            BinaryOp::Eq | BinaryOp::EqEq | BinaryOp::NotEq => {
                self.unify(&left, &right, span, "checking an equality test");
                Ty::Bool
            }
            BinaryOp::Lt | BinaryOp::Gt | BinaryOp::LtEq | BinaryOp::GtEq => {
                self.numeric(&left, span, "comparison");
                self.unify(&left, &right, span, "checking a comparison");
                Ty::Bool
            }
            BinaryOp::Concat => {
                match self.resolve_deep(&left) {
                    Ty::Text | Ty::Var(_) | Ty::Error => {}
                    Ty::Array { .. } => {}
                    other => {
                        let display = self.display(&other);
                        self.err(
                            span,
                            format!("`++` expects strings or arrays, found `{display}`"),
                        );
                    }
                }
                self.unify(&left, &right, span, "checking `++`");
                left
            }
            BinaryOp::RangeInclusive | BinaryOp::RangeHalfOpen => {
                self.numeric(&left, span, "range");
                self.unify(&left, &right, span, "checking a range");
                Ty::Range(Box::new(left))
            }
            BinaryOp::Choice => {
                self.add_effects(&[Effect::Decides]);
                self.unify(&left, &right, span, "checking `|` alternatives");
                Ty::Choice(Box::new(left))
            }
            BinaryOp::Flow => {
                let display = self.display(&left);
                self.err(
                    span,
                    format!("cannot select `…` from `{display}`: no such field or method"),
                );
                Ty::Error
            }
        }
    }

    /// Types `lhs.field`, with a diagnostic listing the available fields.
    fn rhs_field(&mut self, rhs: &Expr, base: &Ty, span: Span) -> Ty {
        let name = match rhs.path_res() {
            Some((segments, _)) => segments[0],
            None => return Ty::Error,
        };
        let text = self.text(name).to_string();
        match self.resolve_deep(base) {
            Ty::Record(id) | Ty::Nominal(id) => {
                let fields = match &self.nominals[id.0 as usize].kind {
                    NominalKind::Record(fields) | NominalKind::View(fields) => fields.clone(),
                    _ => Vec::new(),
                };
                match fields.iter().find(|field| field.name == name) {
                    Some(field) => field.ty.clone(),
                    None => {
                        let names: Vec<&str> = fields.iter().map(|f| self.text(f.name)).collect();
                        self.diagnostics.push(
                            Diagnostic::error(
                                span,
                                format!("`{}` has no field `{text}`", self.nominal_name(id)),
                            )
                            .note(if names.is_empty() {
                                "this type has no fields".to_string()
                            } else {
                                format!("available fields: {}", names.join(", "))
                            }),
                        );
                        Ty::Error
                    }
                }
            }
            Ty::Tuple(elems) => match text.parse::<usize>() {
                Ok(index) if index < elems.len() => elems[index].clone(),
                Ok(index) => {
                    self.err(
                        span,
                        format!(
                            "tuple index {index} is out of range (length {})",
                            elems.len()
                        ),
                    );
                    Ty::Error
                }
                Err(_) => {
                    self.err(span, format!("tuples are indexed by number, not `{text}`"));
                    Ty::Error
                }
            },
            Ty::Error => Ty::Error,
            other => {
                let display = self.display(&other);
                self.err(span, format!("`{display}` has no field `{text}`"));
                Ty::Error
            }
        }
    }

    fn call(&mut self, whole: &Expr, callee: &Expr, args: &[rill_hir::Arg]) -> Ty {
        // `items.fold 0 { … }` — a `.`-selected method applied to `items`.
        if let ExprKind::Binary {
            op: BinaryOp::Flow,
            lhs,
            rhs,
        } = &callee.kind
        {
            if let Some((_, Res::Field)) = rhs.path_res() {
                let receiver = self.expr(lhs);
                let _ = receiver;
                let method_ty = self.expr(rhs);
                let arg_tys: Vec<Ty> = args.iter().map(|arg| self.expr(&arg.value)).collect();
                return self.apply(
                    whole.span,
                    &method_ty,
                    &arg_tys,
                    args,
                    "checking a method call",
                );
            }
        }
        let callee_ty = self.expr(callee);
        let arg_tys: Vec<Ty> = args.iter().map(|arg| self.expr(&arg.value)).collect();
        self.apply(whole.span, &callee_ty, &arg_tys, args, "checking a call")
    }

    fn apply(
        &mut self,
        span: Span,
        callee: &Ty,
        arg_tys: &[Ty],
        args: &[rill_hir::Arg],
        context: &str,
    ) -> Ty {
        match self.resolve_deep(callee) {
            Ty::Fn {
                params,
                ret,
                effects,
            } => {
                if params.len() != arg_tys.len() {
                    self.diagnostics.push(
                        Diagnostic::error(
                            span,
                            format!(
                                "this takes {} argument(s) but {} were given",
                                params.len(),
                                arg_tys.len()
                            ),
                        )
                        .note("Rill calls are flat: `f a b` passes two arguments".to_string()),
                    );
                    return Ty::Error;
                }
                self.add_effects(&effects);
                for (index, (param, arg)) in params.iter().zip(arg_tys.iter()).enumerate() {
                    let arg_span = args.get(index).map(|a| a.span).unwrap_or(span);
                    self.unify(param, arg, arg_span, context);
                }
                (*ret).clone()
            }
            Ty::Nominal(id) | Ty::Record(id) => {
                let expected = match &self.nominals[id.0 as usize].kind {
                    NominalKind::Record(fields) | NominalKind::View(fields) => fields.len(),
                    NominalKind::Enum(_) => 0,
                    _ => 0,
                };
                if expected != arg_tys.len() {
                    self.diagnostics.push(
                        Diagnostic::error(
                            span,
                            format!(
                                "`{}` takes {expected} value(s) but {} were given",
                                self.nominal_name(id),
                                arg_tys.len()
                            ),
                        )
                        .note(format!("while {context}")),
                    );
                }
                Ty::Nominal(id)
            }
            Ty::Error => Ty::Error,
            other => {
                let display = self.display(&other);
                self.err(span, format!("`{display}` is not callable"));
                Ty::Error
            }
        }
    }

    // -- statements --------------------------------------------------------

    fn block(&mut self, block: &Block) -> Ty {
        for stmt in &block.stmts {
            self.stmt(stmt);
        }
        match &block.tail {
            Some(tail) => self.expr(tail),
            None => Ty::Unit,
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Error(_) => {}
            Stmt::Let { item, .. } => {
                if let Some(inner) = self.hir.try_item(*item) {
                    self.item_body(inner);
                }
            }
            Stmt::Var {
                local, ty, init, ..
            } => {
                let init_ty = self.expr(init);
                let expected = match ty {
                    Some(ast) => self.ast_ty(ast),
                    None => init_ty.clone(),
                };
                self.unify(
                    &expected,
                    &init_ty,
                    init.span,
                    "checking a `var` initializer",
                );
                self.local_types[local.0 as usize] = Some(expected);
            }
            Stmt::Bind { pattern, value, .. } => {
                let value_ty = self.expr(value);
                let bound = self.element_ty(&value_ty);
                self.pattern(pattern, &bound);
            }
            Stmt::TryBind { local, value, .. } => {
                let value_ty = self.expr(value);
                let bound = self.element_ty(&value_ty);
                self.local_types[local.0 as usize] = Some(bound);
            }
            Stmt::Set { place, value, .. } => {
                let place_ty = self.expr(place);
                let value_ty = self.expr(value);
                self.unify(&place_ty, &value_ty, stmt.span(), "checking an assignment");
            }
            Stmt::Yield { value, .. } => {
                self.expr(value);
            }
            Stmt::When { cond, .. } => {
                let cond_ty = self.cond(cond);
                self.unify(&Ty::Bool, &cond_ty, cond.span(), "checking a `when` guard");
            }
            Stmt::For {
                pattern,
                iter,
                body,
                ..
            } => {
                let iter_ty = self.expr(iter);
                let elem = self.loop_element_ty(&iter_ty);
                self.pattern(pattern, &elem);
                self.block(body);
            }
            Stmt::Expr(expr) => {
                self.expr(expr);
            }
        }
    }

    /// The element type a `for (x := e)` loop iterates over.
    fn loop_element_ty(&mut self, ty: &Ty) -> Ty {
        match self.resolve_deep(ty) {
            Ty::Range(elem) => (*elem).clone(),
            Ty::Array { elem, .. } => (*elem).clone(),
            other => other,
        }
    }

    /// The element type an `x := e` / `for (x := e)` binding iterates over.
    fn element_ty(&mut self, ty: &Ty) -> Ty {
        match self.resolve_deep(ty) {
            // Outside a handler, `x := e` binds the value itself.
            _ if self.handler.is_none() => ty.clone(),
            Ty::Range(elem) => (*elem).clone(),
            Ty::Array { elem, .. } => (*elem).clone(),
            Ty::Nominal(id) if id == self.option_id => self.fresh(),
            Ty::Choice(inner) => (*inner).clone(),
            other => other,
        }
    }

    fn cond(&mut self, cond: &Cond) -> Ty {
        match cond {
            Cond::Expr(expr) => self.expr(expr),
            Cond::Bind { pattern, value, .. } => {
                let value_ty = self.expr(value);
                let bound = self.element_ty(&value_ty);
                self.pattern(pattern, &bound);
                Ty::Bool
            }
        }
    }

    /// Binds a pattern's variables to (parts of) `scrutinee`.
    fn pattern(&mut self, pattern: &Pattern, scrutinee: &Ty) {
        match pattern {
            Pattern::Wildcard(_) | Pattern::Literal { .. } | Pattern::Error(_) => {}
            Pattern::Binding { local, .. } => {
                self.local_types[local.0 as usize] = Some(scrutinee.clone());
            }
            Pattern::Tuple { elems, span } => {
                let expected = Ty::Tuple(elems.iter().map(|_| self.fresh()).collect());
                let elems_ty = match self.resolve_deep(scrutinee) {
                    Ty::Tuple(parts) if parts.len() == elems.len() => parts,
                    Ty::Tuple(parts) => {
                        self.err(
                            *span,
                            format!(
                                "pattern has {} element(s) but the value has {}",
                                elems.len(),
                                parts.len()
                            ),
                        );
                        return;
                    }
                    Ty::Error => return,
                    other => {
                        let display = self.display(&other);
                        self.err(
                            *span,
                            format!("cannot match a tuple pattern against `{display}`"),
                        );
                        return;
                    }
                };
                for (elem, ty) in elems.iter().zip(elems_ty.iter()) {
                    let ty = ty.clone();
                    self.pattern(elem, &ty);
                }
                let _ = expected;
            }
            Pattern::Constructor {
                path,
                args,
                span,
                res,
            } => {
                let name = path[0];
                // The resolver already bound the pattern's head: an enum
                // variant, or a type name used as a record pattern.
                let nominal = match res {
                    Res::Item(id) => self.nominal_by_item.get(id).copied(),
                    _ => self
                        .hir
                        .named_items(name)
                        .into_iter()
                        .find_map(|id| self.nominal_by_item.get(&id).copied()),
                };
                let Some(id) = nominal else {
                    self.err(
                        *span,
                        format!("`{}` is not a type or variant", self.text(name)),
                    );
                    return;
                };
                match self.nominals[id.0 as usize].kind.clone() {
                    NominalKind::Enum(variants) => {
                        let payload = match variants.iter().find(|v| v.name == name) {
                            Some(variant) => variant.payload.clone(),
                            None => {
                                // `Pair (a, b)` where `Pair` is the type itself:
                                // treat its single record as the payload.
                                match &self.nominals[id.0 as usize].kind {
                                    NominalKind::Record(fields) => {
                                        fields.iter().map(|f| f.ty.clone()).collect()
                                    }
                                    _ => {
                                        let names: Vec<&str> =
                                            variants.iter().map(|v| self.text(v.name)).collect();
                                        self.diagnostics.push(
                                            Diagnostic::error(
                                                *span,
                                                format!(
                                                    "`{}` has no variant `{}`",
                                                    self.nominal_name(id),
                                                    self.text(name)
                                                ),
                                            )
                                            .note(format!("variants: {}", names.join(", "))),
                                        );
                                        return;
                                    }
                                }
                            }
                        };
                        if !payload.is_empty() && args.len() != payload.len() {
                            self.err(
                                *span,
                                format!(
                                    "`{}` takes {} value(s), the pattern binds {}",
                                    self.text(name),
                                    payload.len(),
                                    args.len()
                                ),
                            );
                        }
                        for (arg, ty) in args.iter().zip(payload.iter()) {
                            let ty = ty.clone();
                            self.pattern(arg, &ty);
                        }
                    }
                    NominalKind::Record(fields) => {
                        for (index, arg) in args.iter().enumerate() {
                            let ty = fields.get(index).map(|f| f.ty.clone()).unwrap_or(Ty::Error);
                            self.pattern(arg, &ty);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn describe_effects(effects: &[String]) -> String {
    effects.join(", ")
}
