use super::llvm::FunctionBuilder;
use crate::mir::{Operand, Terminator};
use crate::types::{TypeId, TypeKind};

impl FunctionBuilder<'_, '_> {
    fn mutex_element(&self, operand: &Operand) -> TypeId {
        let Operand::Ref(place) = operand else {
            unreachable!("mutex operations borrow their mutex")
        };
        match self.module.package.types.kind(self.place_ty(place)) {
            TypeKind::Mutex { element } => element,
            _ => unreachable!("mutex operations take a mutex"),
        }
    }

    fn mutex_handle(&mut self, operand: &Operand) -> String {
        let Operand::Ref(place) = operand else {
            unreachable!("mutex operations borrow their mutex")
        };
        let address = self.address(place);
        let handle = self.fresh();
        self.line(format!("{handle} = load ptr, ptr {address}"));
        handle
    }

    pub(super) fn mutex_new(&mut self, element: TypeId, value: &Operand) -> (String, String) {
        let element_ty = self.ty(element);
        let value = self.owned_value(value);
        let slot = self.fresh();
        self.hoist_alloca(&slot, &element_ty);
        self.line(format!("store {element_ty} {value}, ptr {slot}"));
        let size = self.byte_size(&element_ty, "1");
        let destroyer = self.module.channel_destroyer(element, self.body);
        let mutex = self.fresh();
        self.line(format!(
            "{mutex} = call ptr @zore_mutex_new(i64 {size}, ptr {destroyer}, ptr {slot})"
        ));
        ("ptr".into(), mutex)
    }

    pub(super) fn mutex_is_poisoned(&mut self, mutex: &Operand) -> (String, String) {
        let handle = self.mutex_handle(mutex);
        let poisoned = self.fresh();
        self.line(format!(
            "{poisoned} = call zeroext i1 @zore_mutex_is_poisoned(ptr {handle})"
        ));
        ("i1".into(), poisoned)
    }

    pub(super) fn mutex_with_lock(&mut self, args: &[Operand]) -> Option<(String, String)> {
        let handle = self.mutex_handle(&args[0]);
        let value = self.fresh();
        self.line(format!("{value} = call ptr @zore_mutex_lock(ptr {handle})"));
        self.mutex_callback(args, &handle, &value)
    }

    pub(super) fn suspend_mutex(&mut self, state: usize, terminator: &Terminator) {
        let Terminator::Call {
            args,
            destinations,
            target,
            unwind,
            ..
        } = terminator
        else {
            unreachable!()
        };
        let handle = self.mutex_handle(&args[0]);
        let operation = self.fresh();
        self.line(format!(
            "{operation} = call ptr @zore_mutex_start(ptr {handle}, ptr %context)"
        ));
        self.line(format!("store ptr {operation}, ptr %pending.slot"));
        self.line(format!("br label %resume.{state}"));
        self.out.push_str(&format!("resume.{state}:\n"));
        let operation = self.fresh();
        self.line(format!("{operation} = load ptr, ptr %pending.slot"));
        let output = self.fresh();
        self.hoist_alloca(&output, "ptr");
        let ready = self.fresh();
        self.line(format!(
            "{ready} = call i8 @zore_mutex_poll(ptr {operation}, ptr %context, ptr {output})"
        ));
        let finished = self.fresh();
        self.line(format!("{finished} = icmp ne i8 {ready}, 0"));
        let (done, pending) = (self.label(), self.label());
        self.line(format!("br i1 {finished}, label %{done}, label %{pending}"));
        self.out.push_str(&format!("{pending}:\n"));
        self.line(format!("store i32 {}, ptr %frame", state + 1));
        self.line("ret i8 0");
        self.out.push_str(&format!("{done}:\n"));
        self.line("store ptr null, ptr %pending.slot");
        let value = self.fresh();
        self.line(format!("{value} = load ptr, ptr {output}"));
        let handle = self.mutex_handle(&args[0]);
        let result = self.mutex_callback(args, &handle, &value);
        self.call_continuation(result, destinations, *target, *unwind);
    }

    fn mutex_callback(
        &mut self,
        args: &[Operand],
        handle: &str,
        value: &str,
    ) -> Option<(String, String)> {
        let package = self.module.package;
        let element = self.mutex_element(&args[0]);
        let Operand::Ref(callback) = &args[1] else {
            unreachable!("the callback is borrowed")
        };
        let results = package
            .types
            .func_signature(self.place_ty(callback))
            .expect("a function-typed callback")
            .results
            .clone();
        let address = self.address(callback);
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
        let returns = self.module.results_ty(&results);
        let slot = (!results.is_empty()).then(|| {
            let slot = self.fresh();
            self.hoist_alloca(&slot, &returns);
            slot
        });
        if let Some(slot) = &slot {
            self.line(format!("store {returns} zeroinitializer, ptr {slot}"));
        }
        let acquired = self.fresh();
        self.line(format!("{acquired} = icmp ne ptr {value}, null"));
        let (run, done) = (self.label(), self.label());
        self.line(format!("br i1 {acquired}, label %{run}, label %{done}"));
        self.out.push_str(&format!("{run}:\n"));
        let mut rendered = vec![format!("ptr {environment}")];
        if let crate::types::TypeKind::Interface(_) = package.types.kind(element) {
            // A borrowed interface value has the same parts as the owned one.
            let ty = self.ty(element);
            let parts = self.fresh();
            self.line(format!("{parts} = load {ty}, ptr {value}"));
            rendered.push(format!("{ty} {parts}"));
        } else {
            rendered.push(format!("ptr {value}"));
            if package.needs_drop(element) {
                let flags = self.scratch_flags(element);
                rendered.push(format!("ptr {flags}"));
            }
        }
        let call = format!("{code}({})", rendered.join(", "));
        match &slot {
            Some(slot) => {
                let result = self.fresh();
                self.line(format!("{result} = call {returns} {call}"));
                self.line(format!("store {returns} {result}, ptr {slot}"));
            }
            None => self.line(format!("call void {call}")),
        }
        let pending = self.fresh();
        self.line(format!("{pending} = call zeroext i1 @zore_panic_pending()"));
        self.line(format!(
            "call void @zore_mutex_unlock(ptr {handle}, i1 zeroext {pending})"
        ));
        self.line(format!("br label %{done}"));
        self.out.push_str(&format!("{done}:\n"));
        slot.map(|slot| {
            let result = self.fresh();
            self.line(format!("{result} = load {returns}, ptr {slot}"));
            (returns, result)
        })
    }
}
