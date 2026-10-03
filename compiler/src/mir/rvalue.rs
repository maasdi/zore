use super::operand::{Operand, Place};
use crate::ast::{BinaryOp, UnaryOp};
use crate::types::{StructId, TypeId};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AggregateKind {
    Struct(StructId),
    /// Element type; the count is the operand list's length.
    Array(TypeId),
    /// A newly allocated `Array<T>` of the element type, holding the operands.
    DynArray(TypeId),
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
    /// Checks `0 <= index < length` (an `int64` length) per the index's own
    /// signedness, panics otherwise, and evaluates to the index widened to `int64`.
    BoundsCheck(Operand, Operand),
    /// The length of the slice or dynamic array at the place, as an `int64`.
    Length(Place),
    /// A view of `place` (an array, dynamic array, or slice) from `low` (default zero) up to
    /// `high` (default its length); panics unless `0 <= low <= high <= length`.
    Slice {
        place: Place,
        low: Option<Operand>,
        high: Option<Operand>,
        mutable: bool,
    },
    /// Fields or elements in declaration/evaluation order.
    Aggregate(AggregateKind, Vec<Operand>),
}
