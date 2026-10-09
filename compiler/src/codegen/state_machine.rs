use std::fmt::Write;

use super::llvm::{FunctionBuilder, Module};
use crate::async_lowering::{LocalStorage, Suspension};
use crate::mir::{self, Callee, Operand, Place, Terminator};
use crate::resolve::FunctionId;

pub(super) struct Frame {
    pub(super) name: String,
    pub(super) fields: Vec<String>,
    locals: Vec<(Option<usize>, Option<usize>)>,
}

impl Module<'_> {
    pub(super) fn async_name(&self, id: FunctionId, suffix: &str) -> String {
        format!(
            "@\"{}.{}$async${suffix}\"",
            self.package.name,
            self.package.function(id).name
        )
    }

    pub(super) fn async_function(&mut self, body: &mir::Body) -> String {
        let mut fields = vec![
            "i32".into(),
            "ptr".into(),
            "ptr".into(),
            "ptr".into(),
            "ptr".into(),
        ];
        let mut locals = Vec::new();
        let machine = &self.machines.machines[&body.function];
        let storage = &machine.storage;
        let frame_reuse = &machine.frame_reuse;
        let mut shared_slots = vec![None; body.locals.len()];
        for (index, local) in body.locals.iter().enumerate() {
            let slot = (storage[index] == LocalStorage::Frame).then(|| {
                let owner = frame_reuse[index];
                if let Some(field) = shared_slots[owner] {
                    field
                } else {
                    let field = fields.len();
                    fields.push(if local.by_reference {
                        "ptr".into()
                    } else {
                        self.ty(local.ty)
                    });
                    shared_slots[owner] = Some(field);
                    field
                }
            });
            let flags = if slot.is_some() && self.package.needs_drop(local.ty) {
                let field = fields.len();
                fields.push(if local.by_reference {
                    "ptr".into()
                } else {
                    self.flag_ty(local.ty)
                });
                Some(field)
            } else {
                None
            };
            locals.push((slot, flags));
        }
        self.frames.insert(
            body.function,
            Frame {
                name: format!("%\"{}.AsyncFrame.{}\"", self.package.name, body.function.0),
                fields,
                locals,
            },
        );
        let mut f = FunctionBuilder {
            module: self,
            body,
            out: String::new(),
            next: 0,
            active_unwind: None,
            emitting_unwind: false,
            drop_check_after_store: false,
            hoisted: String::new(),
            polling: true,
        };
        f.emit_poll();
        let poll = std::mem::take(&mut f.out);
        // Emit poll first to discover scratch fields before initializing the frame.
        f.polling = false;
        f.emit_constructor();
        let constructor = std::mem::take(&mut f.out);
        f.emit_frame_destroy();
        let destroy = std::mem::take(&mut f.out);
        let frame = self.frames.get(&body.function).unwrap();
        writeln!(
            self.frame_types,
            "{} = type {{ {} }}",
            frame.name,
            frame.fields.join(", ")
        )
        .unwrap();
        format!("{constructor}{poll}{destroy}")
    }
}

impl FunctionBuilder<'_, '_> {
    fn frame_slots(&mut self) {
        let frame = self.module.frames.get(&self.body.function).unwrap();
        let name = frame.name.clone();
        let locals = frame.locals.clone();
        for (index, (slot, flags)) in locals.iter().enumerate() {
            if let Some(slot) = slot {
                self.line(format!(
                    "%l{index} = getelementptr inbounds {name}, ptr %frame, i32 0, i32 {slot}"
                ));
            } else if self.polling {
                let ty = if self.body.locals[index].by_reference {
                    "ptr".into()
                } else {
                    self.module.ty(self.body.locals[index].ty)
                };
                self.line(format!("%l{index} = alloca {ty}"));
            }
            if let Some(flags) = flags {
                self.line(format!(
                    "%lf{index} = getelementptr inbounds {name}, ptr %frame, i32 0, i32 {flags}"
                ));
            } else if self.polling && self.module.package.needs_drop(self.body.locals[index].ty) {
                let ty = if self.body.locals[index].by_reference {
                    "ptr".into()
                } else {
                    self.module.flag_ty(self.body.locals[index].ty)
                };
                self.line(format!("%lf{index} = alloca {ty}"));
                self.line(format!("store {ty} zeroinitializer, ptr %lf{index}"));
            }
        }
    }

