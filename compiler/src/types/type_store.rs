use std::collections::HashMap;
use std::fmt;

use super::ty::{FloatType, FuncSignature, IntType, InterfaceMethod, TypeKind};
use super::type_id::{FuncTypeId, InterfaceId, StructId, TaskTypeId, TypeId};
use crate::ast::ParamMode;

/// Aliases such as `int` and `int64` share one identity.
#[derive(Debug)]
pub struct TypeStore {
    kinds: Vec<TypeKind>,
    struct_names: Vec<String>,
    struct_types: Vec<TypeId>,
    array_types: HashMap<(TypeId, u32), TypeId>,
    slice_types: HashMap<(TypeId, bool), TypeId>,
    dyn_array_types: HashMap<TypeId, TypeId>,
    map_types: HashMap<(TypeId, TypeId), TypeId>,
    signatures: Vec<FuncSignature>,
    func_types: HashMap<FuncSignature, TypeId>,
    task_results: Vec<Vec<TypeId>>,
    task_types: HashMap<Vec<TypeId>, TypeId>,
    channel_types: HashMap<TypeId, TypeId>,
    mutex_types: HashMap<TypeId, TypeId>,
    /// A named type's display name and the predeclared type it is built on.
    named: HashMap<TypeId, (String, TypeId)>,
    interface_names: Vec<String>,
    interface_types: Vec<TypeId>,
    interface_methods: Vec<Vec<InterfaceMethod>>,
    interface_views: HashMap<(InterfaceId, bool), TypeId>,
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
            slice_types: HashMap::new(),
            dyn_array_types: HashMap::new(),
            map_types: HashMap::new(),
            signatures: Vec::new(),
            func_types: HashMap::new(),
            task_results: Vec::new(),
            task_types: HashMap::new(),
            channel_types: HashMap::new(),
            mutex_types: HashMap::new(),
            named: HashMap::new(),
            interface_names: Vec::new(),
            interface_types: Vec::new(),
            interface_methods: Vec::new(),
            interface_views: HashMap::new(),
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

    /// The entries are set by `set_interface_methods` once their types are resolved.
    pub fn add_interface(&mut self, name: &str) -> (InterfaceId, TypeId) {
        let id = InterfaceId(self.interface_names.len() as u32);
        self.interface_names.push(name.to_owned());
        self.interface_methods.push(Vec::new());
        let ty = TypeId(self.kinds.len() as u32);
        self.kinds.push(TypeKind::Interface(id));
        self.interface_types.push(ty);
        (id, ty)
    }

    pub fn set_interface_methods(&mut self, id: InterfaceId, methods: Vec<InterfaceMethod>) {
        self.interface_methods[id.0 as usize] = methods;
    }

    pub fn interface_methods(&self, id: InterfaceId) -> &[InterfaceMethod] {
        &self.interface_methods[id.0 as usize]
    }

    pub fn interface_type(&self, id: InterfaceId) -> TypeId {
        self.interface_types[id.0 as usize]
    }

    pub fn interface_view(&mut self, interface: InterfaceId, mutable: bool) -> TypeId {
        if let Some(&ty) = self.interface_views.get(&(interface, mutable)) {
            return ty;
        }
        let ty = TypeId(self.kinds.len() as u32);
        self.kinds
            .push(TypeKind::InterfaceView { interface, mutable });
        self.interface_views.insert((interface, mutable), ty);
        ty
    }

    pub fn interface_of(&self, ty: TypeId) -> Option<InterfaceId> {
        match self.kind(ty) {
            TypeKind::Interface(id) | TypeKind::InterfaceView { interface: id, .. } => Some(id),
            _ => None,
        }
    }

    /// The kind is set by `set_named_base` once the base type is resolved.
    pub fn add_named(&mut self, name: &str) -> TypeId {
        let ty = TypeId(self.kinds.len() as u32);
        self.kinds.push(TypeKind::Bool);
        self.named.insert(ty, (name.to_owned(), ty));
        ty
    }

    pub fn set_named_base(&mut self, ty: TypeId, base: TypeId) {
        self.kinds[ty.0 as usize] = self.kind(base);
        let builtin = self.base(base);
        if let Some(entry) = self.named.get_mut(&ty) {
            entry.1 = builtin;
        }
    }

    pub fn is_named(&self, ty: TypeId) -> bool {
        self.named.contains_key(&ty)
    }

    /// The predeclared type a named type is built on; any other type is its own base.
    pub fn base(&self, ty: TypeId) -> TypeId {
        self.named.get(&ty).map_or(ty, |&(_, base)| base)
    }

    pub fn struct_type(&self, id: StructId) -> TypeId {
        self.struct_types[id.0 as usize]
    }

    /// Interning gives each shape exactly one `TypeId`.
    pub fn array_type(&mut self, element: TypeId, size: u32) -> TypeId {
        if let Some(&ty) = self.array_types.get(&(element, size)) {
            return ty;
        }
        let ty = TypeId(self.kinds.len() as u32);
        self.kinds.push(TypeKind::Array { element, size });
        self.array_types.insert((element, size), ty);
        ty
    }

    pub fn slice_type(&mut self, element: TypeId, mutable: bool) -> TypeId {
        if let Some(&ty) = self.slice_types.get(&(element, mutable)) {
            return ty;
        }
        let ty = TypeId(self.kinds.len() as u32);
        self.kinds.push(TypeKind::Slice { element, mutable });
        self.slice_types.insert((element, mutable), ty);
        ty
    }

