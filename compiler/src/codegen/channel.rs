use super::llvm::{FunctionBuilder, Module};
use crate::mir::{Body, Callee, Operand, SelectKind, Terminator};
use crate::types::{TypeId, TypeKind};

const RECORD: &str = "{ ptr, i8, ptr, i64, ptr, i8 }";
type Settlement = Vec<(usize, SelectKind, String, TypeId, usize)>;

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
                    polling: false,
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

    /// Fills one runtime record per case, performs the chosen case, then settles the values
    /// of the cases that were not chosen.
    pub(super) fn channel_select(
        &mut self,
        kinds: &[SelectKind],
        has_default: bool,
        args: &[Operand],
    ) -> (String, String) {
        let (records, settle) = self.prepare_select(kinds, args);
        let chosen = self.fresh();
        self.line(format!(
            "{chosen} = call i64 @zore_select(ptr {records}, i64 {}, i1 {has_default})",
            kinds.len()
        ));
        self.settle_select(&records, kinds.len(), settle, args, &chosen);
        ("i64".into(), chosen)
    }

    fn prepare_select(&mut self, kinds: &[SelectKind], args: &[Operand]) -> (String, Settlement) {
        let count = kinds.len();
        let records = self.fresh();
        self.hoist_alloca(&records, &format!("[{count} x {RECORD}]"));
        // Per case: the record address and the operands it was given.
        let mut cursor = 0;
        let mut settle = Vec::new();
        for (position, kind) in kinds.iter().enumerate() {
            let record = self.fresh();
            self.line(format!(
                "{record} = getelementptr inbounds [{count} x {RECORD}], ptr {records}, i64 0, i64 {position}"
            ));
            let case_cursor = cursor;
            let channel_operand = &args[cursor];
            let element = self.channel_element(channel_operand);
            let element_ty = self.ty(element);
            let channel = self.channel_handle(channel_operand);
            let size = self.byte_size(&element_ty, "1");
            let (value, destroyer, send) = match kind {
                SelectKind::Receive => {
                    let Operand::Ref(value_place) = &args[cursor + 1] else {
                        unreachable!("a received value is written through a reference")
                    };
                    cursor += 3;
                    let address = self.address(value_place);
                    (address, "null".to_string(), 0)
                }
                SelectKind::Send => {
                    let value = self.owned_value(&args[cursor + 1]);
                    cursor += 2;
                    let slot = self.fresh();
                    self.hoist_alloca(&slot, &element_ty);
                    self.line(format!("store {element_ty} {value}, ptr {slot}"));
                    let destroyer = self.module.channel_destroyer(element, self.body);
                    (slot, destroyer, 1)
                }
            };
            self.fill_case(&record, channel, send, &value, size, destroyer);
            settle.push((position, *kind, value, element, case_cursor));
        }
        (records, settle)
    }

    fn fill_case(
        &mut self,
        record: &str,
        channel: String,
        send: u8,
        value: &str,
        size: String,
        destroyer: String,
    ) {
        for (field, ty, text) in [
            (0, "ptr", channel),
            (1, "i8", send.to_string()),
            (2, "ptr", value.to_string()),
            (3, "i64", size),
            (4, "ptr", destroyer),
            (5, "i8", "0".to_string()),
        ] {
            let slot = self.fresh();
            self.line(format!(
                "{slot} = getelementptr inbounds {RECORD}, ptr {record}, i32 0, i32 {field}"
            ));
            self.line(format!("store {ty} {text}, ptr {slot}"));
        }
    }

    fn settle_select(
        &mut self,
        records: &str,
        count: usize,
        settle: Settlement,
        args: &[Operand],
        chosen: &str,
    ) {
        // §19.14 requires unchosen send values to drop in reverse source order.
        for (position, kind, value, element, cursor) in settle.into_iter().rev() {
            // Recompute on Ready; preparation temporaries do not dominate a resumed poll.
            let record = self.fresh();
            self.line(format!("{record} = getelementptr inbounds [{count} x {RECORD}], ptr {records}, i64 0, i64 {position}"));
            let was_chosen = self.fresh();
            self.line(format!("{was_chosen} = icmp eq i64 {chosen}, {position}"));
            let (taken, other, done) = (self.label(), self.label(), self.label());
            self.line(format!(
                "br i1 {was_chosen}, label %{taken}, label %{other}"
            ));
            match kind {
                SelectKind::Receive => {
                    let (Operand::Ref(value_place), Operand::Ref(flag_place)) =
                        (&args[cursor + 1], &args[cursor + 2])
                    else {
                        unreachable!("results are written through references")
                    };
                    self.out.push_str(&format!("{taken}:\n"));
                    let flag_slot = self.fresh();
                    self.line(format!(
                        "{flag_slot} = getelementptr inbounds {RECORD}, ptr {record}, i32 0, i32 5"
                    ));
                    let byte = self.fresh();
                    self.line(format!("{byte} = load i8, ptr {flag_slot}"));
                    let received = self.fresh();
                    self.line(format!("{received} = icmp ne i8 {byte}, 0"));
                    self.store(flag_place, &received);
                    self.line(format!("br label %{done}"));
                    self.out.push_str(&format!("{other}:\n"));
                    self.set_flags(value_place, false);
                    self.line(format!("br label %{done}"));
                }
                SelectKind::Send => {
                    self.out.push_str(&format!("{taken}:\n"));
                    self.line(format!("br label %{done}"));
                    self.out.push_str(&format!("{other}:\n"));
                    self.drop_unconditional(&value, element);
                    self.line(format!("br label %{done}"));
                }
            }
            self.out.push_str(&format!("{done}:\n"));
        }
    }

    /// Constructs channel records once, then resumes only the poll/settlement part.
    pub(super) fn suspend_channel(&mut self, state: usize, terminator: &Terminator) {
        let Terminator::Call {
            callee,
            args,
            destinations,
            target,
            unwind,
            ..
        } = terminator
        else {
            unreachable!()
        };
        let (records, count, has_default, settle, receive) = match callee {
            Callee::Select { cases, has_default } => {
                let (records, settle) = self.prepare_select(cases, args);
                (records, cases.len(), *has_default, settle, None)
            }
            Callee::ChannelSend | Callee::ChannelReceive => {
                let element = self.channel_element(&args[0]);
                let element_ty = self.ty(element);
                let channel = self.channel_handle(&args[0]);
                let slot = self.fresh();
                self.hoist_alloca(&slot, &element_ty);
                let send = matches!(callee, Callee::ChannelSend);
                let destroyer = if send {
                    let value = self.owned_value(&args[1]);
                    self.line(format!("store {element_ty} {value}, ptr {slot}"));
                    self.module.channel_destroyer(element, self.body)
                } else {
                    "null".into()
                };
                let records = self.fresh();
                self.hoist_alloca(&records, RECORD);
                let size = self.byte_size(&element_ty, "1");
                self.fill_case(&records, channel, u8::from(send), &slot, size, destroyer);
                let receive = (!send).then_some((slot, element_ty));
                (records, 1, false, Vec::new(), receive)
            }
            _ => unreachable!(),
        };
        let operation = self.fresh();
        self.line(format!("{operation} = call ptr @zore_channel_start(ptr {records}, i64 {count}, i1 {has_default}, ptr %context)"));
        self.line(format!("store ptr {operation}, ptr %pending.slot"));
        self.line(format!("br label %resume.{state}"));
        self.out.push_str(&format!("resume.{state}:\n"));
        let operation = self.fresh();
        self.line(format!("{operation} = load ptr, ptr %pending.slot"));
        let output = self.fresh();
        self.hoist_alloca(&output, "i64");
        let ready = self.fresh();
        self.line(format!(
            "{ready} = call i8 @zore_channel_poll(ptr {operation}, ptr %context, ptr {output})"
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
        let result = if matches!(callee, Callee::Select { .. }) {
            let chosen = self.fresh();
            self.line(format!("{chosen} = load i64, ptr {output}"));
            self.settle_select(&records, count, settle, args, &chosen);
            Some(("i64".into(), chosen))
        } else if let Some((slot, element_ty)) = receive {
            let flag_slot = self.fresh();
            self.line(format!(
                "{flag_slot} = getelementptr inbounds {RECORD}, ptr {records}, i32 0, i32 5"
            ));
            let flag = self.fresh();
            self.line(format!("{flag} = load i8, ptr {flag_slot}"));
            let received = self.fresh();
            self.line(format!("{received} = icmp ne i8 {flag}, 0"));
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
            Some((pair_ty, pair))
        } else {
            None
        };
        self.call_continuation(result, destinations, *target, *unwind);
    }
}
