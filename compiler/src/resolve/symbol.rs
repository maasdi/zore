use super::ids::{ConstId, FunctionId, LocalId};
use crate::ast::{self, ParamMode};
use crate::source::Span;
use crate::types::{StructId, TypeId, TypeStore};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Res {
    Local(LocalId),
    Const(ConstId),
    Function(FunctionId),
    Struct(StructId),
    Primitive(TypeId),
    /// An import's name; valid only as the qualifier of `package.Name`.
    Package(usize),
    Println,
    Drop,
    Clone,
    /// `mutex(value)`.
    NewMutex,
    /// A predeclared name whose feature is not supported yet; uses are diagnosed.
    Unsupported,
}

pub struct ConstDecl<'a> {
    pub name: &'a ast::Name,
    pub ty: Option<&'a ast::Type>,
    pub value: &'a ast::Expr,
}

pub struct LocalDecl {
    pub name: String,
    pub span: Span,
    pub kind: LocalKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalKind {
    Param(ParamMode),
    Let,
    Var,
    Capture(LocalId),
    /// A collection loop's shared borrow of the current element.
    Item,
}

pub struct ClosureDecl<'a> {
    pub id: FunctionId,
    pub closure: &'a ast::Closure,
    pub span: Span,
    pub parent: FunctionId,
    /// `(outer, local)`: a local of `parent` and its capture.
    pub captures: Vec<(LocalId, LocalId)>,
}

pub(super) fn predeclared(name: &str) -> Option<Res> {
    if let Some(ty) = TypeStore::primitive(name) {
        return Some(Res::Primitive(ty));
    }
    match name {
        "println" => Some(Res::Println),
        "drop" => Some(Res::Drop),
        "clone" => Some(Res::Clone),
        "mutex" => Some(Res::NewMutex),
        "Array" | "Task" | "Mutex" => Some(Res::Unsupported),
        _ => None,
    }
}
