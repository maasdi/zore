use super::llvm::FunctionBuilder;
use crate::mir::Operand;
use crate::types::{TypeId, TypeKind};

impl FunctionBuilder<'_, '_> {
    pub(super) fn clone_call(&mut self, ty: TypeId, args: &[Operand]) -> (String, String) {
        let [Operand::Ref(place)] = args else {
            unreachable!("clone borrows one place")
        };
        let source = self.address(place);
        let ty_text = self.ty(ty);
        let slot = self.fresh();
        self.hoist_alloca(&slot, &ty_text);
        let (failed, done) = (self.label(), self.label());
        self.clone_value(&source, &slot, ty, &failed);
        self.line(format!("br label %{done}"));
        self.out.push_str(&format!("{failed}:\n"));
        self.line(format!("br label %{done}"));
        self.out.push_str(&format!("{done}:\n"));
        let value = self.fresh();
        self.line(format!("{value} = load {ty_text}, ptr {slot}"));
        (ty_text, value)
    }

    fn branch_if_panicking(&mut self, failed: &str) {
        let pending = self.fresh();
        self.line(format!("{pending} = call zeroext i1 @zore_panic_pending()"));
        let ok = self.label();
        self.line(format!("br i1 {pending}, label %{failed}, label %{ok}"));
        self.out.push_str(&format!("{ok}:\n"));
    }

    fn field_address(&mut self, base: &str, struct_ty: &str, index: usize) -> String {
        let address = self.fresh();
        self.line(format!(
            "{address} = getelementptr inbounds {struct_ty}, ptr {base}, i32 0, i32 {index}"
        ));
        address
    }

    /// A panic in a custom clone drops what was already cloned, then branches to `failed`.
    fn clone_value(&mut self, source: &str, target: &str, ty: TypeId, failed: &str) {
        let package = self.module.package;
        let ty_text = self.ty(ty);
        if package.is_copy(ty) {
            let value = self.fresh();
            self.line(format!("{value} = load {ty_text}, ptr {source}"));
            self.line(format!("store {ty_text} {value}, ptr {target}"));
            if package.holds_shared(ty) {
                self.retain_at(target, ty);
            }
            return;
        }
        match package.types.kind(ty) {
            TypeKind::Struct(id) => {
                let strukt = package.strukt(id);
                match strukt.clone {
                    Some(method) => self.clone_with_method(method, source, target, ty, failed),
                    None => self.clone_fields(source, target, ty, failed),
                }
            }
            TypeKind::Array { element, size } => {
                let first_source = self.element_address(source, &ty_text, "0");
                let first_target = self.element_address(target, &ty_text, "0");
                self.clone_elements(
                    &first_source,
                    &first_target,
                    element,
                    &size.to_string(),
                    failed,
                    |_| {},
                );
            }
            TypeKind::DynArray { element } => self.clone_dyn_array(source, target, element, failed),
            TypeKind::Map { value, .. } => self.clone_map(source, target, value, failed),
            _ => unreachable!("only owned composites need clone code"),
        }
    }

    fn element_address(&mut self, base: &str, array_ty: &str, index: &str) -> String {
        let address = self.fresh();
        self.line(format!(
            "{address} = getelementptr inbounds {array_ty}, ptr {base}, i64 0, i64 {index}"
        ));
        address
    }

    fn clone_with_method(
        &mut self,
        method: crate::resolve::FunctionId,
        source: &str,
        target: &str,
        ty: TypeId,
        failed: &str,
    ) {
        let package = self.module.package;
        let flags = self.scratch_flags(ty);
        let ty_text = self.ty(ty);
        let result = self.fresh();
        self.line(format!(
            "{result} = call {ty_text} @\"{}.{}\"(ptr {source}, ptr {flags})",
            package.name,
            package.function(method).name
        ));
        self.branch_if_panicking(failed);
        self.line(format!("store {ty_text} {result}, ptr {target}"));
    }

