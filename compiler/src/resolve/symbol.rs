use super::ids::{ConstId, FunctionId, GlobalId, LocalId};
use crate::ast::{self, ParamMode};
use crate::source::Span;
use crate::types::{Constraint, StructId, TypeId, TypeStore};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Res {
    Local(LocalId),
    Const(ConstId),
    Function(FunctionId),
    Struct(StructId),
    /// A package-level `let`.
    Global(GlobalId),
    /// A declared type built on a predeclared type (`type Duration int`).
    Named(TypeId),
    Interface(TypeId),
    TypeParam(TypeId),
    /// `any`, `copyable`, `comparable`, or `ordered`; valid only as a constraint.
    Constraint(Constraint),
    Primitive(TypeId),
    /// An import's name; valid only as the qualifier of `package.Name`.
    Package(usize),
    Println,
    Panic,
    Drop,
    Clone,
    /// `mutex(value)`.
    NewMutex,
    /// A predeclared name whose feature is not supported yet; uses are diagnosed.
    Unsupported,
}

pub struct GlobalDecl<'a> {
    pub name: &'a ast::Name,
    pub ty: Option<&'a ast::Type>,
    /// Returns the initial value.
    pub function: FunctionId,
    pub package: usize,
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
        "panic" => Some(Res::Panic),
        "drop" => Some(Res::Drop),
        "clone" => Some(Res::Clone),
        "mutex" => Some(Res::NewMutex),
        "any" => Some(Res::Constraint(Constraint::Any)),
        "copyable" => Some(Res::Constraint(Constraint::Copyable)),
        "comparable" => Some(Res::Constraint(Constraint::Comparable)),
        "ordered" => Some(Res::Constraint(Constraint::Ordered)),
        "Array" | "Task" | "Mutex" => Some(Res::Unsupported),
        _ => None,
    }
}
