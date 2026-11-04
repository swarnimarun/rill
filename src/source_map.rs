//! Opt-in origins for the unmodified parsed AST, not identities across edits,
//! module rewriting, inference, specialization or lowering.
use crate::{
    ast::*,
    parser::ParsedModule,
    source::{LexemeKind, Span},
    syntax::{AstDeclaration, NodeId, SyntaxElement, SyntaxKind, SyntaxTree},
};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AstPath {
    pub declaration: AstDeclaration,
    pub steps: Vec<AstStep>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AstStep {
    Parameter(usize),
    FunctionBody,
    FunctionResult,
    Annotation,
    Variant(usize),
    FieldType(usize),
    TypeArgument(usize),
    ForeignParameter(usize),
    ForeignParameterType,
    ForeignResult,
    ConstantType,
    LetValue,
    LetBody,
    BinaryLeft,
    BinaryRight,
    Condition,
    Then,
    Else,
    WhileBody,
    AssignValue,
    Argument(usize),
    RecordField(usize),
    MatchValue,
    Arm(usize),
    ArmPattern,
    ArmBody,
    PatternField(usize),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AstNodeKind {
    Function,
    ExternFunction,
    Constant,
    TypeDeclaration,
    Variant,
    Parameter,
    ForeignParameter,
    ForeignType,
    TypeExpression,
    Expression,
    Pattern,
    MatchArm,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AstNode<'a> {
    Function(&'a Function),
    ExternFunction(&'a ExternFunction),
    Constant(&'a ExternConst),
    TypeDeclaration(&'a TypeDecl),
    Variant(&'a VariantDecl),
    Parameter(&'a Param),
    ForeignParameter(&'a ExternParam),
    ForeignType(&'a ExternType),
    TypeExpression(&'a TypeExpr),
    Expression(&'a Expr),
    Pattern(&'a Pattern),
    MatchArm(&'a MatchArm),
}
impl AstPath {
    pub fn root(declaration: AstDeclaration) -> Self {
        Self {
            declaration,
            steps: Vec::new(),
        }
    }
    pub fn child(&self, step: AstStep) -> Self {
        let mut path = self.clone();
        path.steps.push(step);
        path
    }
    /// Validates every structural step and index against this supplied program.
    /// Resolving a path is not proof that a changed program has the old origin.
    pub fn resolve_program<'a>(&self, program: &'a Program) -> Option<AstNode<'a>> {
        let mut node = match self.declaration {
            AstDeclaration::Function(i) => AstNode::Function(program.functions.get(i)?),
            AstDeclaration::ExternFunction(i) => AstNode::ExternFunction(program.externs.get(i)?),
            AstDeclaration::Constant(i) => AstNode::Constant(program.constants.get(i)?),
            AstDeclaration::Type(i) => AstNode::TypeDeclaration(program.types.get(i)?),
        };
        for step in &self.steps {
            node = node.child(*step)?;
        }
        Some(node)
    }
    pub fn resolve<'a>(&self, module: &'a ParsedModule) -> Option<AstNode<'a>> {
        self.resolve_program(&module.program)
    }
}
impl<'a> AstNode<'a> {
    pub fn kind(self) -> AstNodeKind {
        match self {
            Self::Function(_) => AstNodeKind::Function,
            Self::ExternFunction(_) => AstNodeKind::ExternFunction,
            Self::Constant(_) => AstNodeKind::Constant,
            Self::TypeDeclaration(_) => AstNodeKind::TypeDeclaration,
            Self::Variant(_) => AstNodeKind::Variant,
            Self::Parameter(_) => AstNodeKind::Parameter,
            Self::ForeignParameter(_) => AstNodeKind::ForeignParameter,
            Self::ForeignType(_) => AstNodeKind::ForeignType,
            Self::TypeExpression(_) => AstNodeKind::TypeExpression,
            Self::Expression(_) => AstNodeKind::Expression,
            Self::Pattern(_) => AstNodeKind::Pattern,
            Self::MatchArm(_) => AstNodeKind::MatchArm,
        }
    }
    pub fn child(self, step: AstStep) -> Option<Self> {
        use AstStep::*;
        Some(match (self, step) {
            (Self::Function(f), Parameter(i)) => Self::Parameter(f.params.get(i)?),
            (Self::Function(f), FunctionBody) => Self::Expression(&f.body),
            (Self::Function(f), FunctionResult) => {
                Self::TypeExpression(f.result_annotation.as_ref()?)
            }
            (Self::Parameter(p), Annotation) => Self::TypeExpression(p.annotation.as_ref()?),
            (Self::TypeDeclaration(t), Variant(i)) => Self::Variant(t.variants.get(i)?),
            (Self::Variant(v), FieldType(i)) => Self::TypeExpression(v.fields.get(i)?),
            (Self::TypeExpression(t), TypeArgument(i)) => Self::TypeExpression(t.args.get(i)?),
            (Self::ExternFunction(f), ForeignParameter(i)) => {
                Self::ForeignParameter(f.params.get(i)?)
            }
            (Self::ExternFunction(f), ForeignResult) => Self::ForeignType(&f.result),
            (Self::ForeignParameter(p), ForeignParameterType) => Self::ForeignType(&p.ty),
            (Self::Constant(c), ConstantType) => Self::ForeignType(&c.ty),
            (Self::Expression(Expr::Let(_, a, _) | Expr::LetMut(_, a, _)), LetValue) => {
                Self::Expression(a)
            }
            (Self::Expression(Expr::Let(_, _, b) | Expr::LetMut(_, _, b)), LetBody) => {
                Self::Expression(b)
            }
            (Self::Expression(Expr::Binary(_, a, _)), BinaryLeft) => Self::Expression(a),
            (Self::Expression(Expr::Binary(_, _, b)), BinaryRight) => Self::Expression(b),
            (Self::Expression(Expr::If(c, _, _) | Expr::While(c, _)), Condition) => {
                Self::Expression(c)
            }
            (Self::Expression(Expr::If(_, a, _)), Then) => Self::Expression(a),
            (Self::Expression(Expr::If(_, _, b)), Else) => Self::Expression(b),
            (Self::Expression(Expr::While(_, b)), WhileBody) => Self::Expression(b),
            (Self::Expression(Expr::Assign(_, v) | Expr::FieldAssign(_, _, v)), AssignValue) => {
                Self::Expression(v)
            }
            (Self::Expression(Expr::Call(_, args)), Argument(i)) => Self::Expression(args.get(i)?),
            (Self::Expression(Expr::Record(_, fields)), RecordField(i)) => {
                Self::Expression(&fields.get(i)?.1)
            }
            (Self::Expression(Expr::Match(v, _)), MatchValue) => Self::Expression(v),
            (Self::Expression(Expr::Match(_, arms)), Arm(i)) => Self::MatchArm(arms.get(i)?),
            (Self::MatchArm(a), ArmPattern) => Self::Pattern(&a.pattern),
            (Self::MatchArm(a), ArmBody) => Self::Expression(&a.body),
            (Self::Pattern(crate::ast::Pattern::Constructor(_, fields)), PatternField(i)) => {
                Self::Pattern(fields.get(i)?)
            }
            _ => return None,
        })
    }
    /// Semantic structural children, including parser-generated nodes.
    pub fn child_steps(self) -> Vec<AstStep> {
        use AstStep::*;
        match self {
            Self::Function(f) => (0..f.params.len())
                .map(Parameter)
                .chain(f.result_annotation.as_ref().map(|_| FunctionResult))
                .chain([FunctionBody])
                .collect(),
            Self::ExternFunction(f) => (0..f.params.len())
                .map(ForeignParameter)
                .chain([ForeignResult])
                .collect(),
            Self::Constant(_) => vec![ConstantType],
            Self::TypeDeclaration(t) => (0..t.variants.len()).map(Variant).collect(),
            Self::Variant(v) => (0..v.fields.len()).map(FieldType).collect(),
            Self::Parameter(p) => {
                if p.annotation.is_some() {
                    vec![Annotation]
                } else {
                    vec![]
                }
            }
            Self::ForeignParameter(_) => vec![ForeignParameterType],
            Self::TypeExpression(t) => (0..t.args.len()).map(TypeArgument).collect(),
            Self::Expression(e) => match e {
                Expr::Let(..) | Expr::LetMut(..) => vec![LetValue, LetBody],
                Expr::Binary(..) => vec![BinaryLeft, BinaryRight],
                Expr::If(..) => vec![Condition, Then, Else],
                Expr::While(..) => vec![Condition, WhileBody],
                Expr::Assign(..) | Expr::FieldAssign(..) => vec![AssignValue],
                Expr::Call(_, args) => (0..args.len()).map(Argument).collect(),
                Expr::Record(_, fields) => (0..fields.len()).map(RecordField).collect(),
                Expr::Match(_, arms) => std::iter::once(MatchValue)
                    .chain((0..arms.len()).map(Arm))
                    .collect(),
                _ => vec![],
            },
            Self::MatchArm(_) => vec![ArmPattern, ArmBody],
            Self::Pattern(crate::ast::Pattern::Constructor(_, fields)) => {
                (0..fields.len()).map(PatternField).collect()
            }
            _ => vec![],
        }
    }
}
/// Explicit source/desugaring provenance. Source grouping does not create AST
/// nodes; its inner semantic node retains its own source origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OriginKind {
    Source,
    BlockBinding,
    BlockSequence,
    StatementDiscard,
    ImplicitUnit,
    /// A unary subtraction rewrite, including its introduced zero operand.
    UnaryNegation,
    /// A boolean negation rewrite, including its introduced false operand.
    BooleanNegation,
    /// A nominal record's synthetic single AST variant.
    RecordVariant,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AstOrigin {
    pub path: AstPath,
    pub kind: AstNodeKind,
    pub syntax: NodeId,
    pub span: Span,
    pub provenance: OriginKind,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MappingError {
    pub path: Option<AstPath>,
    pub message: &'static str,
}
impl std::fmt::Display for MappingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "E-SOURCE-MAP: {}", self.message)
    }
}
impl std::error::Error for MappingError {}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AstSourceMap {
    entries: Vec<AstOrigin>,
    index: BTreeMap<AstPath, usize>,
}
impl AstSourceMap {
    /// Source declaration order, then semantic preorder. No partial result on failure.
    pub fn entries(&self) -> &[AstOrigin] {
        &self.entries
    }
    pub fn origin(&self, path: &AstPath) -> Option<&AstOrigin> {
        self.index.get(path).map(|&i| &self.entries[i])
    }
    pub fn from_tree(tree: &SyntaxTree<'_>) -> Result<Self, MappingError> {
        let module = tree.parsed_module().ok_or(MappingError {
            path: None,
            message: "source has syntax diagnostics",
        })?;
        let mut mapper = Mapper {
            tree,
            program: &module.program,
            map: Self {
                entries: Vec::new(),
                index: BTreeMap::new(),
            },
        };
        for declaration in tree.declarations() {
            mapper.declaration(AstPath::root(declaration.declaration), declaration.node)?;
        }
        // Validate total structural coverage independently of the source walk.
        let mut work = Vec::new();
        let program = &module.program;
        let roots = (0..program.functions.len())
            .map(AstDeclaration::Function)
            .chain((0..program.externs.len()).map(AstDeclaration::ExternFunction))
            .chain((0..program.constants.len()).map(AstDeclaration::Constant))
            .chain((0..program.types.len()).map(AstDeclaration::Type))
            .collect::<Vec<_>>();
        for declaration in roots.into_iter().rev() {
            work.push(AstPath::root(declaration));
        }
        let mut count = 0;
        while let Some(path) = work.pop() {
            let node = path
                .resolve_program(&module.program)
                .ok_or_else(|| fail(&path, "invalid structural path"))?;
            let origin = mapper
                .map
                .origin(&path)
                .ok_or_else(|| fail(&path, "missing AST origin"))?;
            if origin.kind != node.kind() {
                return Err(fail(&path, "origin category differs from AST"));
            }
            count += 1;
            for step in node.child_steps().into_iter().rev() {
                work.push(path.child(step));
            }
        }
        if count != mapper.map.entries.len() {
            return Err(MappingError {
                path: None,
                message: "extra or duplicate AST origins",
            });
        }
        Ok(mapper.map)
    }
}
impl SyntaxTree<'_> {
    /// Optional tooling pass. Its failure never changes strict parser acceptance.
    pub fn ast_source_map(&self) -> Result<AstSourceMap, MappingError> {
        AstSourceMap::from_tree(self)
    }
}
fn fail(path: &AstPath, message: &'static str) -> MappingError {
    MappingError {
        path: Some(path.clone()),
        message,
    }
}
struct Mapper<'a, 's> {
    tree: &'a SyntaxTree<'s>,
    program: &'a Program,
    map: AstSourceMap,
}
fn expression_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Expression
            | SyntaxKind::Block
            | SyntaxKind::WhileExpression
            | SyntaxKind::MatchExpression
            | SyntaxKind::LetExpression
            | SyntaxKind::IfExpression
            | SyntaxKind::AssignmentExpression
            | SyntaxKind::BinaryExpression
            | SyntaxKind::UnaryExpression
            | SyntaxKind::ParenthesizedExpression
            | SyntaxKind::UnitExpression
            | SyntaxKind::CallExpression
            | SyntaxKind::FieldExpression
            | SyntaxKind::NameExpression
            | SyntaxKind::RecordExpression
            | SyntaxKind::MoveExpression
            | SyntaxKind::LiteralExpression
    )
}
impl Mapper<'_, '_> {
    fn children(&self, node: NodeId) -> Vec<NodeId> {
        self.tree
            .node(node)
            .unwrap()
            .children()
            .iter()
            .filter_map(|e| match e {
                SyntaxElement::Node(n) => Some(*n),
                _ => None,
            })
            .collect()
    }
    fn nodes(&self, node: NodeId, kind: SyntaxKind) -> Vec<NodeId> {
        self.children(node)
            .into_iter()
            .filter(|&n| self.tree.node(n).unwrap().kind() == kind)
            .collect()
    }
    fn expressions(&self, node: NodeId) -> Vec<NodeId> {
        self.children(node)
            .into_iter()
            .filter(|&n| expression_kind(self.tree.node(n).unwrap().kind()))
            .collect()
    }
    fn one(&self, path: &AstPath, nodes: Vec<NodeId>) -> Result<NodeId, MappingError> {
        if nodes.len() == 1 {
            Ok(nodes[0])
        } else {
            Err(fail(path, "expected one grammar child"))
        }
    }
    fn insert(
        &mut self,
        path: &AstPath,
        node: NodeId,
        provenance: OriginKind,
        span: Option<Span>,
    ) -> Result<(), MappingError> {
        let kind = path
            .resolve_program(self.program)
            .ok_or_else(|| fail(path, "invalid AST origin path"))?
            .kind();
        let syntax = self.tree.node(node).unwrap();
        let span = span.unwrap_or(syntax.span());
        if span.start.byte < syntax.span().start.byte
            || span.end.byte > syntax.span().end.byte
            || span.start.byte > span.end.byte
        {
            return Err(fail(path, "origin span escapes its grammar node"));
        }
        if self.map.index.contains_key(path) {
            return Err(fail(path, "duplicate AST origin"));
        }
        self.map.index.insert(path.clone(), self.map.entries.len());
        self.map.entries.push(AstOrigin {
            path: path.clone(),
            kind,
            syntax: node,
            span,
            provenance,
        });
        Ok(())
    }
    fn declaration(&mut self, path: AstPath, node: NodeId) -> Result<(), MappingError> {
        self.insert(&path, node, OriginKind::Source, None)?;
        match path
            .resolve_program(self.program)
            .ok_or_else(|| fail(&path, "invalid declaration"))?
        {
            AstNode::Function(f) => {
                let params = self.nodes(node, SyntaxKind::Parameters);
                let params = if params.is_empty() {
                    vec![]
                } else {
                    self.nodes(self.one(&path, params)?, SyntaxKind::Parameter)
                };
                if params.len() != f.params.len() {
                    return Err(fail(&path, "parameter count differs"));
                }
                for (i, (param, &n)) in f.params.iter().zip(&params).enumerate() {
                    let p = path.child(AstStep::Parameter(i));
                    self.insert(&p, n, OriginKind::Source, None)?;
                    if let Some(t) = &param.annotation {
                        self.ty(
                            p.child(AstStep::Annotation),
                            t,
                            self.one(&p, self.nodes(n, SyntaxKind::TypeExpression))?,
                        )?;
                    }
                }
                if let Some(annotation) = &f.result_annotation {
                    self.ty(
                        path.child(AstStep::FunctionResult),
                        annotation,
                        self.one(&path, self.nodes(node, SyntaxKind::TypeExpression))?,
                    )?;
                }
                self.expr(
                    path.child(AstStep::FunctionBody),
                    &f.body,
                    self.one(&path, self.expressions(node))?,
                )?;
            }
            AstNode::TypeDeclaration(t) => {
                if t.record_fields.is_some() {
                    if t.variants.len() != 1 {
                        return Err(fail(&path, "record variant count differs"));
                    }
                    let p = path.child(AstStep::Variant(0));
                    self.insert(&p, node, OriginKind::RecordVariant, None)?;
                    let fields = self.nodes(node, SyntaxKind::RecordField);
                    if fields.len() != t.variants[0].fields.len() {
                        return Err(fail(&path, "record field count differs"));
                    }
                    for (i, (ty, n)) in t.variants[0].fields.iter().zip(fields).enumerate() {
                        self.ty(
                            p.child(AstStep::FieldType(i)),
                            ty,
                            self.one(&p, self.nodes(n, SyntaxKind::TypeExpression))?,
                        )?;
                    }
                } else {
                    let variants = self.nodes(node, SyntaxKind::Variant);
                    if variants.len() != t.variants.len() {
                        return Err(fail(&path, "variant count differs"));
                    }
                    for (i, (variant, n)) in t.variants.iter().zip(variants).enumerate() {
                        let p = path.child(AstStep::Variant(i));
                        self.insert(&p, n, OriginKind::Source, None)?;
                        let fields = self.nodes(n, SyntaxKind::TypeExpression);
                        if fields.len() != variant.fields.len() {
                            return Err(fail(&p, "variant field count differs"));
                        }
                        for (j, (ty, n)) in variant.fields.iter().zip(fields).enumerate() {
                            self.ty(p.child(AstStep::FieldType(j)), ty, n)?;
                        }
                    }
                }
            }
            AstNode::ExternFunction(f) => {
                let parameters = self.one(&path, self.nodes(node, SyntaxKind::Parameters))?;
                let params = self.nodes(parameters, SyntaxKind::Parameter);
                if params.len() != f.params.len() {
                    return Err(fail(&path, "foreign parameter count differs"));
                }
                for (i, n) in params.into_iter().enumerate() {
                    let p = path.child(AstStep::ForeignParameter(i));
                    self.insert(&p, n, OriginKind::Source, None)?;
                    self.insert(
                        &p.child(AstStep::ForeignParameterType),
                        self.one(&p, self.nodes(n, SyntaxKind::ForeignType))?,
                        OriginKind::Source,
                        None,
                    )?;
                }
                self.insert(
                    &path.child(AstStep::ForeignResult),
                    self.one(&path, self.nodes(node, SyntaxKind::ForeignType))?,
                    OriginKind::Source,
                    None,
                )?;
            }
            AstNode::Constant(_) => self.insert(
                &path.child(AstStep::ConstantType),
                self.one(&path, self.nodes(node, SyntaxKind::ForeignType))?,
                OriginKind::Source,
                None,
            )?,
            _ => return Err(fail(&path, "expected declaration AST")),
        }
        Ok(())
    }
    fn ty(&mut self, path: AstPath, ty: &TypeExpr, node: NodeId) -> Result<(), MappingError> {
        self.insert(&path, node, OriginKind::Source, None)?;
        let args = self.nodes(node, SyntaxKind::TypeExpression);
        if args.len() != ty.args.len() {
            return Err(fail(&path, "type argument count differs"));
        }
        for (i, (arg, n)) in ty.args.iter().zip(args).enumerate() {
            self.ty(path.child(AstStep::TypeArgument(i)), arg, n)?;
        }
        Ok(())
    }
    fn first_or_last_token(&self, node: NodeId, last: bool) -> Option<&crate::source::Lexeme<'_>> {
        let mut work = vec![SyntaxElement::Node(node)];
        while let Some(e) = work.pop() {
            match e {
                SyntaxElement::Node(n) => {
                    let children = self.tree.node(n).unwrap().children();
                    if last {
                        work.extend(children.iter().copied())
                    } else {
                        work.extend(children.iter().rev().copied())
                    }
                }
                SyntaxElement::Lexeme(i) => {
                    let l = &self.tree.source().lexemes()[i];
                    if matches!(l.kind(), LexemeKind::Token(_)) {
                        return Some(l);
                    }
                }
            }
        }
        None
    }
    fn pattern(
        &mut self,
        path: AstPath,
        pattern: &Pattern,
        node: NodeId,
    ) -> Result<(), MappingError> {
        let fields = self.nodes(node, SyntaxKind::Pattern);
        if matches!(
            self.first_or_last_token(node, false).map(|l| l.kind()),
            Some(LexemeKind::Token(crate::lexer::Kind::LParen))
        ) && !fields.is_empty()
        {
            let child = self.one(&path, fields)?;
            return self.pattern(path, pattern, child);
        }
        self.insert(&path, node, OriginKind::Source, None)?;
        if let Pattern::Constructor(_, args) = pattern {
            if args.len() != fields.len() {
                return Err(fail(&path, "pattern field count differs"));
            }
            for (i, (arg, n)) in args.iter().zip(fields).enumerate() {
                self.pattern(path.child(AstStep::PatternField(i)), arg, n)?;
            }
        } else if !fields.is_empty() {
            return Err(fail(&path, "unexpected nested pattern"));
        }
        Ok(())
    }
    fn expr(&mut self, path: AstPath, expr: &Expr, node: NodeId) -> Result<(), MappingError> {
        let kind = self.tree.node(node).unwrap().kind();
        let children = self.expressions(node);
        if matches!(
            kind,
            SyntaxKind::Expression | SyntaxKind::ParenthesizedExpression
        ) {
            let child = self.one(&path, children)?;
            return self.expr(path, expr, child);
        }
        if kind == SyntaxKind::Block {
            let parts = self
                .children(node)
                .into_iter()
                .filter(|&n| {
                    let k = self.tree.node(n).unwrap().kind();
                    expression_kind(k)
                        || matches!(
                            k,
                            SyntaxKind::LocalBinding | SyntaxKind::ExpressionStatement
                        )
                })
                .collect::<Vec<_>>();
            return self.block(path, expr, node, &parts);
        }
        self.insert(
            &path,
            node,
            match (kind, expr) {
                (SyntaxKind::UnaryExpression, Expr::Binary(BinaryOp::Sub, ..)) => {
                    OriginKind::UnaryNegation
                }
                (SyntaxKind::UnaryExpression, Expr::Binary(BinaryOp::Eq, ..)) => {
                    OriginKind::BooleanNegation
                }
                _ => OriginKind::Source,
            },
            None,
        )?;
        match (kind, expr) {
            (
                SyntaxKind::LiteralExpression,
                Expr::Int(_) | Expr::Bool(_) | Expr::Str(_) | Expr::CStr(_),
            )
            | (SyntaxKind::UnitExpression, Expr::Unit)
            | (SyntaxKind::NameExpression, Expr::Var(_))
            | (SyntaxKind::MoveExpression, Expr::Move(_))
            | (SyntaxKind::FieldExpression, Expr::Field(..)) => {}
            (SyntaxKind::BinaryExpression, Expr::Binary(_, a, b)) => {
                if children.len() != 2 {
                    return Err(fail(&path, "binary child count differs"));
                }
                self.expr(path.child(AstStep::BinaryLeft), a, children[0])?;
                self.expr(path.child(AstStep::BinaryRight), b, children[1])?;
            }
            (SyntaxKind::UnaryExpression, Expr::Int(n))
                if *n == i32::MIN && children.is_empty() => {}
            (SyntaxKind::UnaryExpression, Expr::Binary(BinaryOp::Eq, a, b))
                if **b == Expr::Bool(false) =>
            {
                self.expr(
                    path.child(AstStep::BinaryLeft),
                    a,
                    self.one(&path, children)?,
                )?;
                let span = self.first_or_last_token(node, false).unwrap().span();
                self.insert(
                    &path.child(AstStep::BinaryRight),
                    node,
                    OriginKind::BooleanNegation,
                    Some(span),
                )?;
            }
            (SyntaxKind::UnaryExpression, Expr::Binary(BinaryOp::Sub, a, b))
                if **a == Expr::Int(0) =>
            {
                let span = self.first_or_last_token(node, false).unwrap().span();
                self.insert(
                    &path.child(AstStep::BinaryLeft),
                    node,
                    OriginKind::UnaryNegation,
                    Some(span),
                )?;
                self.expr(
                    path.child(AstStep::BinaryRight),
                    b,
                    self.one(&path, children)?,
                )?;
            }
            (SyntaxKind::AssignmentExpression, Expr::Assign(_, v) | Expr::FieldAssign(_, _, v)) => {
                if children.len() != 2 {
                    return Err(fail(&path, "assignment child count differs"));
                }
                self.expr(path.child(AstStep::AssignValue), v, children[1])?;
            }
            (SyntaxKind::IfExpression, Expr::If(c, a, b)) => {
                if children.len() == 2 && **b == Expr::Unit {
                    self.expr(path.child(AstStep::Condition), c, children[0])?;
                    self.expr(path.child(AstStep::Then), a, children[1])?;
                    let end = self.tree.node(node).unwrap().span().end;
                    self.insert(
                        &path.child(AstStep::Else),
                        node,
                        OriginKind::ImplicitUnit,
                        Some(Span { start: end, end }),
                    )?;
                } else if children.len() == 3 {
                    for (step, e, n) in [
                        (AstStep::Condition, c, children[0]),
                        (AstStep::Then, a, children[1]),
                        (AstStep::Else, b, children[2]),
                    ] {
                        self.expr(path.child(step), e, n)?;
                    }
                } else {
                    return Err(fail(&path, "if child count differs"));
                }
            }
            (SyntaxKind::WhileExpression, Expr::While(c, b)) => {
                if children.len() != 2 {
                    return Err(fail(&path, "while child count differs"));
                }
                self.expr(path.child(AstStep::Condition), c, children[0])?;
                self.expr(path.child(AstStep::WhileBody), b, children[1])?;
            }
            (SyntaxKind::LetExpression, Expr::Let(_, a, b) | Expr::LetMut(_, a, b)) => {
                if children.len() != 2 {
                    return Err(fail(&path, "let child count differs"));
                }
                self.expr(path.child(AstStep::LetValue), a, children[0])?;
                self.expr(path.child(AstStep::LetBody), b, children[1])?;
            }
            (SyntaxKind::CallExpression, Expr::Call(_, args)) => {
                let arguments = self.nodes(node, SyntaxKind::Argument);
                if arguments.len() != args.len() {
                    return Err(fail(&path, "call argument count differs"));
                }
                for (i, (arg, n)) in args.iter().zip(arguments).enumerate() {
                    let p = path.child(AstStep::Argument(i));
                    if matches!(arg, Expr::Inout(_) | Expr::InoutField(..)) {
                        self.insert(&p, n, OriginKind::Source, None)?;
                    } else {
                        self.expr(p, arg, self.one(&path, self.expressions(n))?)?;
                    }
                }
            }
            (SyntaxKind::RecordExpression, Expr::Record(_, fields)) => {
                let nodes = self.nodes(node, SyntaxKind::RecordField);
                if nodes.len() != fields.len() {
                    return Err(fail(&path, "record construction field count differs"));
                }
                for (i, ((_, value), n)) in fields.iter().zip(nodes).enumerate() {
                    self.expr(
                        path.child(AstStep::RecordField(i)),
                        value,
                        self.one(&path, self.expressions(n))?,
                    )?;
                }
            }
            (SyntaxKind::MatchExpression, Expr::Match(value, arms)) => {
                self.expr(
                    path.child(AstStep::MatchValue),
                    value,
                    self.one(&path, children)?,
                )?;
                let nodes = self.nodes(node, SyntaxKind::MatchArm);
                if nodes.len() != arms.len() {
                    return Err(fail(&path, "match arm count differs"));
                }
                for (i, (arm, n)) in arms.iter().zip(nodes).enumerate() {
                    let p = path.child(AstStep::Arm(i));
                    self.insert(&p, n, OriginKind::Source, None)?;
                    self.pattern(
                        p.child(AstStep::ArmPattern),
                        &arm.pattern,
                        self.one(&p, self.nodes(n, SyntaxKind::Pattern))?,
                    )?;
                    self.expr(
                        p.child(AstStep::ArmBody),
                        &arm.body,
                        self.one(&p, self.expressions(n))?,
                    )?;
                }
            }
            _ => return Err(fail(&path, "expression grammar and AST differ")),
        }
        Ok(())
    }
    fn block(
        &mut self,
        path: AstPath,
        expr: &Expr,
        block: NodeId,
        parts: &[NodeId],
    ) -> Result<(), MappingError> {
        let Some((&first, rest)) = parts.split_first() else {
            if *expr != Expr::Unit {
                return Err(fail(&path, "block fallthrough is not unit"));
            }
            let end = self.first_or_last_token(block, true).unwrap().span().start;
            return self.insert(
                &path,
                block,
                OriginKind::ImplicitUnit,
                Some(Span { start: end, end }),
            );
        };
        match self.tree.node(first).unwrap().kind() {
            SyntaxKind::LocalBinding => {
                let (Expr::Let(_, value, body) | Expr::LetMut(_, value, body)) = expr else {
                    return Err(fail(&path, "block binding AST differs"));
                };
                self.insert(&path, first, OriginKind::BlockBinding, None)?;
                self.expr(
                    path.child(AstStep::LetValue),
                    value,
                    self.one(&path, self.expressions(first))?,
                )?;
                self.block(path.child(AstStep::LetBody), body, block, rest)
            }
            SyntaxKind::ExpressionStatement => {
                let Expr::Let(name, statement, body) = expr else {
                    return Err(fail(&path, "block sequence AST differs"));
                };
                let Expr::Let(discard, value, unit) = statement.as_ref() else {
                    return Err(fail(&path, "statement discard AST differs"));
                };
                if !name.starts_with("$block")
                    || !discard.starts_with("$discard")
                    || **unit != Expr::Unit
                {
                    return Err(fail(&path, "statement synthesis differs"));
                }
                self.insert(&path, first, OriginKind::BlockSequence, None)?;
                let p = path.child(AstStep::LetValue);
                self.insert(&p, first, OriginKind::StatementDiscard, None)?;
                self.expr(
                    p.child(AstStep::LetValue),
                    value,
                    self.one(&p, self.expressions(first))?,
                )?;
                self.insert(
                    &p.child(AstStep::LetBody),
                    first,
                    OriginKind::ImplicitUnit,
                    Some(self.first_or_last_token(first, true).unwrap().span()),
                )?;
                self.block(path.child(AstStep::LetBody), body, block, rest)
            }
            _ => {
                if !rest.is_empty() {
                    return Err(fail(&path, "nonfinal block tail"));
                }
                self.expr(path, expr, first)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_shape_failures_are_ordered_and_do_not_change_strict_parsing() {
        let source = "fn first(x){x} fn second(y){y}";
        let tree = SyntaxTree::parse(source);
        let original = crate::parser::parse_module(source).unwrap();
        let mut changed = original.program.clone();
        changed.functions[0].params.clear();
        changed.functions[1].params.clear();
        let attempt = || {
            let mut mapper = Mapper {
                tree: &tree,
                program: &changed,
                map: AstSourceMap {
                    entries: Vec::new(),
                    index: BTreeMap::new(),
                },
            };
            for declaration in tree.declarations() {
                mapper.declaration(AstPath::root(declaration.declaration), declaration.node)?;
            }
            Ok::<_, MappingError>(())
        };
        let error = attempt().unwrap_err();
        assert_eq!(error, attempt().unwrap_err());
        assert_eq!(error.path, Some(AstPath::root(AstDeclaration::Function(0))));
        assert_eq!(error.message, "parameter count differs");
        assert_eq!(crate::parser::parse_module(source).unwrap(), original);
        assert_eq!(tree.parsed_module(), Some(&original));
        assert!(tree.ast_source_map().is_ok());
    }
}
