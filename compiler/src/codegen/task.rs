use super::llvm::{FunctionBuilder, Module};
use crate::mir::{Body, Local, Operand, Place};
use crate::types::TypeId;

/// A task's heap block holds its closure, then room for its results.
pub(super) struct TaskShape {
    pub(super) block_ty: String,
    pub(super) entry: String,
    pub(super) poll_entry: String,
    pub(super) drop_results: String,
}

impl Module<'_> {
    fn results_slot_ty(&self, results: &[TypeId]) -> String {
        match results {
            [] => "{}".into(),
            _ => self.results_ty(results),
        }
    }

    /// One entry function and one result destructor serve every spawn with these results.
    pub(super) fn task_shape(&mut self, results: &[TypeId], body: &Body) -> TaskShape {
        let slot_ty = self.results_slot_ty(results);
        let block_ty = format!("{{ {{ ptr, ptr, ptr }}, {slot_ty} }}");
        let index = match self.task_shapes.get(results) {
            Some(&index) => index,
            None => {
                let index = self.task_shapes.len();
                self.task_shapes.insert(results.to_vec(), index);
                let entry = self.task_entry(index, results, &block_ty, &slot_ty);
                let destructor = self.task_drop_results(index, results, &block_ty, &slot_ty, body);
                self.task_code.push_str(&entry);
                let poll = self.task_poll_entry(index, results, &block_ty);
                self.task_code.push_str(&poll);
                self.task_code.push_str(&destructor);
                index
            }
        };
        TaskShape {
            block_ty,
            entry: format!("@zore_task_entry.{index}"),
            poll_entry: format!("@zore_task_poll_entry.{index}"),
            drop_results: format!("@zore_task_drop_results.{index}"),
        }
    }

    fn task_poll_entry(&self, index: usize, results: &[TypeId], block_ty: &str) -> String {
        let mut text = format!(
            "define private i8 @zore_task_poll_entry.{index}(ptr %block, ptr %context) {{\nentry:\n\
             \x20 %closure = load {{ ptr, ptr, ptr }}, ptr %block\n\
             \x20 %code = extractvalue {{ ptr, ptr, ptr }} %closure, 0\n\
             \x20 %frame = extractvalue {{ ptr, ptr, ptr }} %closure, 1\n\
             \x20 %destroy = extractvalue {{ ptr, ptr, ptr }} %closure, 2\n\
             \x20 %out = getelementptr inbounds {block_ty}, ptr %block, i32 0, i32 1\n\
             \x20 %status = call i8 %code(ptr %frame, ptr %context, ptr %out)\n\
             \x20 %ready = icmp ne i8 %status, 0\n\
             \x20 br i1 %ready, label %done, label %pending\npending:\n\
             \x20 ret i8 0\ndone:\n"
        );
        if results.iter().any(|&ty| self.package.needs_drop(ty)) {
            text.push_str(
                "  %body.panicked = call zeroext i1 @zore_panic_pending()\n  %returned = xor i1 %body.panicked, true\n",
            );
        }
        text.push_str("  call void %destroy(ptr %frame)\n");
        text.push_str(&self.task_result_cleanup(index, results));
        text.push_str("  ret i8 1\n}\n\n");
        text
    }

    /// Runs the closure once, keeps its results unless it panicked, then destroys what it still owns.
    fn task_entry(
        &self,
        index: usize,
        results: &[TypeId],
        block_ty: &str,
        slot_ty: &str,
    ) -> String {
        let returns = self.results_ty(results);
        let mut text = format!(
            "define private void @zore_task_entry.{index}(ptr %block) {{\nentry:\n\
             \x20 %closure = load {{ ptr, ptr, ptr }}, ptr %block\n\
             \x20 %code = extractvalue {{ ptr, ptr, ptr }} %closure, 0\n\
             \x20 %env = extractvalue {{ ptr, ptr, ptr }} %closure, 1\n\
             \x20 %destructor = extractvalue {{ ptr, ptr, ptr }} %closure, 2\n"
        );
        if results.is_empty() {
            text.push_str("  call void %code(ptr %env)\n  br label %done\n");
        } else {
            text.push_str(&format!(
                "  %result = call {returns} %code(ptr %env)\n\
                 \x20 %pending = call zeroext i1 @zore_panic_pending()\n\
                 \x20 br i1 %pending, label %done, label %keep\nkeep:\n\
                 \x20 %slot = getelementptr inbounds {block_ty}, ptr %block, i32 0, i32 1\n\
                 \x20 store {slot_ty} %result, ptr %slot\n\
                 \x20 br label %done\n"
            ));
        }
        text.push_str("done:\n");
        if results.iter().any(|&ty| self.package.needs_drop(ty)) {
            text.push_str("  %returned = phi i1 [ false, %entry ], [ true, %keep ]\n");
        }
        text.push_str("  call void @zore_enter_drop()\n  call void %destructor(ptr %env)\n  call void @zore_leave_drop()\n");
        text.push_str(&self.task_result_cleanup(index, results));
        text.push_str("  ret void\n}\n\n");
        text
    }

    fn task_result_cleanup(&self, index: usize, results: &[TypeId]) -> String {
        if !results.iter().any(|&ty| self.package.needs_drop(ty)) {
            return String::new();
        }
        format!(
            "  %cleanup.panicked = call zeroext i1 @zore_panic_pending()\n\
             \x20 %discard = and i1 %returned, %cleanup.panicked\n\
             \x20 br i1 %discard, label %discard.results, label %finished\ndiscard.results:\n\
             \x20 call void @zore_task_drop_results.{index}(ptr %block)\n\
             \x20 br label %finished\nfinished:\n"
        )
    }

    fn task_drop_results(
        &mut self,
        index: usize,
        results: &[TypeId],
        block_ty: &str,
        slot_ty: &str,
        body: &Body,
    ) -> String {
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
        let owned: Vec<(usize, TypeId)> = results
            .iter()
            .copied()
            .enumerate()
            .filter(|&(_, ty)| f.module.package.needs_drop(ty))
            .collect();
        if !owned.is_empty() {
            let slot = f.fresh();
            f.line(format!(
                "{slot} = getelementptr inbounds {block_ty}, ptr %block, i32 0, i32 1"
            ));
            f.line("call void @zore_enter_drop()");
            for &(position, ty) in owned.iter().rev() {
                let address = if results.len() == 1 {
                    slot.clone()
                } else {
                    let field = f.fresh();
                    f.line(format!(
                        "{field} = getelementptr inbounds {slot_ty}, ptr {slot}, i32 0, i32 {position}"
                    ));
                    field
                };
                f.drop_unconditional(&address, ty);
            }
            f.line("call void @zore_leave_drop()");
        }
        f.line("ret void");
        format!(
            "define private void @zore_task_drop_results.{index}(ptr %block) {{\nentry:\n{}{}}}\n\n",
            f.hoisted, f.out
        )
    }
}