    fn emit_constructor(&mut self) {
        let function = self.module.package.function(self.body.function);
        let mut params = Vec::new();
        if function.is_closure {
            params.push("ptr %env".into());
        }
        for &param in &self.body.params {
            params.push(format!("{} %p{}", self.slot_ty(param), param.0));
            if self.body.locals[param.0 as usize].by_reference
                && self.module.package.needs_drop(self.local_ty(param))
            {
                params.push(format!("ptr %pf{}", param.0));
            }
        }
        let name = self.module.async_name(self.body.function, "new");
        self.out.push_str(&format!(
            "define private ptr {name}({}) {{\nentry:\n",
            params.join(", ")
        ));
        let frame_ty = self.module.frames[&self.body.function].name.clone();
        let size = self.byte_size(&frame_ty, "1");
        self.line(format!("%frame = call ptr @zore_alloc(i64 {size})"));
        self.line(format!("store {frame_ty} zeroinitializer, ptr %frame"));
        for (field, suffix) in [(3, "poll"), (4, "destroy")] {
            self.line(format!(
                "%header.{suffix} = getelementptr inbounds {frame_ty}, ptr %frame, i32 0, i32 {field}"
            ));
            let function = self.module.async_name(self.body.function, suffix);
            self.line(format!("store ptr {function}, ptr %header.{suffix}"));
        }
        self.frame_slots();
        for &param in &self.body.params {
            self.line(format!(
                "store {} %p{}, ptr %l{}",
                self.slot_ty(param),
                param.0,
                param.0
            ));
            if self.module.package.needs_drop(self.local_ty(param)) {
                if self.body.locals[param.0 as usize].by_reference {
                    self.line(format!("store ptr %pf{}, ptr %lf{}", param.0, param.0));
                } else {
                    self.set_flags(&Place::local(param), true);
                }
            }
        }
        if function.is_closure {
            self.line(format!(
                "%env.slot = getelementptr inbounds {frame_ty}, ptr %frame, i32 0, i32 2"
            ));
            self.line("store ptr %env, ptr %env.slot");
            self.load_captures();
        }
        self.line("ret ptr %frame");
        self.out.push_str("}\n\n");
    }

    fn emit_frame_destroy(&mut self) {
        let name = self.module.async_name(self.body.function, "destroy");
        let frame_ty = self.module.frames[&self.body.function].name.clone();
        self.out.push_str(&format!(
            "define private void {name}(ptr %frame) {{\nentry:\n"
        ));
        if self.module.owning_closures.contains(&self.body.function) {
            self.line(format!(
                "%env.slot = getelementptr inbounds {frame_ty}, ptr %frame, i32 0, i32 2"
            ));
            self.line("%env = load ptr, ptr %env.slot");
            let drop = self.module.environment_drop_name(self.body.function);
            self.line("call void @zore_enter_drop()");
            self.line(format!("call void {drop}(ptr %env)"));
            self.line("call void @zore_leave_drop()");
        }
        let size = self.byte_size(&frame_ty, "1");
        self.line(format!("call void @zore_free(ptr %frame, i64 {size})"));
        self.line("ret void");
        self.out.push_str("}\n\n");
    }

