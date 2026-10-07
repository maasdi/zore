use super::llvm::{FunctionBuilder, Module};
use crate::mir::{Body, Operand};
use crate::types::{TypeId, TypeKind};

impl Module<'_> {
    /// The function that destroys one queued value of the type, or `null` when there is nothing to do.
    pub(super) fn channel_destroyer(&mut self, element: TypeId, body: &Body) -> String {
        if !self.package.needs_drop(element) {
            return "null".into();
        }
        let index = match self.channel_destroyers.get(&element) {
            Some(&index) => index,
            None => {
                let index = self.channel_destroyers.len();
                self.channel_destroyers.insert(element, index);
                let mut f = FunctionBuilder {
                    module: self,
                    body,
                    out: String::new(),
                    next: 0,
                    active_unwind: None,
                    emitting_unwind: false,
                    drop_check_after_store: false,
                    hoisted: String::new(),
                };
                f.line("call void @zore_enter_drop()");
                f.drop_unconditional("%value", element);
                f.line("call void @zore_leave_drop()");
                f.line("ret void");
                let text = format!(
                    "define private void @zore_channel_destroy.{index}(ptr %value) {{\nentry:\n{}{}}}\n\n",
                    f.hoisted, f.out
                );
                self.task_code.push_str(&text);
                index
            }
        };
        format!("@zore_channel_destroy.{index}")
    }
}

impl FunctionBuilder<'_, '_> {
    fn channel_element(&self, operand: &Operand) -> TypeId {
        let Operand::Ref(place) = operand else {
            unreachable!("channel operations borrow their channel")
        };
        match self.module.package.types.kind(self.place_ty(place)) {
            TypeKind::Channel { element } => element,
            _ => unreachable!("channel operations take a channel"),
        }
    }

    fn channel_handle(&mut self, operand: &Operand) -> String {
        let Operand::Ref(place) = operand else {
            unreachable!("channel operations borrow their channel")
        };
        let address = self.address(place);
        let handle = self.fresh();
        self.line(format!("{handle} = load ptr, ptr {address}"));
        handle
    }

    pub(super) fn channel_make(&mut self, element: TypeId, capacity: &Operand) -> (String, String) {
        let capacity = self.value(capacity);
        let element_ty = self.ty(element);
        let size = self.byte_size(&element_ty, "1");
        let destroyer = self.module.channel_destroyer(element, self.body);
        let channel = self.fresh();
        self.line(format!(
            "{channel} = call ptr @zore_channel_make(i64 {size}, i64 {capacity}, ptr {destroyer})"
        ));
        ("ptr".into(), channel)
    }

    pub(super) fn channel_send(&mut self, args: &[Operand]) {
        let element = self.channel_element(&args[0]);
        let element_ty = self.ty(element);
        let channel = self.channel_handle(&args[0]);
        let value = self.owned_value(&args[1]);
        let slot = self.fresh();
        self.hoist_alloca(&slot, &element_ty);
        self.line(format!("store {element_ty} {value}, ptr {slot}"));
        let destroyer = self.module.channel_destroyer(element, self.body);
        self.line(format!(
            "call void @zore_channel_send(ptr {channel}, ptr {slot}, ptr {destroyer})"
        ));
    }

    pub(super) fn channel_receive(&mut self, args: &[Operand]) -> (String, String) {
        let element = self.channel_element(&args[0]);
        let element_ty = self.ty(element);
        let channel = self.channel_handle(&args[0]);
        let slot = self.fresh();
        self.hoist_alloca(&slot, &element_ty);
        let size = self.byte_size(&element_ty, "1");
        let received = self.fresh();
        self.line(format!(
            "{received} = call zeroext i1 @zore_channel_receive(ptr {channel}, ptr {slot}, i64 {size})"
        ));
        let value = self.fresh();
        self.line(format!("{value} = load {element_ty}, ptr {slot}"));
        let pair_ty = format!("{{ {element_ty}, i1 }}");
        let first = self.fresh();
        self.line(format!(
            "{first} = insertvalue {pair_ty} undef, {element_ty} {value}, 0"
        ));
        let pair = self.fresh();
        self.line(format!(
            "{pair} = insertvalue {pair_ty} {first}, i1 {received}, 1"
        ));
        (pair_ty, pair)
    }

    pub(super) fn channel_close(&mut self, args: &[Operand]) {
        let channel = self.channel_handle(&args[0]);
        self.line(format!("call void @zore_channel_close(ptr {channel})"));
    }
}
