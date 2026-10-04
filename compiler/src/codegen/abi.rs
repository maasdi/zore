//! The runtime ABI: declared runtime symbols, the calling convention for Zore
//! functions, runtime output calls, and the native entry shim.

use super::llvm::{FunctionBuilder, Module};
use crate::mir::{self, Callee, Operand};
use crate::resolve::FunctionId;
use crate::source::Span;
use crate::types::{TypeId, TypeKind};

pub(super) const RUNTIME_DECLARATIONS: &str = "\
declare void @zore_println_str(ptr, i64)
declare void @zore_println_i64(i64)
declare void @zore_println_u64(i64)
declare void @zore_println_bool(i1 zeroext)
declare void @zore_println_rune(i32)
declare i32 @zore_string_compare(ptr, i64, ptr, i64)
declare noalias ptr @zore_alloc(i64)
declare void @zore_free(ptr, i64)
declare ptr @zore_map_find(ptr, i32, ptr)
declare ptr @zore_map_insert(ptr, i32, ptr, i64)
declare zeroext i1 @zore_map_detach(ptr, i32, ptr, ptr)
declare i64 @zore_map_len(ptr)
declare ptr @zore_map_value_at(ptr, i64)
declare void @zore_map_free(ptr)
declare void @zore_raise_panic(ptr, i64)
declare zeroext i1 @zore_panic_pending()
declare void @zore_enter_drop()
declare void @zore_leave_drop()
declare void @zore_abort() noreturn
declare double @llvm.trunc.f64(double)
declare double @llvm.fabs.f64(double)
";

impl Module<'_> {
    /// The native entry point the Rust runtime calls into.
    pub(super) fn entry_shim(&self, entry: FunctionId) -> String {
        let name = &self.package.function(entry).name;
        format!(
            "define void @zore_entry() {{\nentry:\n  call void @\"{}.{name}\"()\n  ret void\n}}\n",
            self.package.name
        )
    }
}

impl FunctionBuilder<'_, '_> {
    pub(super) fn call(&mut self, id: FunctionId, args: &[Operand]) -> Option<(String, String)> {
        let package = self.module.package;
        let callee = package.function(id);
        let mut rendered = Vec::new();
        for arg in args {
            if let Operand::Ref(place) = arg {
                let address = self.address(place);
                rendered.push(format!("ptr {address}"));
                let ty = self.place_ty(place);
                if !package.is_copy(ty) {
                    let flags = if place
                        .projections
                        .iter()
                        .any(|p| matches!(p, mir::Projection::Index(_)))
                    {
                        self.scratch_flags(ty)
                    } else {
                        self.flag_address(place)
                    };
                    rendered.push(format!("ptr {flags}"));
                }
                continue;
            }
            let ty = self.ty(self.operand_ty(arg));
            let value = self.value(arg);
            rendered.push(format!("{ty} {value}"));
        }
        let ret = self.module.results_ty(&callee.results);
        let target = format!(
            "@\"{}.{}\"({})",
            package.name,
            callee.name,
            rendered.join(", ")
        );
        if callee.results.is_empty() {
            self.line(format!("call void {target}"));
            return None;
        }
        let result = self.fresh();
        self.line(format!("{result} = call {ret} {target}"));
        Some((ret, result))
    }

    /// Emits a compiler-provided map operation (§13.3). Lookup and removal
    /// return the `{ i1, V }` presence/value pair for their destinations.
    pub(super) fn map_call(
        &mut self,
        callee: &Callee,
        args: &[Operand],
        span: Span,
    ) -> Option<(String, String)> {
        let Operand::Ref(map_place) = &args[0] else {
            unreachable!("map operations borrow their map")
        };
        let TypeKind::Map { key, value } = self.module.package.types.kind(self.place_ty(map_place))
        else {
            unreachable!("map operations take a map")
        };
        let slot = self.address(map_place);
        let (kind, key) = self.map_key(&args[1], key);
        let value_ty = self.ty(value);
        let map = self.fresh();
        self.line(format!("{map} = load ptr, ptr {slot}"));
        match callee {
            Callee::MapLookup => {
                let found_at = self.fresh();
                self.line(format!(
                    "{found_at} = call ptr @zore_map_find(ptr {map}, i32 {kind}, ptr {key})"
                ));
                let found = self.fresh();
                self.line(format!("{found} = icmp ne ptr {found_at}, null"));
                let zero = self.zeroed_buffer(&value_ty);
                let source = self.fresh();
                self.line(format!(
                    "{source} = select i1 {found}, ptr {found_at}, ptr {zero}"
                ));
                Some(self.presence_pair(&found, &source, &value_ty))
            }
            Callee::MapRemove => {
                let buffer = self.zeroed_buffer(&value_ty);
                let found = self.fresh();
                self.line(format!(
                    "{found} = call zeroext i1 @zore_map_detach(ptr {map}, i32 {kind}, ptr {key}, ptr {buffer})"
                ));
                Some(self.presence_pair(&found, &buffer, &value_ty))
            }
            Callee::MapAssign => {
                let old = self.zeroed_buffer(&value_ty);
                let found = self.fresh();
                self.line(format!(
                    "{found} = call zeroext i1 @zore_map_detach(ptr {map}, i32 {kind}, ptr {key}, ptr {old})"
                ));
                if !self.module.package.is_copy(value) {
                    let (drop_old, stored) = (self.label(), self.label());
                    self.line(format!("br i1 {found}, label %{drop_old}, label %{stored}"));
                    self.out.push_str(&format!("{drop_old}:\n"));
                    self.drop_unconditional(&old, value);
                    self.line(format!("br label %{stored}"));
                    self.out.push_str(&format!("{stored}:\n"));
                    // §13.3: the old entry is already detached; on a panic the
                    // new value stays with its temporary for unwinding.
                    self.check_after_drop(false);
                }
                self.store_map_value(&slot, &kind, &key, &args[2], &value_ty);
                None
            }
            Callee::MapInsertNew => {
                let existing = self.fresh();
                self.line(format!(
                    "{existing} = call ptr @zore_map_find(ptr {map}, i32 {kind}, ptr {key})"
                ));
                let duplicate = self.fresh();
                self.line(format!("{duplicate} = icmp ne ptr {existing}, null"));
                let (fail, insert) = (self.label(), self.label());
                self.line(format!("br i1 {duplicate}, label %{fail}, label %{insert}"));
                self.out.push_str(&format!("{fail}:\n"));
                self.raise_panic("duplicate key in map literal", span);
                let unwind = self.body.unwind.expect("map literal cleanup block");
                self.line(format!("br label %bb{}", unwind.0));
                self.out.push_str(&format!("{insert}:\n"));
                self.store_map_value(&slot, &kind, &key, &args[2], &value_ty);
                None
            }
            Callee::Function(_) | Callee::Println | Callee::Drop => {
                unreachable!("not a map operation")
            }
        }
    }

