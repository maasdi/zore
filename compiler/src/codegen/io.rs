use super::llvm::FunctionBuilder;
use crate::async_lowering::{NativeWait, native_wait};
use crate::mir::Terminator;
use crate::resolve::FunctionId;

impl FunctionBuilder<'_, '_> {
    pub(super) fn suspend_io(&mut self, state: usize, id: FunctionId, terminator: &Terminator) {
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
        let function = self.module.package.function(id);
        let returns = self.module.results_ty(&function.results);
        let name = format!("@\"{}.{}$io", self.module.package.name, function.name);
        let output = self.fresh();
        self.hoist_alloca(&output, &returns);
        let rendered = self.arguments(args);
        let operation = self.fresh();
        let helper = native_wait(&function.name) == Some(NativeWait::Helper);
        if helper {
            let types: Vec<_> = function
                .params
                .iter()
                .map(|param| self.ty(function.locals[param.0 as usize].ty))
                .chain(std::iter::once("ptr".into()))
                .collect();
            let layout = format!("{{ {} }}", types.join(", "));
            let storage = self.fresh();
            self.hoist_alloca(&storage, &layout);
            for (index, value) in rendered
                .iter()
                .chain(std::iter::once(&format!("ptr {output}")))
                .enumerate()
            {
                let field = self.fresh();
                self.line(format!(
                    "{field} = getelementptr inbounds {layout}, ptr {storage}, i32 0, i32 {index}"
                ));
                self.line(format!("store {value}, ptr {field}"));
            }
            self.line(format!("{operation} = call ptr @zore_blocking_start(ptr {name}$job\", ptr {storage}, ptr %context)"));
        } else {
            let mut rendered = rendered;
            rendered.push("ptr %context".into());
            self.line(format!(
                "{operation} = call ptr {name}$start\"({})",
                rendered.join(", ")
            ));
        }
        self.line(format!("store ptr {operation}, ptr %pending.slot"));
        self.line(format!("br label %resume.{state}"));
        self.out.push_str(&format!("resume.{state}:\n"));
        let operation = self.fresh();
        self.line(format!("{operation} = load ptr, ptr %pending.slot"));
        let ready = self.fresh();
        if helper {
            self.line(format!(
                "{ready} = call i8 @zore_blocking_poll(ptr {operation})"
            ));
        } else {
            self.line(format!(
                "{ready} = call i8 {name}$poll\"(ptr {operation}, ptr %context, ptr {output})"
            ));
        }
        let finished = self.fresh();
        self.line(format!("{finished} = icmp ne i8 {ready}, 0"));
        let (done, pending) = (self.label(), self.label());
        self.line(format!("br i1 {finished}, label %{done}, label %{pending}"));
        self.out.push_str(&format!("{pending}:\n"));
        self.line(format!("store i32 {}, ptr %frame", state + 1));
        self.line("ret i8 0");
        self.out.push_str(&format!("{done}:\n"));
        self.line("store ptr null, ptr %pending.slot");
        let result = self.fresh();
        self.line(format!("{result} = load {returns}, ptr {output}"));
        self.call_continuation(Some((returns, result)), destinations, *target, *unwind);
    }
}
