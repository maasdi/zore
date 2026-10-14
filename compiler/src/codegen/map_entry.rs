use super::llvm::FunctionBuilder;
use crate::mir::{MapEntryGuard, Operand, Place};
use crate::types::TypeKind;

impl FunctionBuilder<'_, '_> {
    pub(super) fn map_entry_reference(
        &mut self,
        map: &Place,
        key: &Operand,
    ) -> (String, crate::types::TypeId) {
        let TypeKind::Map { key: key_ty, value } =
            self.module.package.types.kind(self.place_ty(map))
        else {
            unreachable!("an entry reference takes a map")
        };
        let slot = self.address(map);
        let loaded = self.fresh();
        self.line(format!("{loaded} = load ptr, ptr {slot}"));
        let (kind, key) = self.map_key(key, key_ty);
        let entry = self.fresh();
        self.line(format!(
            "{entry} = call ptr @zore_map_find(ptr {loaded}, i32 {kind}, ptr {key})"
        ));
        (entry, value)
    }

    pub(super) fn end_map_entry(&mut self, guard: &MapEntryGuard, unwind: bool) {
        if !guard.mutable {
            return;
        }
        if !unwind {
            self.line(format!("store ptr null, ptr %l{}", guard.entry.0));
            return;
        }
        let reference = self.fresh();
        self.line(format!("{reference} = load ptr, ptr %l{}", guard.entry.0));
        self.line(format!("store ptr null, ptr %l{}", guard.entry.0));
        let active = self.fresh();
        self.line(format!("{active} = icmp ne ptr {reference}, null"));
        let (cleanup, done) = (self.label(), self.label());
        self.line(format!("br i1 {active}, label %{cleanup}, label %{done}"));
        self.out.push_str(&format!("{cleanup}:\n"));
        let TypeKind::Map { key, value } =
            self.module.package.types.kind(self.place_ty(&guard.map))
        else {
            unreachable!("an entry guard holds a map")
        };
        let map_slot = self.address(&guard.map);
        let map = self.fresh();
        self.line(format!("{map} = load ptr, ptr {map_slot}"));
        let (kind, key) = self.map_key(&guard.key, key);
        let value_ty = self.ty(value);
        let detached = self.zeroed_buffer(&value_ty);
        let removed = self.fresh();
        self.line(format!("{removed} = call zeroext i1 @zore_map_detach(ptr {map}, i32 {kind}, ptr {key}, ptr {detached})"));
        if self.module.package.needs_drop(value) {
            let flags = self.fresh();
            self.line(format!("{flags} = load ptr, ptr %lf{}", guard.entry.0));
            self.drop_value(&detached, value, &flags);
        }
        self.line(format!("br label %{done}"));
        self.out.push_str(&format!("{done}:\n"));
    }
}
