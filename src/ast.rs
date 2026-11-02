//! Syntax for the currently supported sequential expression subset.

#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub native: crate::c_import::Config,
    pub constants: Vec<ExternConst>,
    pub allocation_types: Option<AllocationTypes>,
    pub types: Vec<TypeDecl>,
    pub externs: Vec<ExternFunction>,
    pub functions: Vec<Function>,
}

/// Resolved declaration identities for the bundled allocation contract.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AllocationTypes {
    pub result_type: String,
    pub ok_constructor: String,
    pub err_constructor: String,
    pub error_type: String,
    pub invalid_length_constructor: String,
    pub size_overflow_constructor: String,
    pub out_of_memory_constructor: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExternFunction {
    pub name: String,
    pub params: Vec<ExternParam>,
    pub result: ExternType,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExternConst {
    pub name: String,
    pub ty: ExternType,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExternParam {
    pub name: String,
    pub ty: ExternType,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExternType {
    U8,
    U32,
    U64,
    F32,
    Pointer { pointee: String, is_const: bool },
    I32,
    Bool,
    Unit,
    CStr,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypeDecl {
    pub name: String,
    pub params: Vec<String>,
    pub variants: Vec<VariantDecl>,
    /// Field names for a record's single synthetic variant, in declaration order.
    pub record_fields: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VariantDecl {
    pub name: String,
    pub fields: Vec<TypeExpr>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypeExpr {
    pub name: String,
    pub args: Vec<TypeExpr>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Expr,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Pattern {
    Wildcard,
    Binding(String),
    Constructor(String, Vec<Pattern>),
    Bool(bool),
    Int(i32),
    Unit,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    pub name: String,
    pub params: Vec<Param>,
    pub body: Expr,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: String,
    pub mode: ParamMode,
    pub annotation: Option<TypeExpr>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamMode {
    Value,
    Read,
    Inout,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Int(i32),
    Str(String),
    CStr(String),
    Bool(bool),
    Unit,
    Var(String),
    Move(String),
    Inout(String),
    InoutField(String, Vec<String>),
    Record(String, Vec<(String, Expr)>),
    Field(String, Vec<String>),
    FieldAssign(String, Vec<String>, Box<Expr>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    Let(String, Box<Expr>, Box<Expr>),
    LetMut(String, Box<Expr>, Box<Expr>),
    Assign(String, Box<Expr>),
    Call(String, Vec<Expr>),
    Match(Box<Expr>, Vec<MatchArm>),
    While(Box<Expr>, Box<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}