    pub fn dyn_array_type(&mut self, element: TypeId) -> TypeId {
        if let Some(&ty) = self.dyn_array_types.get(&element) {
            return ty;
        }
        let ty = TypeId(self.kinds.len() as u32);
        self.kinds.push(TypeKind::DynArray { element });
        self.dyn_array_types.insert(element, ty);
        ty
    }

    pub fn map_type(&mut self, key: TypeId, value: TypeId) -> TypeId {
        if let Some(&ty) = self.map_types.get(&(key, value)) {
            return ty;
        }
        let ty = TypeId(self.kinds.len() as u32);
        self.kinds.push(TypeKind::Map { key, value });
        self.map_types.insert((key, value), ty);
        ty
    }

    pub fn func_type(&mut self, signature: FuncSignature) -> TypeId {
        if let Some(&ty) = self.func_types.get(&signature) {
            return ty;
        }
        let id = FuncTypeId(self.signatures.len() as u32);
        self.signatures.push(signature.clone());
        let ty = TypeId(self.kinds.len() as u32);
        self.kinds.push(TypeKind::Func(id));
        self.func_types.insert(signature, ty);
        ty
    }

    pub fn channel_type(&mut self, element: TypeId) -> TypeId {
        if let Some(&ty) = self.channel_types.get(&element) {
            return ty;
        }
        let ty = TypeId(self.kinds.len() as u32);
        self.kinds.push(TypeKind::Channel { element });
        self.channel_types.insert(element, ty);
        ty
    }

    pub fn mutex_type(&mut self, element: TypeId) -> TypeId {
        if let Some(&ty) = self.mutex_types.get(&element) {
            return ty;
        }
        let ty = TypeId(self.kinds.len() as u32);
        self.kinds.push(TypeKind::Mutex { element });
        self.mutex_types.insert(element, ty);
        ty
    }

    /// `Task<R1, ..., Rn>` for a spawned call's result list.
    pub fn task_type(&mut self, results: Vec<TypeId>) -> TypeId {
        if let Some(&ty) = self.task_types.get(&results) {
            return ty;
        }
        let id = TaskTypeId(self.task_results.len() as u32);
        self.task_results.push(results.clone());
        let ty = TypeId(self.kinds.len() as u32);
        self.kinds.push(TypeKind::Task(id));
        self.task_types.insert(results, ty);
        ty
    }

    pub fn task_results(&self, ty: TypeId) -> Option<&[TypeId]> {
        match self.kind(ty) {
            TypeKind::Task(id) => Some(&self.task_results[id.0 as usize]),
            _ => None,
        }
    }

    pub fn signature(&self, id: FuncTypeId) -> &FuncSignature {
        &self.signatures[id.0 as usize]
    }

    pub fn func_signature(&self, ty: TypeId) -> Option<&FuncSignature> {
        match self.kind(ty) {
            TypeKind::Func(id) => Some(self.signature(id)),
            _ => None,
        }
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
        if let Some((name, _)) = self.store.named.get(&self.ty) {
            return f.write_str(name);
        }
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
            TypeKind::Slice { element, mutable } => {
                let prefix = if mutable { "mut " } else { "" };
                write!(f, "{prefix}[]{}", self.store.display(element))
            }
            TypeKind::DynArray { element } => {
                write!(f, "Array<{}>", self.store.display(element))
            }
            TypeKind::Map { key, value } => write!(
                f,
                "map[{}]{}",
                self.store.display(key),
                self.store.display(value)
            ),
            TypeKind::Channel { element } => {
                write!(f, "channel<{}>", self.store.display(element))
            }
            TypeKind::Mutex { element } => {
                write!(f, "Mutex<{}>", self.store.display(element))
            }
            TypeKind::Interface(id) => f.write_str(&self.store.interface_names[id.0 as usize]),
            TypeKind::InterfaceView { interface, mutable } => {
                let prefix = if mutable { "mut " } else { "" };
                write!(
                    f,
                    "{prefix}{} (borrowed)",
                    self.store.interface_names[interface.0 as usize]
                )
            }
            TypeKind::Task(id) => {
                let results = &self.store.task_results[id.0 as usize];
                f.write_str("Task")?;
                if results.is_empty() {
                    return Ok(());
                }
                f.write_str("<")?;
                for (index, &ty) in results.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{}", self.store.display(ty))?;
                }
                f.write_str(">")
            }
            TypeKind::Func(id) => {
                let signature = self.store.signature(id);
                if signature.is_async {
                    f.write_str("async ")?;
                }
                f.write_str("func(")?;
                for (index, &(mode, ty)) in signature.params.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    match mode {
                        ParamMode::Borrow => {}
                        ParamMode::Mut => f.write_str("mut ")?,
                        ParamMode::Own => f.write_str("own ")?,
                    }
                    match self.store.kind(ty) {
                        TypeKind::InterfaceView { interface, .. } => {
                            f.write_str(&self.store.interface_names[interface.0 as usize])?
                        }
                        _ => write!(f, "{}", self.store.display(ty))?,
                    }
                }
                f.write_str(")")?;
                match &signature.results[..] {
                    [] => Ok(()),
                    [one] => write!(f, " {}", self.store.display(*one)),
                    many => {
                        f.write_str(" (")?;
                        for (index, &ty) in many.iter().enumerate() {
                            if index > 0 {
                                f.write_str(", ")?;
                            }
                            write!(f, "{}", self.store.display(ty))?;
                        }
                        f.write_str(")")
                    }
                }
            }
        }
    }
}
