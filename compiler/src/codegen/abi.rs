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
declare ptr @zore_map_key_at(ptr, i64)
declare void @zore_string_concat(ptr, ptr, i64, ptr, i64)
declare void @zore_string_retain(ptr, i64)
declare void @zore_string_release(ptr, i64)
declare void @zore_string_from_rune(ptr, i32)
declare zeroext i1 @zore_string_is_boundary(ptr, i64, i64)
declare i32 @zore_string_rune_at(ptr, i64, i64)
declare i64 @zore_string_char_width(ptr, i64, i64)
declare noalias ptr @zore_realloc(ptr, i64, i64)
declare void @zore_map_free(ptr)
declare ptr @zore_map_clone_shape(ptr)
declare void @zore_raise_panic(ptr, i64)
declare zeroext i1 @zore_panic_pending()
declare void @zore_enter_drop()
declare void @zore_leave_drop()
declare void @zore_abort() noreturn
declare ptr @zore_task_spawn(ptr, ptr, ptr, i64)
declare ptr @zore_task_spawn_poll(ptr, ptr, ptr, i64)
declare i8 @zore_task_poll(ptr, ptr, i64, ptr)
declare ptr @zore_task_wait(ptr, i64)
declare void @zore_task_detach(ptr)
declare ptr @zore_channel_make(i64, i64, ptr)
declare void @zore_channel_retain(ptr)
declare void @zore_channel_release(ptr)
declare void @zore_channel_send(ptr, ptr, ptr)
declare zeroext i1 @zore_channel_receive(ptr, ptr, i64)
declare void @zore_channel_close(ptr)
declare i64 @zore_select(ptr, i64, i1 zeroext)
declare ptr @zore_channel_start(ptr, i64, i1 zeroext, ptr)
declare i8 @zore_channel_poll(ptr, ptr, ptr)
declare ptr @zore_native_time_sleep_start(i64, ptr)
declare i8 @zore_reactor_poll(ptr)
declare ptr @zore_mutex_new(i64, ptr, ptr)
declare void @zore_mutex_retain(ptr)
declare void @zore_mutex_release(ptr)
declare ptr @zore_mutex_lock(ptr)
declare ptr @zore_blocking_start(ptr, ptr, ptr)
declare i8 @zore_blocking_poll(ptr)
declare ptr @zore_mutex_start(ptr, ptr)
declare i8 @zore_mutex_poll(ptr, ptr, ptr)
declare void @zore_mutex_unlock(ptr, i1 zeroext)
declare zeroext i1 @zore_mutex_is_poisoned(ptr)
declare double @llvm.trunc.f64(double)
declare double @llvm.fabs.f64(double)
";

