use std::fmt::Write;

use super::llvm::{FunctionBuilder, Module};
use crate::mir::{self, Place};
use crate::resolve::FunctionId;
use crate::types::TypeId;

/// An owning closure's heap environment: capture pointers first, then each value and its flags.
pub(super) struct Environment {
    pub(super) ty: String,
    /// Per capture: its type, its value field, and its flags field for a Move value.
    pub(super) fields: Vec<(TypeId, Option<usize>, Option<usize>)>,
}

impl Module<'_> {
    pub(super) fn environment(&self, closure: FunctionId) -> Environment {
        let function = self.package.function(closure);
        let mut parts = Vec::new();
        let mut fields = Vec::new();
        let mut pointers = 0;
        for (index, &capture) in function.captures.iter().enumerate() {
            let ty = function.locals[capture.0 as usize].ty;
            if self
                .capture_modes
                .get(&closure)
                .is_some_and(|modes| modes[index] != crate::ast::ParamMode::Own)
            {
                pointers += 1 + usize::from(self.package.needs_drop(ty));
                fields.push((ty, None, None));
                continue;
            }
            parts.push(self.ty(ty));
            let value = parts.len();
            pointers += 1;
            let flags = self.package.needs_drop(ty).then(|| {
                parts.push(self.flag_ty(ty));
                pointers += 1;
                parts.len()
            });
            fields.push((ty, Some(value), flags));
        }
        let mut ty = format!("{{ [{pointers} x ptr]");
        for part in parts {
            write!(ty, ", {part}").unwrap();
        }
        ty.push_str(" }");
        Environment { ty, fields }
    }

    fn closure_code(&self, closure: FunctionId) -> String {
        if self.async_values.contains(&closure) {
            self.async_name(closure, "new")
        } else {
            self.closure_name(closure)
        }
    }

    pub(super) fn closure_name(&self, closure: FunctionId) -> String {
        format!(
            "@\"{}.{}\"",
            self.package.name,
            self.package.function(closure).name
        )
    }

    pub(super) fn environment_drop_name(&self, closure: FunctionId) -> String {
        format!(
            "@\"{}.{}.drop\"",
            self.package.name,
            self.package.function(closure).name
        )
    }
    /// Destroys the captured values a call has not consumed, newest first, then frees the storage.
    pub(super) fn environment_drop(&mut self, body: &mir::Body) -> String {
        let closure = body.function;
        let environment = self.environment(closure);
        let name = self.environment_drop_name(closure);
        let mut f = FunctionBuilder {
            module: self,
            body,
            out: String::new(),
            next: 0,
            active_unwind: None,
            emitting_unwind: false,
            drop_check_after_store: false,
            hoisted: String::new(),
            polling: false,
        };
        for &(ty, value, flags) in environment.fields.iter().rev() {
            let Some(value) = value else {
                continue;
            };
            let Some(flags) = flags else {
                continue;
            };
            let slot = f.fresh();
            f.line(format!(
                "{slot} = getelementptr inbounds {}, ptr %env, i32 0, i32 {value}",
                environment.ty
            ));
            let flag_slot = f.fresh();
            f.line(format!(
                "{flag_slot} = getelementptr inbounds {}, ptr %env, i32 0, i32 {flags}",
                environment.ty
            ));
            f.drop_value(&slot, ty, &flag_slot);
        }
        let size = f.byte_size(&environment.ty, "1");
        f.line(format!("call void @zore_free(ptr %env, i64 {size})"));
        f.line("ret void");
        format!(
            "define private void {name}(ptr %env) {{\nentry:\n{}{}}}\n\n",
            f.hoisted, f.out
        )
    }
}

impl FunctionBuilder<'_, '_> {
    /// Per capture, the environment holds its address, then its drop flags' address for a Move value.
    pub(super) fn load_captures(&mut self) {
        let mut slot = 0;
        for &capture in &self.body.captures {
            let mut targets = vec![format!("%l{}", capture.0)];
            if self.module.package.needs_drop(self.local_ty(capture)) {
                targets.push(format!("%lf{}", capture.0));
            }
            for target in targets {
                let entry = self.fresh();
                self.line(format!(
                    "{entry} = getelementptr inbounds ptr, ptr %env, i64 {slot}"
                ));
                let pointer = self.fresh();
                self.line(format!("{pointer} = load ptr, ptr {entry}"));
                self.line(format!("store ptr {pointer}, ptr {target}"));
                slot += 1;
            }
        }
    }

