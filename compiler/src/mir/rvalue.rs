use super::operand::Operand;
use crate::ast::{BinaryOp, UnaryOp};
use crate::types::{StructId, TypeId};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AggregateKind {
    Struct(StructId),
    /// Element type; the count is the operand list's length.
    Array(TypeId),
}

#[derive(Debug)]
pub enum Rvalue {
    Use(Operand),
    Zero,
    /// Checked arithmetic; `&&` and `||` are lowered to branches.
    Binary(BinaryOp, Operand, Operand),
    Unary(UnaryOp, Operand),
    /// Checked numeric conversion to the given type.
    Convert(Operand, TypeId),
    Error(Operand),
    /// Checks `0 <= index < length` per the operand's own signedness, panics
    /// otherwise, and evaluates to the index widened to `int64`.
    BoundsCheck(Operand, u32),
    /// Fields or elements in declaration/evaluation order.
    Aggregate(AggregateKind, Vec<Operand>),
}