    fn emit_poll(&mut self) {
        let name = self.module.async_name(self.body.function, "poll");
        self.out.push_str(&format!(
            "define private i8 {name}(ptr %frame, ptr %context, ptr %out) {{\nentry:\n"
        ));
        self.frame_slots();
        let frame_ty = self.module.frames[&self.body.function].name.clone();
        self.line(format!(
            "%pending.slot = getelementptr inbounds {frame_ty}, ptr %frame, i32 0, i32 1"
        ));
        let entry_end = self.out.len();
        self.line("%state = load i32, ptr %frame");
        self.line("%new.frame = icmp eq i32 %state, 0");
        let (check_budget, dispatch) = (self.label(), self.label());
        self.line(format!(
            "br i1 %new.frame, label %{check_budget}, label %{dispatch}"
        ));
        self.out.push_str(&format!("{check_budget}:\n"));
        self.budget_step(None);
        self.line(format!("br label %{dispatch}"));
        self.out.push_str(&format!("{dispatch}:\n"));
        self.line("switch i32 %state, label %invalid [ i32 0, label %bb0");
        let machine = &self.module.machines.machines[&self.body.function];
        let suspensions = machine.suspensions.clone();
        let budget_blocks = machine.budget_blocks.clone();
        for (index, _) in suspensions.iter().enumerate() {
            self.line(format!("i32 {}, label %resume.{index}", index + 1));
        }
        for (index, block) in budget_blocks.iter().enumerate() {
            self.line(format!(
                "i32 {}, label %bb{}",
                suspensions.len() + index + 1,
                block.0
            ));
        }
        self.line("]");
        self.out.push_str("invalid:\n  unreachable\n");
        for (index, block) in self.body.blocks.iter().enumerate() {
            self.emitting_unwind = self.body.unwind == Some(mir::BlockId(index as u32));
            self.out.push_str(&format!("bb{index}:\n"));
            if let Some(state) = budget_blocks
                .iter()
                .position(|block| block.0 as usize == index)
            {
                self.budget_step(Some(suspensions.len() + state + 1));
            }
            for statement in &block.statements {
                self.statement(statement);
            }
            if let Some((state, (_, kind))) = suspensions
                .iter()
                .enumerate()
                .find(|(_, (id, _))| id.0 as usize == index)
            {
                self.suspend_call(state, *kind, &block.terminator);
            } else {
                self.terminator(&block.terminator);
            }
        }
        self.out
            .insert_str(entry_end, &std::mem::take(&mut self.hoisted));
        self.out.push_str("}\n\n");
    }

    fn budget_step(&mut self, state: Option<usize>) {
        let remaining = self.fresh();
        self.line(format!("{remaining} = load i16, ptr %context"));
        let has_budget = self.fresh();
        self.line(format!("{has_budget} = icmp ne i16 {remaining}, 0"));
        let (proceed, exhausted) = (self.label(), self.label());
        self.line(format!(
            "br i1 {has_budget}, label %{proceed}, label %{exhausted}"
        ));
        self.out.push_str(&format!("{exhausted}:\n"));
        self.line("call void @zore_budget_yield(ptr %context)");
        if let Some(state) = state {
            self.line(format!("store i32 {state}, ptr %frame"));
        }
        self.line("ret i8 0");
        self.out.push_str(&format!("{proceed}:\n"));
        let next = self.fresh();
        self.line(format!("{next} = sub i16 {remaining}, 1"));
        self.line(format!("store i16 {next}, ptr %context"));
    }

    /// The poll (3) or destroy (4) function that every frame records at its start.
    fn frame_header_pointer(&mut self, frame: &str, field: usize) -> String {
        let slot = self.fresh();
        self.line(format!(
            "{slot} = getelementptr inbounds {{ i32, ptr, ptr, ptr, ptr }}, ptr {frame}, i32 0, i32 {field}"
        ));
        let function = self.fresh();
        self.line(format!("{function} = load ptr, ptr {slot}"));
        function
    }