    pub(super) fn closure_value(
        &mut self,
        function: FunctionId,
        captures: &[(Place, bool)],
        owning: bool,
    ) -> String {
        if owning {
            return self.owning_closure_value(function, captures);
        }
        let package = self.module.package;
        let mut pointers = Vec::new();
        for (place, _) in captures {
            pointers.push(self.address(place));
            if package.needs_drop(self.place_ty(place)) {
                pointers.push(self.flag_address(place));
            }
        }
        let environment = if pointers.is_empty() {
            "null".to_string()
        } else {
            // A site stores the same addresses every run, so one slot serves all its values.
            let environment = self.fresh();
            self.hoist_alloca(&environment, &format!("[{} x ptr]", pointers.len()));
            for (index, pointer) in pointers.iter().enumerate() {
                let entry = self.fresh();
                self.line(format!(
                    "{entry} = getelementptr inbounds ptr, ptr {environment}, i64 {index}"
                ));
                self.line(format!("store ptr {pointer}, ptr {entry}"));
            }
            environment
        };
        let code = self.module.closure_code(function);
        self.closure_triple(&code, &environment, "null")
    }

    pub(super) fn closure_triple(&mut self, code: &str, environment: &str, drop: &str) -> String {
        let mut value = "undef".to_string();
        for (index, field) in [code, environment, drop].into_iter().enumerate() {
            let next = self.fresh();
            self.line(format!(
                "{next} = insertvalue {{ ptr, ptr, ptr }} {value}, ptr {field}, {index}"
            ));
            value = next;
        }
        value
    }

    /// Copies or moves each capture into heap storage that the closure value owns.
    fn owning_closure_value(&mut self, function: FunctionId, captures: &[(Place, bool)]) -> String {
        let environment = self.module.environment(function);
        let size = self.byte_size(&environment.ty, "1");
        let block = self.fresh();
        self.line(format!("{block} = call ptr @zore_alloc(i64 {size})"));
        let mut pointer = 0;
        for ((place, exclusive), &(ty, value, flags)) in captures.iter().zip(&environment.fields) {
            let Some(value) = value else {
                let address = self.address(place);
                self.store_environment_pointer(&environment, &block, pointer, &address);
                pointer += 1;
                if self.module.package.needs_drop(ty) {
                    let flags = self.flag_address(place);
                    self.store_environment_pointer(&environment, &block, pointer, &flags);
                    pointer += 1;
                }
                continue;
            };
            let operand =
                mir::captured_operand(self.module.package, &self.body.locals, place, *exclusive);
            let loaded = self.owned_value(&operand);
            let slot = self.environment_field(&environment, &block, value);
            self.line(format!("store {} {loaded}, ptr {slot}", self.ty(ty)));
            self.store_environment_pointer(&environment, &block, pointer, &slot);
            pointer += 1;
            if let Some(flags) = flags {
                let flag_slot = self.environment_field(&environment, &block, flags);
                self.init_flags_true(&flag_slot, ty);
                self.store_environment_pointer(&environment, &block, pointer, &flag_slot);
                pointer += 1;
            }
        }
        let code = self.module.closure_code(function);
        let drop = self.module.environment_drop_name(function);
        self.closure_triple(&code, &block, &drop)
    }

    fn environment_field(
        &mut self,
        environment: &Environment,
        block: &str,
        field: usize,
    ) -> String {
        let slot = self.fresh();
        self.line(format!(
            "{slot} = getelementptr inbounds {}, ptr {block}, i32 0, i32 {field}",
            environment.ty
        ));
        slot
    }

    fn store_environment_pointer(
        &mut self,
        environment: &Environment,
        block: &str,
        index: usize,
        pointer: &str,
    ) {
        let entry = self.fresh();
        self.line(format!(
            "{entry} = getelementptr inbounds {}, ptr {block}, i32 0, i32 0, i64 {index}",
            environment.ty
        ));
        self.line(format!("store ptr {pointer}, ptr {entry}"));
    }

    /// Only an owning closure has a destructor.
    pub(super) fn drop_closure(&mut self, address: &str) {
        let closure = self.fresh();
        self.line(format!(
            "{closure} = load {{ ptr, ptr, ptr }}, ptr {address}"
        ));
        let environment = self.fresh();
        self.line(format!(
            "{environment} = extractvalue {{ ptr, ptr, ptr }} {closure}, 1"
        ));
        let destructor = self.fresh();
        self.line(format!(
            "{destructor} = extractvalue {{ ptr, ptr, ptr }} {closure}, 2"
        ));
        let owning = self.fresh();
        self.line(format!("{owning} = icmp ne ptr {destructor}, null"));
        let (run, done) = (self.label(), self.label());
        self.line(format!("br i1 {owning}, label %{run}, label %{done}"));
        self.out.push_str(&format!("{run}:\n"));
        self.line(format!("call void {destructor}(ptr {environment})"));
        self.line(format!("br label %{done}"));
        self.out.push_str(&format!("{done}:\n"));
    }
}
