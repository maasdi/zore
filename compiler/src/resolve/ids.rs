//! Strong semantic IDs assigned during resolution.

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ConstId(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FunctionId(pub u32);

/// Index of a field within its struct's declaration order.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FieldId(pub u32);

/// Index into the owning function's `locals`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LocalId(pub u32);