impl FunctionBuilder<'_, '_> {
    /// Moves the closure into a new block and starts the task.
    pub(super) fn spawn(&mut self, closure: &Operand, scope: Option<Local>) -> String {
        let package = self.module.package;
        let results = package
            .types
            .func_signature(self.operand_ty(closure))
            .expect("a spawned closure")
            .results
            .clone();
        let shape = self.module.task_shape(&results, self.body);
        let thunk = match closure {
            Operand::Copy(place) | Operand::Move(place) => self
                .body
                .blocks
                .iter()
                .flat_map(|block| &block.statements)
                .find_map(|statement| match statement {
                    crate::mir::Statement::Assign {
                        place: destination,
                        rvalue: crate::mir::Rvalue::Closure { function, .. },
                        ..
                    } if destination == place
                        && self.module.machines.machines.contains_key(function) =>
                    {
                        Some(*function)
                    }
                    _ => None,
                }),
            _ => None,
        };
        let mut value = self.value(closure);
        if let Some(thunk) = thunk {
            let env = self.fresh();
            self.line(format!(
                "{env} = extractvalue {{ ptr, ptr, ptr }} {value}, 1"
            ));
            let frame = self.fresh();
            let constructor = self.module.async_name(thunk, "new");
            self.line(format!("{frame} = call ptr {constructor}(ptr {env})"));
            value = self.closure_triple(
                &self.module.async_name(thunk, "poll"),
                &frame,
                &self.module.async_name(thunk, "destroy"),
            );
        }
        let size = self.byte_size(&shape.block_ty, "1");
        let block = self.fresh();
        self.line(format!("{block} = call ptr @zore_alloc(i64 {size})"));
        self.line(format!("store {{ ptr, ptr, ptr }} {value}, ptr {block}"));
        if !results.is_empty() {
            let slot = self.fresh();
            self.line(format!(
                "{slot} = getelementptr inbounds {}, ptr {block}, i32 0, i32 1",
                shape.block_ty
            ));
            let slot_ty = self.module.results_ty(&results);
            self.line(format!("store {slot_ty} zeroinitializer, ptr {slot}"));
        }
        let handle = self.fresh();
        let (spawn, entry) = if thunk.is_some() {
            ("zore_task_spawn_poll", &shape.poll_entry)
        } else {
            ("zore_task_spawn", &shape.entry)
        };
        let (spawn, prefix) = if let Some(scope) = scope {
            let value = self.value(&Operand::Copy(Place::local(scope)));
            let pointer = self.fresh();
            self.line(format!("{pointer} = inttoptr i64 {value} to ptr"));
            (
                if thunk.is_some() {
                    "zore_scope_spawn_poll"
                } else {
                    "zore_scope_spawn"
                },
                format!("ptr {pointer}, "),
            )
        } else {
            (spawn, String::new())
        };
        self.line(format!(
            "{handle} = call ptr @{spawn}({prefix}ptr {entry}, ptr {}, ptr {block}, i64 {size})",
            shape.drop_results
        ));
        handle
    }

    /// The task's results, or zeroes when the wait raised a panic.
    pub(super) fn task_wait(&mut self, task: &Operand) -> Option<(String, String)> {
        let package = self.module.package;
        let results = package
            .types
            .task_results(self.operand_ty(task))
            .expect("a task operand")
            .to_vec();
        let slot_ty = self.module.results_slot_ty(&results);
        let block_ty = format!("{{ {{ ptr, ptr, ptr }}, {slot_ty} }}");
        let handle = self.value(task);
        let size = self.byte_size(&block_ty, "1");
        let block = self.fresh();
        let wait = if matches!(
            package.types.kind(self.operand_ty(task)),
            crate::types::TypeKind::ScopedTask(_, _)
        ) {
            "zore_scope_wait"
        } else {
            "zore_task_wait"
        };
        self.line(format!(
            "{block} = call ptr @{wait}(ptr {handle}, i64 {size})"
        ));
        let loaded = (!results.is_empty()).then(|| {
            let slot = self.fresh();
            self.line(format!(
                "{slot} = getelementptr inbounds {block_ty}, ptr {block}, i32 0, i32 1"
            ));
            let value = self.fresh();
            self.line(format!("{value} = load {slot_ty}, ptr {slot}"));
            (slot_ty, value)
        });
        self.line(format!("call void @zore_free(ptr {block}, i64 {size})"));
        loaded
    }
}