    fn suspend_call(&mut self, state: usize, kind: Suspension, terminator: &Terminator) {
        if let Suspension::Io(id) = kind {
            self.suspend_io(state, id, terminator);
            return;
        }
        if kind == Suspension::Mutex {
            self.suspend_mutex(state, terminator);
            return;
        }
        if kind == Suspension::Channel {
            self.suspend_channel(state, terminator);
            return;
        }
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
        if kind == Suspension::Sleep {
            self.suspend_sleep(state, args, destinations, *target, *unwind);
            return;
        }
        let start = match kind {
            Suspension::Call(id) => {
                let args = self.arguments(args);
                let name = self.module.async_name(id, "new");
                let child = self.fresh();
                self.line(format!("{child} = call ptr {name}({})", args.join(", ")));
                child
            }
            Suspension::Task => self.value(&args[0]),
            Suspension::Value => {
                let Terminator::Call {
                    callee: Callee::Value(place),
                    ..
                } = terminator
                else {
                    unreachable!()
                };
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
                let child = self.fresh();
                self.line(format!(
                    "{child} = call ptr {code}({})",
                    rendered.join(", ")
                ));
                child
            }
            Suspension::Channel | Suspension::Sleep | Suspension::Mutex | Suspension::Io(_) => {
                unreachable!()
            }
        };
        self.line(format!("store ptr {start}, ptr %pending.slot"));
        self.line(format!("br label %resume.{state}"));
        self.out.push_str(&format!("resume.{state}:\n"));
        let child = self.fresh();
        self.line(format!("{child} = load ptr, ptr %pending.slot"));
        let results = match kind {
            Suspension::Call(id) => self.module.package.function(id).results.clone(),
            Suspension::Value => {
                let Terminator::Call {
                    callee: Callee::Value(place),
                    ..
                } = terminator
                else {
                    unreachable!()
                };
                self.module
                    .package
                    .types
                    .func_signature(self.place_ty(place))
                    .expect("a function-typed callee")
                    .results
                    .clone()
            }
            Suspension::Task => self
                .module
                .package
                .types
                .task_results(self.operand_ty(&args[0]))
                .unwrap()
                .to_vec(),
            Suspension::Channel | Suspension::Sleep | Suspension::Mutex | Suspension::Io(_) => {
                unreachable!()
            }
        };
        let result_ty = if results.is_empty() {
            "{}".to_string()
        } else {
            self.module.results_ty(&results)
        };
        let output = self.fresh();
        self.hoist_alloca(
            &output,
            if kind == Suspension::Task {
                "ptr"
            } else {
                &result_ty
            },
        );
        let ready = self.fresh();
        let block_ty = format!("{{ {{ ptr, ptr, ptr }}, {result_ty} }}");
        match kind {
            Suspension::Call(id) => {
                let poll = self.module.async_name(id, "poll");
                self.line(format!(
                    "{ready} = call i8 {poll}(ptr {child}, ptr %context, ptr {output})"
                ));
            }
            Suspension::Value => {
                let poll = self.frame_header_pointer(&child, 3);
                self.line(format!(
                    "{ready} = call i8 {poll}(ptr {child}, ptr %context, ptr {output})"
                ));
            }
            Suspension::Task => {
                let size = self.byte_size(&block_ty, "1");
                self.line(format!("{ready} = call i8 @zore_task_poll(ptr {child}, ptr %context, i64 {size}, ptr {output})"));
            }
            Suspension::Channel | Suspension::Sleep | Suspension::Mutex | Suspension::Io(_) => {
                unreachable!()
            }
        }
        let finished = self.fresh();
        self.line(format!("{finished} = icmp ne i8 {ready}, 0"));
        let pending = self.label();
        let done = self.label();
        self.line(format!("br i1 {finished}, label %{done}, label %{pending}"));
        self.out.push_str(&format!("{pending}:\n"));
        self.line(format!("store i32 {}, ptr %frame", state + 1));
        self.line("ret i8 0");
        self.out.push_str(&format!("{done}:\n"));
        let slot = match kind {
            Suspension::Call(id) => {
                let destroy = self.module.async_name(id, "destroy");
                self.line(format!("call void {destroy}(ptr {child})"));
                output
            }
            Suspension::Value => {
                let destroy = self.frame_header_pointer(&child, 4);
                self.line(format!("call void {destroy}(ptr {child})"));
                output
            }
            Suspension::Task => {
                let block = self.fresh();
                self.line(format!("{block} = load ptr, ptr {output}"));
                let slot = self.fresh();
                self.line(format!(
                    "{slot} = getelementptr inbounds {block_ty}, ptr {block}, i32 0, i32 1"
                ));
                let loaded = self.fresh();
                self.line(format!("{loaded} = load {result_ty}, ptr {slot}"));
                let size = self.byte_size(&block_ty, "1");
                self.line(format!("call void @zore_free(ptr {block}, i64 {size})"));
                self.line("store ptr null, ptr %pending.slot");
                let result = (!results.is_empty()).then_some((result_ty, loaded));
                self.call_continuation(result, destinations, *target, *unwind);
                return;
            }
            Suspension::Channel | Suspension::Sleep | Suspension::Mutex | Suspension::Io(_) => {
                unreachable!()
            }
        };
        self.line("store ptr null, ptr %pending.slot");
        let result = (!results.is_empty()).then(|| {
            let loaded = self.fresh();
            self.line(format!("{loaded} = load {result_ty}, ptr {slot}"));
            (result_ty, loaded)
        });
        self.call_continuation(result, destinations, *target, *unwind);
    }

