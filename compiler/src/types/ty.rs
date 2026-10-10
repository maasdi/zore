use super::type_id::{FuncTypeId, InterfaceId, StructId, TaskTypeId, TypeId};
use crate::ast::ParamMode;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct IntType {
    pub bits: u8,
    pub signed: bool,
}

impl IntType {
    pub fn min(self) -> i128 {
        if self.signed {
            -(1i128 << (self.bits - 1))
        } else {
            0
        }
    }

    pub fn max(self) -> i128 {
        if self.signed {
            (1i128 << (self.bits - 1)) - 1
        } else {
            (1i128 << self.bits) - 1
        }
    }

    pub fn contains(self, value: i128) -> bool {
        (self.min()..=self.max()).contains(&value)
    }

    /// Reinterprets the low `bits` of `value` in this type.
    pub fn wrap(self, value: i128) -> i128 {
        let mask = (1i128 << self.bits) - 1;
        let low = value & mask;
        if self.signed && low > self.max() {
            low - (1i128 << self.bits)
        } else {
            low
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FloatType {
    pub bits: u8,
}

impl FloatType {
    pub fn format(self) -> crate::types::bignum::FloatFormat {
        if self.bits == 32 {
            crate::types::bignum::BINARY32
        } else {
            crate::types::bignum::BINARY64
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TypeKind {
    Bool,
    Int(IntType),
    Float(FloatType),
    Rune,
    String,
    Error,
    Struct(StructId),
    Array {
        element: TypeId,
        size: u32,
    },
    /// Borrowed; never owns its elements.
    Slice {
        element: TypeId,
        mutable: bool,
    },
    /// Always Move.
    DynArray {
        element: TypeId,
    },
    /// Always Move.
    Map {
        key: TypeId,
        value: TypeId,
    },
    Func(FuncTypeId),
    /// Always Move.
    Task(TaskTypeId),
    /// A Copy handle to one shared queue.
    Channel {
        element: TypeId,
    },
    /// A Copy handle to one shared lock and the value it guards.
    Mutex {
        element: TypeId,
    },
    /// Owns the value inside; always Move.
    Interface(InterfaceId),
    /// A borrowed interface parameter: a view of the argument, like a slice.
    InterfaceView {
        interface: InterfaceId,
        mutable: bool,
    },
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct FuncSignature {
    pub is_async: bool,
    pub params: Vec<(ParamMode, TypeId)>,
    pub results: Vec<TypeId>,
}

/// One entry of an interface: a method a satisfying type must have.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct InterfaceMethod {
    pub name: String,
    pub receiver: ParamMode,
    pub is_async: bool,
    pub params: Vec<(ParamMode, TypeId)>,
    pub results: Vec<TypeId>,
}
