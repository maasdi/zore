//! Interned semantic types.

pub mod bignum;
pub mod checker;
pub mod constant;

use std::collections::HashMap;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TypeId(u32);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct StructId(pub u32);

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
    Array { element: TypeId, size: u32 },
}

/// Type identities; aliases such as `int` and `int64` share one identity.
#[derive(Debug)]
pub struct TypeStore {
    kinds: Vec<TypeKind>,
    struct_names: Vec<String>,
    struct_types: Vec<TypeId>,
    array_types: HashMap<(TypeId, u32), TypeId>,
}

impl Default for TypeStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeStore {
    pub const BOOL: TypeId = TypeId(0);
    pub const RUNE: TypeId = TypeId(1);
    pub const STRING: TypeId = TypeId(2);
    pub const INT8: TypeId = TypeId(3);
    pub const INT16: TypeId = TypeId(4);
    pub const INT32: TypeId = TypeId(5);
    pub const INT64: TypeId = TypeId(6);
    pub const UINT8: TypeId = TypeId(7);
    pub const UINT16: TypeId = TypeId(8);
    pub const UINT32: TypeId = TypeId(9);
    pub const UINT64: TypeId = TypeId(10);
    pub const FLOAT32: TypeId = TypeId(11);
    pub const FLOAT64: TypeId = TypeId(12);
    pub const ERROR: TypeId = TypeId(13);
    /// Default type of untyped integer constants.
    pub const INT: TypeId = Self::INT64;

    pub fn new() -> Self {
        let mut kinds = vec![TypeKind::Bool, TypeKind::Rune, TypeKind::String];
        for signed in [true, false] {
            for bits in [8, 16, 32, 64] {
                kinds.push(TypeKind::Int(IntType { bits, signed }));
            }
        }
        kinds.push(TypeKind::Float(FloatType { bits: 32 }));
        kinds.push(TypeKind::Float(FloatType { bits: 64 }));
        kinds.push(TypeKind::Error);
        Self {
            kinds,
            struct_names: Vec::new(),
            struct_types: Vec::new(),
            array_types: HashMap::new(),
        }
    }

    pub fn add_struct(&mut self, name: &str) -> (StructId, TypeId) {
        let id = StructId(self.struct_names.len() as u32);
        self.struct_names.push(name.to_owned());
        let ty = TypeId(self.kinds.len() as u32);
        self.kinds.push(TypeKind::Struct(id));
        self.struct_types.push(ty);
        (id, ty)
    }

    pub fn struct_type(&self, id: StructId) -> TypeId {
        self.struct_types[id.0 as usize]
    }

    /// Interns `[element; size]`, so the same shape always shares one `TypeId`.
    pub fn array_type(&mut self, element: TypeId, size: u32) -> TypeId {
        if let Some(&ty) = self.array_types.get(&(element, size)) {
            return ty;
        }
        let ty = TypeId(self.kinds.len() as u32);
        self.kinds.push(TypeKind::Array { element, size });
        self.array_types.insert((element, size), ty);
        ty
    }

    pub fn kind(&self, ty: TypeId) -> TypeKind {
        self.kinds[ty.0 as usize]
    }

    pub fn int(&self, ty: TypeId) -> Option<IntType> {
        match self.kind(ty) {
            TypeKind::Int(int) => Some(int),
            _ => None,
        }
    }

    pub fn float(&self, ty: TypeId) -> Option<FloatType> {
        match self.kind(ty) {
            TypeKind::Float(float) => Some(float),
            _ => None,
        }
    }

    pub fn is_numeric(&self, ty: TypeId) -> bool {
        matches!(self.kind(ty), TypeKind::Int(_) | TypeKind::Float(_))
    }

    pub fn struct_id(&self, ty: TypeId) -> Option<StructId> {
        match self.kind(ty) {
            TypeKind::Struct(id) => Some(id),
            _ => None,
        }
    }

    pub fn display(&self, ty: TypeId) -> TypeName<'_> {
        TypeName { store: self, ty }
    }

    /// Looks up a primitive type name.
    pub fn primitive(name: &str) -> Option<TypeId> {
        Some(match name {
            "bool" => Self::BOOL,
            "rune" => Self::RUNE,
            "string" => Self::STRING,
            "error" => Self::ERROR,
            "int8" => Self::INT8,
            "int16" => Self::INT16,
            "int32" => Self::INT32,
            "int64" | "int" => Self::INT64,
            "uint8" | "byte" => Self::UINT8,
            "uint16" => Self::UINT16,
            "uint32" => Self::UINT32,
            "uint64" | "uint" => Self::UINT64,
            "float32" => Self::FLOAT32,
            "float64" => Self::FLOAT64,
            _ => return None,
        })
    }
}

pub struct TypeName<'a> {
    store: &'a TypeStore,
    ty: TypeId,
}

impl fmt::Display for TypeName<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.store.kind(self.ty) {
            TypeKind::Bool => f.write_str("bool"),
            TypeKind::Rune => f.write_str("rune"),
            TypeKind::String => f.write_str("string"),
            TypeKind::Error => f.write_str("error"),
            TypeKind::Int(IntType { bits, signed }) => {
                write!(f, "{}int{bits}", if signed { "" } else { "u" })
            }
            TypeKind::Float(FloatType { bits }) => write!(f, "float{bits}"),
            TypeKind::Struct(id) => f.write_str(&self.store.struct_names[id.0 as usize]),
            TypeKind::Array { element, size } => {
                write!(f, "[{}; {size}]", self.store.display(element))
            }
        }
    }
}