    /// Stores the key operand in a hoisted slot, returning the runtime key
    /// kind and the slot's address.
    fn map_key(&mut self, operand: &Operand, ty: TypeId) -> (String, String) {
        let value = self.value(operand);
        let slot = self.fresh();
        match self.module.package.types.kind(ty) {
            TypeKind::Bool => {
                // An `i1` in memory leaves its other bits unspecified.
                let byte = self.fresh();
                self.line(format!("{byte} = zext i1 {value} to i8"));
                self.hoist_alloca(&slot, "i8");
                self.line(format!("store i8 {byte}, ptr {slot}"));
                ("1".to_string(), slot)
            }
            TypeKind::Int(int) => {
                let ty = format!("i{}", int.bits);
                self.hoist_alloca(&slot, &ty);
                self.line(format!("store {ty} {value}, ptr {slot}"));
                ((int.bits / 8).to_string(), slot)
            }
            TypeKind::Rune => {
                self.hoist_alloca(&slot, "i32");
                self.line(format!("store i32 {value}, ptr {slot}"));
                ("4".to_string(), slot)
            }
            TypeKind::String => {
                self.hoist_alloca(&slot, "{ ptr, i64 }");
                self.line(format!("store {{ ptr, i64 }} {value}, ptr {slot}"));
                ("16".to_string(), slot)
            }
            _ => unreachable!("checked map key type"),
        }
    }

    /// A hoisted buffer for one value of `ty`, zeroed here.
    fn zeroed_buffer(&mut self, ty: &str) -> String {
        let buffer = self.fresh();
        self.hoist_alloca(&buffer, ty);
        self.line(format!("store {ty} zeroinitializer, ptr {buffer}"));
        buffer
    }

    /// The `{ i1, V }` pair of `found` and the value loaded from `source`.
    fn presence_pair(&mut self, found: &str, source: &str, value_ty: &str) -> (String, String) {
        let value = self.fresh();
        self.line(format!("{value} = load {value_ty}, ptr {source}"));
        let pair_ty = format!("{{ i1, {value_ty} }}");
        let with_found = self.fresh();
        self.line(format!(
            "{with_found} = insertvalue {pair_ty} undef, i1 {found}, 0"
        ));
        let pair = self.fresh();
        self.line(format!(
            "{pair} = insertvalue {pair_ty} {with_found}, {value_ty} {value}, 1"
        ));
        (pair_ty, pair)
    }

    /// Inserts the absent key into the map behind `slot` and moves `value` in.
    fn store_map_value(
        &mut self,
        slot: &str,
        kind: &str,
        key: &str,
        value: &Operand,
        value_ty: &str,
    ) {
        let size = self.byte_size(value_ty, "1");
        let storage = self.fresh();
        self.line(format!(
            "{storage} = call ptr @zore_map_insert(ptr {slot}, i32 {kind}, ptr {key}, i64 {size})"
        ));
        let value = self.value(value);
        self.line(format!("store {value_ty} {value}, ptr {storage}"));
    }

    pub(super) fn println(&mut self, arg: &Operand, span: Span) {
        let arg_ty = self.operand_ty(arg);
        let kind = self.module.package.types.kind(arg_ty);
        if let TypeKind::Float(_) = kind {
            self.module.unsupported(
                "printing floating-point values is",
                span,
                "the float text format for `println` is still TBD (§37.1)",
            );
            return;
        }
        let value = self.value(arg);
        match kind {
            TypeKind::String => {
                let (ptr, len) = self.string_parts(&value);
                self.line(format!("call void @zore_println_str(ptr {ptr}, i64 {len})"));
            }
            TypeKind::Bool => {
                self.line(format!("call void @zore_println_bool(i1 zeroext {value})"))
            }
            TypeKind::Rune => self.line(format!("call void @zore_println_rune(i32 {value})")),
            TypeKind::Int(int) => {
                let wide = if int.bits == 64 {
                    value
                } else {
                    let name = self.fresh();
                    let extend = if int.signed { "sext" } else { "zext" };
                    self.line(format!("{name} = {extend} i{} {value} to i64", int.bits));
                    name
                };
                let function = if int.signed {
                    "zore_println_i64"
                } else {
                    "zore_println_u64"
                };
                self.line(format!("call void @{function}(i64 {wide})"));
            }
            TypeKind::Float(_)
            | TypeKind::Error
            | TypeKind::Struct(_)
            | TypeKind::Array { .. }
            | TypeKind::Slice { .. }
            | TypeKind::DynArray { .. }
            | TypeKind::Map { .. } => unreachable!("checked printable type"),
        }
    }
}
