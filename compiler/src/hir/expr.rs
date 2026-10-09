use crate::ast::{BinaryOp, UnaryOp};
use crate::resolve::{FieldId, FunctionId, LocalId};
use crate::source::Span;
use crate::types::{StructId, TypeId};

#[derive(Clone, Debug, PartialEq)]
pub enum Projection {
    Field(FieldId),
    /// Not yet bounds-checked.
    Index(Box<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub root: LocalId,
    pub projections: Vec<Projection>,
    pub ty: TypeId,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    /// One type per result.
    pub types: Vec<TypeId>,
    pub span: Span,
}

impl Expr {
    pub fn ty(&self) -> TypeId {
        debug_assert_eq!(self.types.len(), 1);
        self.types[0]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Const {
    Bool(bool),
    Int(i128),
    /// Exactly representable in the type; never negative zero.
    Float(f64),
    Rune(char),
    String(String),
    Nil,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    Const(Const),
    Local(LocalId),
    Field {
        base: Box<Expr>,
        field: FieldId,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    Slice {
        base: Box<Expr>,
        low: Option<Box<Expr>>,
        high: Option<Box<Expr>>,
        mutable: bool,
    },
    Call {
        function: FunctionId,
        args: Vec<Expr>,
    },
    /// The callee is evaluated first and used exclusively; a call-once callee is consumed.
    CallValue {
        callee: Box<Expr>,
        args: Vec<Expr>,
        once: bool,
    },
    /// Each capture is `(local, exclusive)` in the enclosing function; an
    /// owning closure captures their values instead of borrowing them.
    Closure {
        function: FunctionId,
        captures: Vec<(LocalId, bool)>,
        owning: bool,
    },
    /// Runs the call to the closure's result on a new task; the arguments become the closure's captures.
    Spawn {
        thunk: FunctionId,
        /// The thunk's type, `func() R1, ..., Rn`.
        closure_ty: TypeId,
        /// The first argument is a function value that the task calls with the others.
        callable: bool,
        args: Vec<Expr>,
    },
    /// Consumes the task handle and gives its results, raising its panic if it had one.
    TaskWait(Box<Expr>),
    MakeChannel {
        element: TypeId,
        capacity: Option<Box<Expr>>,
    },
    /// Moves the value into the channel, waiting for room.
    ChannelSend {
        channel: Box<Expr>,
        value: Box<Expr>,
    },
    /// The value, then whether one arrived.
    ChannelReceive(Box<Expr>),
    ChannelClose(Box<Expr>),
    /// A mutex that owns the value.
    MakeMutex(Box<Expr>),
    /// Calls the callback with a mutable borrow of the guarded value while holding the lock.
    MutexWithLock {
        mutex: Box<Expr>,
        callback: Box<Expr>,
    },
    MutexIsPoisoned(Box<Expr>),
    Println(Box<Expr>),
    Drop(Box<Expr>),
    Clone(Box<Expr>),
    Convert(Box<Expr>),
    Error(Box<Expr>),
    Try(Box<Expr>),
    /// In written order, which is evaluation order.
    StructLit {
        strukt: StructId,
        fields: Vec<(FieldId, Expr)>,
    },
    ArrayLit {
        element: TypeId,
        elements: Vec<Expr>,
    },
    MapLit {
        key: TypeId,
        value: TypeId,
        entries: Vec<(Expr, Expr)>,
    },
    /// Presence, then a copy of the value or its zero.
    MapLookup {
        map: Box<Expr>,
        key: Box<Expr>,
    },
    /// Presence, then the detached value or its zero.
    MapRemove {
        map: Box<Expr>,
        key: Box<Expr>,
    },
    /// Elements of an array or slice, or entries of a map.
    Len(Box<Expr>),
    ArrayPush {
        array: Box<Expr>,
        value: Box<Expr>,
    },
    /// Presence, then the detached last element or its zero.
    ArrayPop(Box<Expr>),
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
}