    fn clone_fields(&mut self, source: &str, target: &str, ty: TypeId, failed: &str) {
        let package = self.module.package;
        let TypeKind::Struct(id) = package.types.kind(ty) else {
            unreachable!("fieldwise clone of a struct")
        };
        let field_types: Vec<TypeId> = package.strukt(id).fields.iter().map(|f| f.ty).collect();
        let struct_ty = self.ty(ty);
        let mut fail_labels = vec![failed.to_string()];
        for _ in 1..field_types.len() {
            let label = self.label();
            fail_labels.push(label);
        }
        for (index, &field_ty) in field_types.iter().enumerate() {
            let from = self.field_address(source, &struct_ty, index);
            let to = self.field_address(target, &struct_ty, index);
            let fail = fail_labels[index].clone();
            self.clone_value(&from, &to, field_ty, &fail);
        }
        let done = self.label();
        self.line(format!("br label %{done}"));
        for index in (1..field_types.len()).rev() {
            self.out.push_str(&format!("{}:\n", fail_labels[index]));
            let to = self.field_address(target, &struct_ty, index - 1);
            self.drop_unconditional(&to, field_types[index - 1]);
            self.line(format!("br label %{}", fail_labels[index - 1]));
        }
        self.out.push_str(&format!("{done}:\n"));
    }

    /// On a panic, drops the cloned prefix, runs `cleanup`, then branches to `failed`.
    fn clone_elements(
        &mut self,
        source: &str,
        target: &str,
        element: TypeId,
        count: &str,
        failed: &str,
        cleanup: impl FnOnce(&mut Self),
    ) {
        let element_ty = self.ty(element);
        let counter = self.fresh();
        self.hoist_alloca(&counter, "i64");
        self.line(format!("store i64 0, ptr {counter}"));
        let (check, body, done, element_failed) =
            (self.label(), self.label(), self.label(), self.label());
        self.line(format!("br label %{check}"));
        self.out.push_str(&format!("{check}:\n"));
        let index = self.fresh();
        self.line(format!("{index} = load i64, ptr {counter}"));
        let more = self.fresh();
        self.line(format!("{more} = icmp ult i64 {index}, {count}"));
        self.line(format!("br i1 {more}, label %{body}, label %{done}"));
        self.out.push_str(&format!("{body}:\n"));
        let from = self.fresh();
        self.line(format!(
            "{from} = getelementptr inbounds {element_ty}, ptr {source}, i64 {index}"
        ));
        let to = self.fresh();
        self.line(format!(
            "{to} = getelementptr inbounds {element_ty}, ptr {target}, i64 {index}"
        ));
        self.clone_value(&from, &to, element, &element_failed);
        let next = self.fresh();
        self.line(format!("{next} = add i64 {index}, 1"));
        self.line(format!("store i64 {next}, ptr {counter}"));
        self.line(format!("br label %{check}"));
        self.out.push_str(&format!("{element_failed}:\n"));
        let cloned = self.fresh();
        self.line(format!("{cloned} = load i64, ptr {counter}"));
        self.drop_elements(target, element, &cloned);
        cleanup(self);
        self.line(format!("br label %{failed}"));
        self.out.push_str(&format!("{done}:\n"));
    }

    fn clone_dyn_array(&mut self, source: &str, target: &str, element: TypeId, failed: &str) {
        let descriptor = self.fresh();
        self.line(format!("{descriptor} = load {{ ptr, i64 }}, ptr {source}"));
        let data = self.fresh();
        self.line(format!(
            "{data} = extractvalue {{ ptr, i64 }} {descriptor}, 0"
        ));
        let length = self.fresh();
        self.line(format!(
            "{length} = extractvalue {{ ptr, i64 }} {descriptor}, 1"
        ));
        let element_ty = self.ty(element);
        let bytes = self.byte_size(&element_ty, &length);
        let copy = self.fresh();
        self.line(format!("{copy} = call ptr @zore_alloc(i64 {bytes})"));
        self.clone_elements(&data, &copy, element, &length, failed, |this| {
            this.line(format!("call void @zore_free(ptr {copy}, i64 {bytes})"));
        });
        let array = self.dyn_array_value(&copy, &length);
        self.line(format!("store {{ ptr, i64, i64 }} {array}, ptr {target}"));
    }

