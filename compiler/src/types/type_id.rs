#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TypeId(pub(super) u32);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct StructId(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FuncTypeId(pub(super) u32);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TaskTypeId(pub(super) u32);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct InterfaceId(pub u32);