impl Module<'_> {
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
        let rendered = self.arguments(args);
        let target = format!("@\"{}.{}\"", package.name, callee.name);
        self.emit_call(&target, &callee.results, &rendered)
    }

    /// The closure's code takes its environment before the ordinary arguments.
    pub(super) fn call_value(
        &mut self,
        place: &mir::Place,
        args: &[Operand],
    ) -> Option<(String, String)> {
        let package = self.module.package;
        let results = package
            .types
            .func_signature(self.place_ty(place))
            .expect("a function-typed callee")
            .results
            .clone();
        let address = self.address(place);
        let closure = self.fresh();
        self.line(format!(
            "{closure} = load {{ ptr, ptr, ptr }}, ptr {address}"
        ));
        let code = self.fresh();
        self.line(format!(
            "{code} = extractvalue {{ ptr, ptr, ptr }} {closure}, 0"
        ));
        let environment = self.fresh();
        self.line(format!(
            "{environment} = extractvalue {{ ptr, ptr, ptr }} {closure}, 1"
        ));
        let mut rendered = vec![format!("ptr {environment}")];
        rendered.extend(self.arguments(args));
        self.emit_call(&code, &results, &rendered)
    }

    fn emit_call(
        &mut self,
        target: &str,
        results: &[TypeId],
        rendered: &[String],
    ) -> Option<(String, String)> {
        let call = format!("{target}({})", rendered.join(", "));
        if results.is_empty() {
            self.line(format!("call void {call}"));
            return None;
        }
        let ret = self.module.results_ty(results);
        let result = self.fresh();
        self.line(format!("{result} = call {ret} {call}"));
        Some((ret, result))
    }

    /// A borrowed Move value passes its address and its drop flags' address.
    pub(super) fn arguments(&mut self, args: &[Operand]) -> Vec<String> {
        let package = self.module.package;
        let mut rendered = Vec::new();
        for arg in args {
            if let Operand::Ref(place) = arg {
                let address = self.address(place);
                rendered.push(format!("ptr {address}"));
                let ty = self.place_ty(place);
                if package.needs_drop(ty) {
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
            let value = self.owned_value(arg);
            rendered.push(format!("{ty} {value}"));
        }
        rendered
    }

    /// Lookup and removal return a `{ i1, V }` presence/value pair.
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
                if self.module.package.copies_shared(value) {
                    self.retain_at(&source, value);
                }
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
                if self.module.package.needs_drop(value) {
                    let (drop_old, stored) = (self.label(), self.label());
                    self.line(format!("br i1 {found}, label %{drop_old}, label %{stored}"));
                    self.out.push_str(&format!("{drop_old}:\n"));
                    self.drop_unconditional(&old, value);
                    self.line(format!("br label %{stored}"));
                    self.out.push_str(&format!("{stored}:\n"));
                    // The old entry is detached; on a panic the new value stays with its temporary.
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
            Callee::Function(_)
            | Callee::Value(_)
            | Callee::Println
            | Callee::Drop
            | Callee::Clone(_)
            | Callee::ArrayPush
            | Callee::ArrayPop
            | Callee::TaskWait
            | Callee::ChannelMake(_)
            | Callee::ChannelSend
            | Callee::ChannelReceive
            | Callee::ChannelClose
            | Callee::Select { .. }
            | Callee::MutexNew(_)
            | Callee::MutexWithLock
            | Callee::MutexIsPoisoned => {
                unreachable!("not a map operation")
            }
        }
    }

    /// `{ data, length, capacity }`; growth doubles the capacity.
    pub(super) fn array_call(
        &mut self,
        callee: &Callee,
        args: &[Operand],
    ) -> Option<(String, String)> {
        let Operand::Ref(array) = &args[0] else {
            unreachable!("array operations borrow their array")
        };
        let TypeKind::DynArray { element } = self.module.package.types.kind(self.place_ty(array))
        else {
            unreachable!("array operations take an `Array<T>`")
        };
        let element_ty = self.ty(element);
        let address = self.address(array);
        let descriptor = self.fresh();
        self.line(format!(
            "{descriptor} = load {{ ptr, i64, i64 }}, ptr {address}"
        ));
        let [data, length, capacity] = [0, 1, 2].map(|index| {
            let field = self.fresh();
            self.line(format!(
                "{field} = extractvalue {{ ptr, i64, i64 }} {descriptor}, {index}"
            ));
            field
        });
        if let Callee::ArrayPop = callee {
            let found = self.fresh();
            self.line(format!("{found} = icmp ugt i64 {length}, 0"));
            let buffer = self.fresh();
            self.hoist_alloca(&buffer, &element_ty);
            self.line(format!("store {element_ty} zeroinitializer, ptr {buffer}"));
            let (take, done) = (self.label(), self.label());
            self.line(format!("br i1 {found}, label %{take}, label %{done}"));
            self.out.push_str(&format!(
                "{take}:
"
            ));
            let last = self.fresh();
            self.line(format!("{last} = sub i64 {length}, 1"));
            let slot = self.fresh();
            self.line(format!(
                "{slot} = getelementptr inbounds {element_ty}, ptr {data}, i64 {last}"
            ));
            let value = self.fresh();
            self.line(format!("{value} = load {element_ty}, ptr {slot}"));
            self.line(format!("store {element_ty} {value}, ptr {buffer}"));
            let length_slot = self.fresh();
            self.line(format!(
                "{length_slot} = getelementptr inbounds {{ ptr, i64, i64 }}, ptr {address}, i32 0, i32 1"
            ));
            self.line(format!("store i64 {last}, ptr {length_slot}"));
            self.line(format!("br label %{done}"));
            self.out.push_str(&format!(
                "{done}:
"
            ));
            return Some(self.presence_pair(&found, &buffer, &element_ty));
        }
        let full = self.fresh();
        self.line(format!("{full} = icmp eq i64 {length}, {capacity}"));
        let (grow, store) = (self.label(), self.label());
        let current = self.label();
        self.line(format!("br label %{current}"));
        self.out.push_str(&format!(
            "{current}:
"
        ));
        self.line(format!("br i1 {full}, label %{grow}, label %{store}"));
        self.out.push_str(&format!(
            "{grow}:
"
        ));
        let doubled = self.fresh();
        self.line(format!("{doubled} = shl i64 {capacity}, 1"));
        let is_empty = self.fresh();
        self.line(format!("{is_empty} = icmp eq i64 {capacity}, 0"));
        let grown_capacity = self.fresh();
        self.line(format!(
            "{grown_capacity} = select i1 {is_empty}, i64 4, i64 {doubled}"
        ));
        let old_bytes = self.byte_size(&element_ty, &capacity);
        let new_bytes = self.byte_size(&element_ty, &grown_capacity);
        let grown = self.fresh();
        self.line(format!(
            "{grown} = call ptr @zore_realloc(ptr {data}, i64 {old_bytes}, i64 {new_bytes})"
        ));
        self.line(format!("store ptr {grown}, ptr {address}"));
        let capacity_slot = self.fresh();
        self.line(format!(
            "{capacity_slot} = getelementptr inbounds {{ ptr, i64, i64 }}, ptr {address}, i32 0, i32 2"
        ));
        self.line(format!("store i64 {grown_capacity}, ptr {capacity_slot}"));
        self.line(format!("br label %{store}"));
        self.out.push_str(&format!(
            "{store}:
"
        ));
        let storage = self.fresh();
        self.line(format!(
            "{storage} = phi ptr [ {data}, %{current} ], [ {grown}, %{grow} ]"
        ));
        let slot = self.fresh();
        self.line(format!(
            "{slot} = getelementptr inbounds {element_ty}, ptr {storage}, i64 {length}"
        ));
        let value = self.owned_value(&args[1]);
        self.line(format!("store {element_ty} {value}, ptr {slot}"));
        let longer = self.fresh();
        self.line(format!("{longer} = add i64 {length}, 1"));
        let length_slot = self.fresh();
        self.line(format!(
            "{length_slot} = getelementptr inbounds {{ ptr, i64, i64 }}, ptr {address}, i32 0, i32 1"
        ));
        self.line(format!("store i64 {longer}, ptr {length_slot}"));
        None
    }

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

    fn zeroed_buffer(&mut self, ty: &str) -> String {
        let buffer = self.fresh();
        self.hoist_alloca(&buffer, ty);
        self.line(format!("store {ty} zeroinitializer, ptr {buffer}"));
        buffer
    }

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
        let value = self.owned_value(value);
        self.line(format!("store {value_ty} {value}, ptr {storage}"));
    }

    pub(super) fn println(&mut self, arg: &Operand, span: Span) {
        let arg_ty = self.operand_ty(arg);
        let kind = self.module.package.types.kind(arg_ty);
        if let TypeKind::Float(_) = kind {
            self.module.unsupported(
                "printing floating-point values is",
                span,
                "the float text format for `println` is still TBD",
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
            | TypeKind::Map { .. }
            | TypeKind::Func(_)
            | TypeKind::Task(_)
            | TypeKind::Channel { .. }
            | TypeKind::Mutex { .. } => unreachable!("checked printable type"),
        }
    }
}
