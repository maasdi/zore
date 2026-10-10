use super::operand::{Operand, Place};
use crate::ast::{BinaryOp, UnaryOp};
use crate::resolve::{FunctionId, GlobalId};
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
    /// A copy of a package-level `let`, with its own share of any text it holds.
    Global(GlobalId),
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
    /// A `string` between the bounds, which must fall on character boundaries;
    /// panics otherwise or unless `0 <= low <= high <= length`.
    StringSlice {
        source: Operand,
        low: Option<Operand>,
        high: Option<Operand>,
    },
    /// The `rune` that starts at a byte position below the string's length.
    StringChar(Operand, Operand),
    /// The byte position after the character that starts at the given position.
    StringAdvance(Operand, Operand),
    /// Panics unless `0 <= low <= high <= length`.
    Slice {
        place: Place,
        low: Option<Operand>,
        high: Option<Operand>,
        mutable: bool,
    },
    /// In declaration/evaluation order.
    Aggregate(AggregateKind, Vec<Operand>),
    /// A borrowed interface value viewing the place, exclusively when mutable.
    InterfaceView {
        place: Place,
        mutable: bool,
    },
    /// Moves or copies the value into heap storage the owned interface value owns.
    InterfaceBox(Operand),
    /// Borrows each place, exclusively when marked, in capture order; an owning
    /// closure instead copies or moves each value into its own environment.
    /// Starts running the closure on a new task and gives the handle; consumes the closure.
    Spawn(Operand),
    Closure {
        function: FunctionId,
        captures: Vec<(Place, bool)>,
        owning: bool,
    },
}
