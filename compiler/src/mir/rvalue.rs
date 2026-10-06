use super::operand::{Operand, Place};
use crate::ast::{BinaryOp, UnaryOp};
use crate::resolve::FunctionId;
use crate::types::{StructId, TypeId};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AggregateKind {
    Struct(StructId),
    /// The count is the operand list's length.
    Array(TypeId),
    DynArray(TypeId),
}

#[derive(Debug)]
pub enum Rvalue {
    Use(Operand),
    Zero,
    /// `&&` and `||` are lowered to branches.
    Binary(BinaryOp, Operand, Operand),
    Unary(UnaryOp, Operand),
    Convert(Operand, TypeId),
    Error(Operand),
    /// Panics unless `0 <= index < length`, then evaluates to the index as `int64`.
    BoundsCheck(Operand, Operand),
    /// As an `int64`: elements of an array or slice, or entries of a map.
    Length(Place),
    /// The address of the place, stored into a by-reference local.
    Ref(Place),
    /// A copy of the key of the map entry at a position below its length.
    MapKeyAt(Place, Operand),
    /// The address of the value of the map entry at a position below its length.
    MapValueRef(Place, Operand),
    /// Panics unless `0 <= low <= high <= length`.
    Slice {
        place: Place,
        low: Option<Operand>,
        high: Option<Operand>,
        mutable: bool,
    },
    /// In declaration/evaluation order.
    Aggregate(AggregateKind, Vec<Operand>),
    /// Borrows each place, exclusively when marked, in capture order.
    Closure {
        function: FunctionId,
        captures: Vec<(Place, bool)>,
    },
}
