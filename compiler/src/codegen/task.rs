use super::llvm::{FunctionBuilder, Module};
use crate::mir::{Body, Operand};
use crate::types::TypeId;

/// A task's heap block holds its closure, then room for its results.
pub(super) struct TaskShape {
    pub(super) block_ty: String,
    pub(super) entry: String,
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
                self.task_code.push_str(&destructor);
                index
            }
        };
        TaskShape {
            block_ty,
            entry: format!("@zore_task_entry.{index}"),
            drop_results: format!("@zore_task_drop_results.{index}"),
        }
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
        text.push_str(
            "done:\n  call void @zore_enter_drop()\n  call void %destructor(ptr %env)\n  call void @zore_leave_drop()\n  ret void\n}\n\n",
        );
        text
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
    pub(super) fn spawn(&mut self, closure: &Operand) -> String {
        let package = self.module.package;
        let results = package
            .types
            .func_signature(self.operand_ty(closure))
            .expect("a spawned closure")
            .results
            .clone();
        let shape = self.module.task_shape(&results, self.body);
        let value = self.value(closure);
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
        self.line(format!(
            "{handle} = call ptr @zore_task_spawn(ptr {}, ptr {}, ptr {block}, i64 {size})",
            shape.entry, shape.drop_results
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
        self.line(format!(
            "{block} = call ptr @zore_task_wait(ptr {handle}, i64 {size})"
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
