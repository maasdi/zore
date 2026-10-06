//! What a resolved name refers to, and the declarations resolution records.

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
    Println,
    Drop,
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
    /// A closure's borrow of a local of its enclosing function (§16.3).
    Capture(LocalId),
}

/// A closure literal: its own function ID, where it appears, and what it captures.
pub struct ClosureDecl<'a> {
    pub id: FunctionId,
    pub closure: &'a ast::Closure,
    pub span: Span,
    /// The function whose body contains the literal.
    pub parent: FunctionId,
    /// `(outer, local)`: a local of `parent` and the capture standing for it.
    pub captures: Vec<(LocalId, LocalId)>,
}

pub(super) fn predeclared(name: &str) -> Option<Res> {
    if let Some(ty) = TypeStore::primitive(name) {
        return Some(Res::Primitive(ty));
    }
    match name {
        "println" => Some(Res::Println),
        "drop" => Some(Res::Drop),
        "Array" | "Task" | "clone" => Some(Res::Unsupported),
        _ => None,
    }
}

pub(super) fn unsupported_predeclared(name: &str) -> &'static str {
    match name {
        "Task" => "`Task` is",
        _ => "`clone` is",
    }
}
