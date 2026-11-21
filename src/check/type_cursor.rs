//! Private borrowed view for consumers which must budget traversal before
//! expanding an inferred type. Never export a substituted tree just to inspect
//! its head: a tiny solver DAG may describe an exponentially large tree.
use super::{Checker, Type};

#[derive(Clone, Copy)]
pub(super) enum TypeCursor<'a> {
    Public(&'a Type),
    Private(usize),
}

pub(super) enum TypeHead<'a> {
    Variable(usize),
    Atom(&'a Type),
    Nominal(&'a str, TypeChildren<'a>),
    Callable(&'a [crate::ast::ParamMode], TypeChildren<'a>),
}

pub(super) enum TypeChildren<'a> {
    Public(&'a [Type]),
    Private(&'a [usize]),
    Callable {
        environment: &'a Type,
        params: &'a [Type],
        result: &'a Type,
    },
}
impl<'a> TypeChildren<'a> {
    pub(super) fn len(&self) -> usize {
        match self {
            Self::Public(children) => children.len(),
            Self::Private(children) => children.len(),
            Self::Callable { params, .. } => params.len() + 2,
        }
    }
    pub(super) fn get(&self, index: usize) -> Option<TypeCursor<'a>> {
        match self {
            Self::Public(children) => children.get(index).map(TypeCursor::Public),
            Self::Private(children) => children.get(index).copied().map(TypeCursor::Private),
            Self::Callable {
                environment,
                params,
                result,
            } => {
                let ty = if index == 0 {
                    Some(*environment)
                } else if index <= params.len() {
                    params.get(index - 1)
                } else if index == params.len() + 1 {
                    Some(*result)
                } else {
                    None
                };
                ty.map(TypeCursor::Public)
            }
        }
    }
}

impl Checker {
    pub(super) fn type_head<'a>(
        &'a self,
        cursor: TypeCursor<'a>,
        charge: impl FnMut() -> Result<(), String>,
    ) -> Result<TypeHead<'a>, String> {
        self.solver.type_head(cursor, charge)
    }
}
