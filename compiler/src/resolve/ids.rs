#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ConstId(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FunctionId(pub u32);

/// Declaration order within its struct.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FieldId(pub u32);

/// Index into the owning function's `locals`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LocalId(pub u32);

/// A package-level `let`, numbered in initialization order.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GlobalId(pub u32);