    fn clone_map(&mut self, source: &str, target: &str, value: TypeId, failed: &str) {
        let map = self.fresh();
        self.line(format!("{map} = load ptr, ptr {source}"));
        let copy = self.fresh();
        self.line(format!(
            "{copy} = call ptr @zore_map_clone_shape(ptr {map})"
        ));
        let length = self.fresh();
        self.line(format!("{length} = call i64 @zore_map_len(ptr {map})"));
        let counter = self.fresh();
        self.hoist_alloca(&counter, "i64");
        self.line(format!("store i64 0, ptr {counter}"));
        let (check, body, done, entry_failed) =
            (self.label(), self.label(), self.label(), self.label());
        self.line(format!("br label %{check}"));
        self.out.push_str(&format!("{check}:\n"));
        let index = self.fresh();
        self.line(format!("{index} = load i64, ptr {counter}"));
        let more = self.fresh();
        self.line(format!("{more} = icmp ult i64 {index}, {length}"));
        self.line(format!("br i1 {more}, label %{body}, label %{done}"));
        self.out.push_str(&format!("{body}:\n"));
        let from = self.fresh();
        self.line(format!(
            "{from} = call ptr @zore_map_value_at(ptr {map}, i64 {index})"
        ));
        let to = self.fresh();
        self.line(format!(
            "{to} = call ptr @zore_map_value_at(ptr {copy}, i64 {index})"
        ));
        self.clone_value(&from, &to, value, &entry_failed);
        let next = self.fresh();
        self.line(format!("{next} = add i64 {index}, 1"));
        self.line(format!("store i64 {next}, ptr {counter}"));
        self.line(format!("br label %{check}"));
        self.out.push_str(&format!("{entry_failed}:\n"));
        self.drop_map_values_before_counter(&copy, value, &counter);
        self.line(format!("call void @zore_map_free(ptr {copy})"));
        self.line(format!("br label %{failed}"));
        self.out.push_str(&format!("{done}:\n"));
        self.line(format!("store ptr {copy}, ptr {target}"));
    }

    fn drop_map_values_before_counter(&mut self, map: &str, value: TypeId, counter: &str) {
        if !self.module.package.needs_drop(value) {
            return;
        }
        let cloned = self.fresh();
        self.line(format!("{cloned} = load i64, ptr {counter}"));
        let drop_counter = self.fresh();
        self.hoist_alloca(&drop_counter, "i64");
        self.line(format!("store i64 0, ptr {drop_counter}"));
        let (check, body, done) = (self.label(), self.label(), self.label());
        self.line(format!("br label %{check}"));
        self.out.push_str(&format!("{check}:\n"));
        let index = self.fresh();
        self.line(format!("{index} = load i64, ptr {drop_counter}"));
        let more = self.fresh();
        self.line(format!("{more} = icmp ult i64 {index}, {cloned}"));
        self.line(format!("br i1 {more}, label %{body}, label %{done}"));
        self.out.push_str(&format!("{body}:\n"));
        let entry = self.fresh();
        self.line(format!(
            "{entry} = call ptr @zore_map_value_at(ptr {map}, i64 {index})"
        ));
        self.drop_unconditional(&entry, value);
        let next = self.fresh();
        self.line(format!("{next} = add i64 {index}, 1"));
        self.line(format!("store i64 {next}, ptr {drop_counter}"));
        self.line(format!("br label %{check}"));
        self.out.push_str(&format!("{done}:\n"));
    }
}
