use super::llvm::FunctionBuilder;
use crate::ast::ParamMode;
use crate::mir::Operand;
use crate::types::{TypeKind, TypeStore};

impl FunctionBuilder<'_, '_> {
    pub(super) fn map_with_entry(&mut self, args: &[Operand], mutable: bool) -> (String, String) {
        let (Operand::Ref(map_place), Operand::Ref(callback_place)) = (&args[0], &args[2]) else {
            unreachable!("entry access borrows the map and callback")
        };
        let package = self.module.package;
        let TypeKind::Map { key, value } = package.types.kind(self.place_ty(map_place)) else {
            unreachable!("entry access takes a map")
        };
        let signature = package
            .types
            .func_signature(self.place_ty(callback_place))
            .unwrap()
            .clone();
        let result_types: Vec<_> = std::iter::once(TypeStore::BOOL)
            .chain(signature.results.iter().copied())
            .collect();
        let result_ty = self.module.results_ty(&result_types);
        let result_slot = self.zeroed_buffer(&result_ty);
        let map_slot = self.address(map_place);
        let map = self.fresh();
        self.line(format!("{map} = load ptr, ptr {map_slot}"));
        let (kind, key) = self.map_key(&args[1], key);
        let entry = self.fresh();
        self.line(format!(
            "{entry} = call ptr @zore_map_find(ptr {map}, i32 {kind}, ptr {key})"
        ));
        let found = self.fresh();
        self.line(format!("{found} = icmp ne ptr {entry}, null"));
        let (run, done) = (self.label(), self.label());
        self.line(format!("br i1 {found}, label %{run}, label %{done}"));
        self.out.push_str(&format!("{run}:\n"));
        let callback_slot = self.address(callback_place);
        let closure = self.fresh();
        self.line(format!(
            "{closure} = load {{ ptr, ptr, ptr }}, ptr {callback_slot}"
        ));
        let code = self.fresh();
        let environment = self.fresh();
        self.line(format!(
            "{code} = extractvalue {{ ptr, ptr, ptr }} {closure}, 0"
        ));
        self.line(format!(
            "{environment} = extractvalue {{ ptr, ptr, ptr }} {closure}, 1"
        ));
        let mut rendered = vec![format!("ptr {environment}")];
        let flags = package.needs_drop(value).then(|| self.scratch_flags(value));
        let (mode, parameter_ty) = signature.params[0];
        if package.passes_by_reference(mode, parameter_ty) {
            rendered.push(format!("ptr {entry}"));
            if let Some(flags) = &flags {
                rendered.push(format!("ptr {flags}"));
            }
        } else {
            let value_ty = self.ty(value);
            let loaded = self.fresh();
            self.line(format!("{loaded} = load {value_ty}, ptr {entry}"));
            if mode == ParamMode::Borrow
                && !matches!(
                    package.types.kind(parameter_ty),
                    TypeKind::InterfaceView { .. }
                )
            {
                self.retain_at(&entry, value);
            }
            rendered.push(format!("{value_ty} {loaded}"));
        }
        let returned = self.emit_call(&code, &signature.results, &rendered);
        let mut combined = if signature.results.is_empty() {
            "true".to_string()
        } else {
            let combined = self.fresh();
            self.line(format!(
                "{combined} = insertvalue {result_ty} zeroinitializer, i1 true, 0"
            ));
            combined
        };
        if let Some((callback_ty, returned)) = returned {
            for (index, ty) in signature.results.iter().enumerate() {
                let result = if signature.results.len() == 1 {
                    returned.clone()
                } else {
                    let result = self.fresh();
                    self.line(format!(
                        "{result} = extractvalue {callback_ty} {returned}, {index}"
                    ));
                    result
                };
                let next = self.fresh();
                self.line(format!(
                    "{next} = insertvalue {result_ty} {combined}, {} {result}, {}",
                    self.ty(*ty),
                    index + 1
                ));
                combined = next;
            }
        }
        self.line(format!("store {result_ty} {combined}, ptr {result_slot}"));
        if mutable {
            let pending = self.fresh();
            self.line(format!("{pending} = call zeroext i1 @zore_panic_pending()"));
            let cleanup = self.label();
            self.line(format!("br i1 {pending}, label %{cleanup}, label %{done}"));
            self.out.push_str(&format!("{cleanup}:\n"));
            let value_ty = self.ty(value);
            let detached = self.zeroed_buffer(&value_ty);
            let removed = self.fresh();
            self.line(format!("{removed} = call zeroext i1 @zore_map_detach(ptr {map}, i32 {kind}, ptr {key}, ptr {detached})"));
            if let Some(flags) = flags {
                self.drop_value(&detached, value, &flags);
            }
        }
        self.line(format!("br label %{done}"));
        self.out.push_str(&format!("{done}:\n"));
        let result = self.fresh();
        self.line(format!("{result} = load {result_ty}, ptr {result_slot}"));
        (result_ty, result)
    }
}
