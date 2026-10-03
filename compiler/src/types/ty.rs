use super::type_id::{StructId, TypeId};

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

/// IEEE 754 binary32 or binary64.
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
    /// A borrowed view, `[]T` or `mut []T`; it never owns its elements.
    Slice {
        element: TypeId,
        mutable: bool,
    },
    /// The owned dynamic array `Array<T>`; always Move (§10.5).
    DynArray {
        element: TypeId,
    },
}