    fn suspend_sleep(
        &mut self,
        state: usize,
        args: &[Operand],
        destinations: &[Option<Place>],
        target: mir::BlockId,
        unwind: Option<mir::BlockId>,
    ) {
        let milliseconds = self.value(&args[0]);
        let operation = self.fresh();
        self.line(format!(
            "{operation} = call ptr @zore_native_time_sleep_start(i64 {milliseconds}, ptr %context)"
        ));
        self.line(format!("store ptr {operation}, ptr %pending.slot"));
        self.line(format!("br label %resume.{state}"));
        self.out.push_str(&format!("resume.{state}:\n"));
        let operation = self.fresh();
        self.line(format!("{operation} = load ptr, ptr %pending.slot"));
        let ready = self.fresh();
        self.line(format!(
            "{ready} = call i8 @zore_reactor_poll(ptr {operation})"
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
        self.call_continuation(None, destinations, target, unwind);
    }

    pub(super) fn async_terminator(&mut self, terminator: &Terminator) -> bool {
        match terminator {
            Terminator::Return => {
                let results = &self.module.package.function(self.body.function).results;
                if let [one] = &self.body.returns[..] {
                    let value = self.value(&Operand::Copy(Place::local(*one)));
                    self.line(format!("store {} {value}, ptr %out", self.ty(results[0])));
                } else if !self.body.returns.is_empty() {
                    let ty = self.module.results_ty(results);
                    let mut current = "undef".to_string();
                    for (index, &local) in self.body.returns.iter().enumerate() {
                        let value = self.value(&Operand::Copy(Place::local(local)));
                        let next = self.fresh();
                        self.line(format!(
                            "{next} = insertvalue {ty} {current}, {} {value}, {index}",
                            self.ty(self.local_ty(local))
                        ));
                        current = next;
                    }
                    self.line(format!("store {ty} {current}, ptr %out"));
                }
            }
            Terminator::PanicReturn => {
                let results = &self.module.package.function(self.body.function).results;
                if !results.is_empty() {
                    self.line(format!(
                        "store {} zeroinitializer, ptr %out",
                        self.module.results_ty(results)
                    ));
                }
            }
            _ => return false,
        }
        self.line("store i32 -1, ptr %frame");
        self.line("ret i8 1");
        true
    }
}
