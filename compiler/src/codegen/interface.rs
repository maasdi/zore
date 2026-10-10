use std::fmt::Write;

use super::llvm::{FunctionBuilder, Module};
use crate::ast::ParamMode;
use crate::hir::Implementation;
use crate::mir::{self, Operand, Place};
use crate::resolve::{FunctionId, LocalKind};
use crate::source::Span;
use crate::types::{InterfaceId, TypeId, TypeKind};

/// The value inside, its drop flags, and the method table.
pub(super) const INTERFACE_TY: &str = "{ ptr, ptr, ptr }";

/// How an adapter reaches the method that serves its entry.
enum Target {
    Method {
        function: FunctionId,
        symbol: String,
        receiver: ParamMode,
        by_reference: bool,
        results: Vec<TypeId>,
    },
    Entry {
        index: usize,
        receiver: ParamMode,
        /// The source interface's entries; its start adapters follow its plain ones.
        count: usize,
    },
}

impl Target {
    fn receiver(&self) -> ParamMode {
        match self {
            Target::Method { receiver, .. } | Target::Entry { receiver, .. } => *receiver,
        }
    }
}

/// Frames begin with the state, the pending operation, the environment, then poll and destroy.
const FRAME_HEADER: &str = "i32, ptr, ptr, ptr, ptr";

impl<'a> Module<'a> {
    /// Owned storage for a value converted to an interface: the value, then its drop flags.
    fn box_ty(&self, ty: TypeId) -> String {
        format!("{{ {}, {} }}", self.ty(ty), self.flag_ty(ty))
    }

    fn helper<'m>(&'m mut self, body: &'m mir::Body) -> FunctionBuilder<'m, 'a> {
        FunctionBuilder {
            module: self,
            body,
            out: String::new(),
            next: 0,
            active_unwind: None,
            emitting_unwind: false,
            drop_check_after_store: false,
            hoisted: String::new(),
            polling: false,
        }
    }

    /// The destructor, then a plain adapter per entry, then a start adapter per entry.
    pub(super) fn vtable(
        &mut self,
        source: TypeId,
        interface: InterfaceId,
        body: &mir::Body,
    ) -> String {
        if let Some(index) = self.vtables.get(&(source, interface)) {
            return format!("@zore_vtable.{index}");
        }
        let index = self.vtables.len();
        self.vtables.insert((source, interface), index);
        let implementations = self.package.implementations[&(source, interface)].clone();
        let mut entries = vec![self.interface_drop(index, source, body)];
        let mut starts = Vec::new();
        for (method, &implementation) in implementations.iter().enumerate() {
            let target = self.target(implementation, source);
            let plain = self.interface_adapter(index, method, source, interface, &target, body);
            starts.push(
                self.interface_start(index, method, source, interface, &target, &plain, body),
            );
            entries.push(plain);
        }
        entries.extend(starts);
        let list: Vec<String> = entries.iter().map(|entry| format!("ptr {entry}")).collect();
        writeln!(
            self.globals,
            "@zore_vtable.{index} = private unnamed_addr constant [{} x ptr] [{}]",
            entries.len(),
            list.join(", ")
        )
        .unwrap();
        format!("@zore_vtable.{index}")
    }

    fn target(&self, implementation: Implementation, source: TypeId) -> Target {
        let package = self.package;
        match implementation {
            Implementation::Method(id) => {
                let function = package.function(id);
                let receiver_local = &function.locals[function.params[0].0 as usize];
                let LocalKind::Param(receiver) = receiver_local.kind else {
                    unreachable!("a method's receiver is its first parameter")
                };
                Target::Method {
                    function: id,
                    symbol: format!("@\"{}.{}\"", package.name, function.name),
                    receiver,
                    by_reference: package.passes_by_reference(receiver, source),
                    results: function.results.clone(),
                }
            }
            Implementation::Entry(inner) => {
                let other = package
                    .types
                    .interface_of(source)
                    .expect("an interface source");
                let entries = package.types.interface_methods(other);
                Target::Entry {
                    index: inner,
                    receiver: entries[inner].receiver,
                    count: entries.len(),
                }
            }
        }
    }

    fn adapter_params(&self, interface: InterfaceId, method: usize) -> (Vec<String>, Vec<String>) {
        let package = self.package;
        let entry = &package.types.interface_methods(interface)[method];
        let mut forwarded = Vec::new();
        for (position, &(mode, ty)) in entry.params.iter().enumerate() {
            if package.passes_by_reference(mode, ty) {
                forwarded.push(format!("ptr %a{position}"));
                if package.needs_drop(ty) {
                    forwarded.push(format!("ptr %af{position}"));
                }
            } else {
                forwarded.push(format!("{} %a{position}", self.ty(ty)));
            }
        }
        let mut params = vec!["ptr %data".to_string(), "ptr %flags".to_string()];
        params.extend(forwarded.iter().cloned());
        (params, forwarded)
    }

    fn interface_drop(&mut self, index: usize, source: TypeId, body: &mir::Body) -> String {
        let name = format!("@zore_vtable.{index}.drop");
        let mut f = self.helper(body);
        f.release_boxed("%data", "%flags", source, true);
        f.line("ret void");
        let code = format!(
            "define private void {name}(ptr %data, ptr %flags) {{\nentry:\n{}{}}}\n\n",
            f.hoisted, f.out
        );
        self.type_helper_code.push_str(&code);
        name
    }

    fn interface_adapter(
        &mut self,
        index: usize,
        method: usize,
        source: TypeId,
        interface: InterfaceId,
        target: &Target,
        body: &mir::Body,
    ) -> String {
        let name = format!("@zore_vtable.{index}.{method}");
        let entry = self.package.types.interface_methods(interface)[method].clone();
        let (params, forwarded) = self.adapter_params(interface, method);
        let results_ty = self.results_ty(&entry.results);
        let mut f = self.helper(body);
        let result = match target {
            Target::Method {
                symbol, results, ..
            } => {
                let mut args = f.receiver_args(target, source);
                args.extend(forwarded);
                f.emit_call(symbol, results, &args)
            }
            Target::Entry { index, .. } => {
                let inner = f.fresh();
                f.line(format!("{inner} = load {INTERFACE_TY}, ptr %data"));
                f.panic_if_empty(&inner, &results_ty);
                let code = f.interface_slot(&inner, 1 + index);
                let (data, flags) = f.interface_parts(&inner);
                let mut args = vec![format!("ptr {data}"), format!("ptr {flags}")];
                args.extend(forwarded);
                f.emit_call(&code, &entry.results, &args)
            }
        };
        if entry.receiver == ParamMode::Own {
            f.release_boxed(
                "%data",
                "%flags",
                source,
                target.receiver() != ParamMode::Own,
            );
        }
        match result {
            Some((ret, value)) => f.line(format!("ret {ret} {value}")),
            None => f.line("ret void"),
        }
        let code = format!(
            "define private {results_ty} {name}({}) {{\nentry:\n{}{}}}\n\n",
            params.join(", "),
            f.hoisted,
            f.out
        );
        self.type_helper_code.push_str(&code);
        name
    }

    /// Gives the method's own frame when it can suspend, and otherwise a frame that is already finished.
    #[allow(clippy::too_many_arguments)]
    fn interface_start(
        &mut self,
        index: usize,
        method: usize,
        source: TypeId,
        interface: InterfaceId,
        target: &Target,
        plain: &str,
        body: &mir::Body,
    ) -> String {
        let name = format!("@zore_vtable.{index}.{method}.start");
        let entry = self.package.types.interface_methods(interface)[method].clone();
        let (params, forwarded) = self.adapter_params(interface, method);
        let owns = entry.receiver == ParamMode::Own;
        let releases_after = owns && target.receiver() != ParamMode::Own;
        let finished = self.finished_frame(index, method, &entry.results, body);
        let wrapper = releases_after.then(|| self.releasing_frame(index, method, source, body));
        let suspends = match target {
            Target::Method { function, .. } => self.machines.machines.contains_key(function),
            Target::Entry { .. } => true,
        };
        let mut f = self.helper(body);
        if suspends {
            let frame = match target {
                Target::Method { function, .. } => {
                    let mut args = f.receiver_args(target, source);
                    args.extend(forwarded.iter().cloned());
                    let constructor = f.module.async_name(*function, "new");
                    let frame = f.fresh();
                    f.line(format!(
                        "{frame} = call ptr {constructor}({})",
                        args.join(", ")
                    ));
                    frame
                }
                Target::Entry { index, count, .. } => {
                    let inner = f.fresh();
                    f.line(format!("{inner} = load {INTERFACE_TY}, ptr %data"));
                    let table = f.interface_table(&inner);
                    let empty = f.fresh();
                    f.line(format!("{empty} = icmp eq ptr {table}, null"));
                    let (missing, present) = (f.label(), f.label());
                    f.line(format!("br i1 {empty}, label %{missing}, label %{present}"));
                    f.out.push_str(&format!("{missing}:\n"));
                    let frame = f.finish_now(plain, &forwarded, &entry.results, &finished);
                    f.line(format!("ret ptr {frame}"));
                    f.out.push_str(&format!("{present}:\n"));
                    let code = f.interface_slot(&inner, 1 + count + index);
                    let (data, flags) = f.interface_parts(&inner);
                    let mut args = vec![format!("ptr {data}"), format!("ptr {flags}")];
                    args.extend(forwarded.iter().cloned());
                    let frame = f.fresh();
                    f.line(format!("{frame} = call ptr {code}({})", args.join(", ")));
                    frame
                }
            };
            let frame = match &wrapper {
                Some(wrapper) => f.releasing(wrapper, &frame),
                None => {
                    if owns {
                        f.release_boxed("%data", "%flags", source, false);
                    }
                    frame
                }
            };
            f.line(format!("ret ptr {frame}"));
        } else {
            let frame = f.finish_now(plain, &forwarded, &entry.results, &finished);
            f.line(format!("ret ptr {frame}"));
        }
        let code = format!(
            "define private ptr {name}({}) {{\nentry:\n{}{}}}\n\n",
            params.join(", "),
            f.hoisted,
            f.out
        );
        self.type_helper_code.push_str(&code);
        name
    }

    /// A frame that already holds its results: polling hands them over at once.
    fn finished_frame(
        &mut self,
        index: usize,
        method: usize,
        results: &[TypeId],
        body: &mir::Body,
    ) -> FrameShape {
        let prefix = format!("@zore_vtable.{index}.{method}.finished");
        let result_ty = self.results_ty(results);
        let frame_ty = if results.is_empty() {
            format!("{{ {FRAME_HEADER} }}")
        } else {
            format!("{{ {FRAME_HEADER}, {result_ty} }}")
        };
        let mut f = self.helper(body);
        if !results.is_empty() {
            let slot = f.fresh();
            f.line(format!(
                "{slot} = getelementptr inbounds {frame_ty}, ptr %frame, i32 0, i32 5"
            ));
            let value = f.fresh();
            f.line(format!("{value} = load {result_ty}, ptr {slot}"));
            f.line(format!("store {result_ty} {value}, ptr %out"));
        }
        f.line("ret i8 1");
        let poll = format!(
            "define private i8 {prefix}.poll(ptr %frame, ptr %context, ptr %out) {{\nentry:\n{}{}}}\n\n",
            f.hoisted, f.out
        );
        let mut f = self.helper(body);
        let size = f.byte_size(&frame_ty, "1");
        f.line(format!("call void @zore_free(ptr %frame, i64 {size})"));
        f.line("ret void");
        let destroy = format!(
            "define private void {prefix}.destroy(ptr %frame) {{\nentry:\n{}{}}}\n\n",
            f.hoisted, f.out
        );
        self.type_helper_code.push_str(&poll);
        self.type_helper_code.push_str(&destroy);
        FrameShape {
            ty: frame_ty,
            poll: format!("{prefix}.poll"),
            destroy: format!("{prefix}.destroy"),
        }
    }

    /// Polls the frame of a method that borrowed the value inside, then destroys that value.
    fn releasing_frame(
        &mut self,
        index: usize,
        method: usize,
        source: TypeId,
        body: &mir::Body,
    ) -> FrameShape {
        let prefix = format!("@zore_vtable.{index}.{method}.releasing");
        let frame_ty = format!("{{ {FRAME_HEADER}, ptr, ptr, ptr }}");
        let mut f = self.helper(body);
        let inner = f.frame_field(&frame_ty, "%frame", 5);
        let child = f.fresh();
        f.line(format!("{child} = load ptr, ptr {inner}"));
        let poll = f.frame_header_pointer(&child, 3);
        let ready = f.fresh();
        f.line(format!(
            "{ready} = call i8 {poll}(ptr {child}, ptr %context, ptr %out)"
        ));
        let done = f.fresh();
        f.line(format!("{done} = icmp ne i8 {ready}, 0"));
        let (finish, wait) = (f.label(), f.label());
        f.line(format!("br i1 {done}, label %{finish}, label %{wait}"));
        f.out.push_str(&format!("{wait}:\n"));
        f.line("ret i8 0");
        f.out.push_str(&format!("{finish}:\n"));
        let destroy = f.frame_header_pointer(&child, 4);
        f.line(format!("call void {destroy}(ptr {child})"));
        f.line(format!("store ptr null, ptr {inner}"));
        f.release_frame_value(&frame_ty, source);
        f.line("ret i8 1");
        let poll = format!(
            "define private i8 {prefix}.poll(ptr %frame, ptr %context, ptr %out) {{\nentry:\n{}{}}}\n\n",
            f.hoisted, f.out
        );
        let mut f = self.helper(body);
        let inner = f.frame_field(&frame_ty, "%frame", 5);
        let child = f.fresh();
        f.line(format!("{child} = load ptr, ptr {inner}"));
        let running = f.fresh();
        f.line(format!("{running} = icmp ne ptr {child}, null"));
        let (stop, free) = (f.label(), f.label());
        f.line(format!("br i1 {running}, label %{stop}, label %{free}"));
        f.out.push_str(&format!("{stop}:\n"));
        let destroy = f.frame_header_pointer(&child, 4);
        f.line(format!("call void {destroy}(ptr {child})"));
        f.release_frame_value(&frame_ty, source);
        f.line(format!("br label %{free}"));
        f.out.push_str(&format!("{free}:\n"));
        let size = f.byte_size(&frame_ty, "1");
        f.line(format!("call void @zore_free(ptr %frame, i64 {size})"));
        f.line("ret void");
        let destroy = format!(
            "define private void {prefix}.destroy(ptr %frame) {{\nentry:\n{}{}}}\n\n",
            f.hoisted, f.out
        );
        self.type_helper_code.push_str(&poll);
        self.type_helper_code.push_str(&destroy);
        FrameShape {
            ty: frame_ty,
            poll: format!("{prefix}.poll"),
            destroy: format!("{prefix}.destroy"),
        }
    }
}

/// A frame layout and the functions its header records.
struct FrameShape {
    ty: String,
    poll: String,
    destroy: String,
}

impl FunctionBuilder<'_, '_> {
    fn interface_parts(&mut self, value: &str) -> (String, String) {
        let data = self.fresh();
        self.line(format!("{data} = extractvalue {INTERFACE_TY} {value}, 0"));
        let flags = self.fresh();
        self.line(format!("{flags} = extractvalue {INTERFACE_TY} {value}, 1"));
        (data, flags)
    }

    fn interface_table(&mut self, value: &str) -> String {
        let table = self.fresh();
        self.line(format!("{table} = extractvalue {INTERFACE_TY} {value}, 2"));
        table
    }

    /// Slot 0 holds the destructor.
    fn interface_slot(&mut self, value: &str, slot: usize) -> String {
        let table = self.interface_table(value);
        let address = self.fresh();
        self.line(format!(
            "{address} = getelementptr inbounds ptr, ptr {table}, i64 {slot}"
        ));
        let code = self.fresh();
        self.line(format!("{code} = load ptr, ptr {address}"));
        code
    }

    /// The receiver as the method takes it: by reference with its flags, or a value read from the storage.
    fn receiver_args(&mut self, target: &Target, source: TypeId) -> Vec<String> {
        let package = self.module.package;
        let Target::Method {
            receiver,
            by_reference,
            ..
        } = target
        else {
            unreachable!("only a method takes a concrete receiver")
        };
        if *by_reference {
            let mut args = vec!["ptr %data".to_string()];
            if package.needs_drop(source) {
                args.push("ptr %flags".to_string());
            }
            return args;
        }
        let ty = self.ty(source);
        let value = self.fresh();
        self.line(format!("{value} = load {ty}, ptr %data"));
        if *receiver == ParamMode::Borrow && package.copies_shared(source) {
            self.retain_value(&value, source);
        }
        vec![format!("{ty} {value}")]
    }

    /// Frees owned storage, first destroying the value inside unless a call consumed it.
    fn release_boxed(&mut self, data: &str, flags: &str, source: TypeId, destroy: bool) {
        if destroy {
            self.drop_value(data, source, flags);
        }
        let size = self.byte_size(&self.module.box_ty(source), "1");
        self.line(format!("call void @zore_free(ptr {data}, i64 {size})"));
    }

    /// Raises the panic and returns at once, since there is no method to call.
    fn panic_if_empty(&mut self, value: &str, results_ty: &str) {
        let table = self.interface_table(value);
        let empty = self.fresh();
        self.line(format!("{empty} = icmp eq ptr {table}, null"));
        let (missing, present) = (self.label(), self.label());
        self.line(format!("br i1 {empty}, label %{missing}, label %{present}"));
        self.out.push_str(&format!("{missing}:\n"));
        let message = "method call on an empty interface value";
        let global = self.module.string_global(message.as_bytes());
        self.line(format!(
            "call void @zore_raise_panic(ptr {global}, i64 {})",
            message.len()
        ));
        if results_ty == "void" {
            self.line("ret void");
        } else {
            self.line(format!("ret {results_ty} undef"));
        }
        self.out.push_str(&format!("{present}:\n"));
    }

    fn frame_field(&mut self, frame_ty: &str, frame: &str, field: usize) -> String {
        let slot = self.fresh();
        self.line(format!(
            "{slot} = getelementptr inbounds {frame_ty}, ptr {frame}, i32 0, i32 {field}"
        ));
        slot
    }

    fn new_frame(&mut self, shape: &FrameShape) -> String {
        let size = self.byte_size(&shape.ty, "1");
        let frame = self.fresh();
        self.line(format!("{frame} = call ptr @zore_alloc(i64 {size})"));
        self.line(format!("store {} zeroinitializer, ptr {frame}", shape.ty));
        let poll = self.frame_field(&shape.ty, &frame, 3);
        self.line(format!("store ptr {}, ptr {poll}", shape.poll));
        let destroy = self.frame_field(&shape.ty, &frame, 4);
        self.line(format!("store ptr {}, ptr {destroy}", shape.destroy));
        frame
    }

    /// Runs the plain adapter now and keeps its results for the first poll.
    fn finish_now(
        &mut self,
        plain: &str,
        forwarded: &[String],
        results: &[TypeId],
        shape: &FrameShape,
    ) -> String {
        let mut args = vec!["ptr %data".to_string(), "ptr %flags".to_string()];
        args.extend(forwarded.iter().cloned());
        let result = self.emit_call(plain, results, &args);
        let frame = self.new_frame(shape);
        if let Some((ret, value)) = result {
            let slot = self.frame_field(&shape.ty, &frame, 5);
            self.line(format!("store {ret} {value}, ptr {slot}"));
        }
        frame
    }

    /// Wraps a running frame so the value inside is destroyed once the call finishes.
    fn releasing(&mut self, shape: &FrameShape, child: &str) -> String {
        let frame = self.new_frame(shape);
        for (field, value) in [(5, child), (6, "%data"), (7, "%flags")] {
            let slot = self.frame_field(&shape.ty, &frame, field);
            self.line(format!("store ptr {value}, ptr {slot}"));
        }
        frame
    }

    fn release_frame_value(&mut self, frame_ty: &str, source: TypeId) {
        let data_slot = self.frame_field(frame_ty, "%frame", 6);
        let data = self.fresh();
        self.line(format!("{data} = load ptr, ptr {data_slot}"));
        let flags_slot = self.frame_field(frame_ty, "%frame", 7);
        let flags = self.fresh();
        self.line(format!("{flags} = load ptr, ptr {flags_slot}"));
        self.release_boxed(&data, &flags, source, true);
    }

    fn triple(&mut self, data: &str, flags: &str, table: &str) -> String {
        let mut value = "undef".to_string();
        for (index, field) in [data, flags, table].into_iter().enumerate() {
            let next = self.fresh();
            self.line(format!(
                "{next} = insertvalue {INTERFACE_TY} {value}, ptr {field}, {index}"
            ));
            value = next;
        }
        value
    }

    pub(super) fn interface_view(&mut self, place: &Place, target: TypeId) -> String {
        let package = self.module.package;
        let source = self.place_ty(place);
        let interface = package
            .types
            .interface_of(target)
            .expect("an interface target");
        match package.types.kind(source) {
            TypeKind::Interface(inner)
            | TypeKind::InterfaceView {
                interface: inner, ..
            } if inner == interface => {
                let address = self.address(place);
                let value = self.fresh();
                self.line(format!("{value} = load {INTERFACE_TY}, ptr {address}"));
                return value;
            }
            _ => {}
        }
        let key = match package.types.kind(source) {
            TypeKind::InterfaceView { interface, .. } => package.types.interface_type(interface),
            _ => source,
        };
        let data = self.address(place);
        let flags = if !package.needs_drop(source) {
            "null".to_string()
        } else if place
            .projections
            .iter()
            .any(|p| matches!(p, mir::Projection::Index(_)))
        {
            self.scratch_flags(source)
        } else {
            self.flag_address(place)
        };
        let table = self.module.vtable(key, interface, self.body);
        self.triple(&data, &flags, &table)
    }

    pub(super) fn interface_box(&mut self, operand: &Operand, target: TypeId) -> String {
        let package = self.module.package;
        let source = self.operand_ty(operand);
        let interface = package
            .types
            .interface_of(target)
            .expect("an interface target");
        let box_ty = self.module.box_ty(source);
        let size = self.byte_size(&box_ty, "1");
        let storage = self.fresh();
        self.line(format!("{storage} = call ptr @zore_alloc(i64 {size})"));
        let value = self.owned_value(operand);
        let data = self.fresh();
        self.line(format!(
            "{data} = getelementptr inbounds {box_ty}, ptr {storage}, i32 0, i32 0"
        ));
        self.line(format!("store {} {value}, ptr {data}", self.ty(source)));
        let flags = if package.needs_drop(source) {
            let flags = self.fresh();
            self.line(format!(
                "{flags} = getelementptr inbounds {box_ty}, ptr {storage}, i32 0, i32 1"
            ));
            self.init_flags_true(&flags, source);
            flags
        } else {
            "null".to_string()
        };
        let table = self.module.vtable(source, interface, self.body);
        self.triple(&data, &flags, &table)
    }

    /// An empty interface value holds nothing to destroy.
    pub(super) fn drop_interface(&mut self, address: &str) {
        let value = self.fresh();
        self.line(format!("{value} = load {INTERFACE_TY}, ptr {address}"));
        let table = self.interface_table(&value);
        let present = self.fresh();
        self.line(format!("{present} = icmp ne ptr {table}, null"));
        let (run, done) = (self.label(), self.label());
        self.line(format!("br i1 {present}, label %{run}, label %{done}"));
        self.out.push_str(&format!("{run}:\n"));
        let (data, flags) = self.interface_parts(&value);
        let destructor = self.fresh();
        self.line(format!("{destructor} = load ptr, ptr {table}"));
        self.line(format!("call void {destructor}(ptr {data}, ptr {flags})"));
        self.line(format!("br label %{done}"));
        self.out.push_str(&format!("{done}:\n"));
    }

    fn interface_receiver(&mut self, receiver: &Operand) -> String {
        match receiver {
            Operand::Ref(place) => {
                let address = self.address(place);
                let value = self.fresh();
                self.line(format!("{value} = load {INTERFACE_TY}, ptr {address}"));
                value
            }
            operand => self.value(operand),
        }
    }

    pub(super) fn interface_start_call(
        &mut self,
        method: usize,
        args: &[Operand],
        destinations: &[Option<Place>],
        target: mir::BlockId,
        unwind: Option<mir::BlockId>,
        span: Span,
    ) -> String {
        let package = self.module.package;
        let receiver_ty = self.operand_ty(&args[0]);
        let interface = package
            .types
            .interface_of(receiver_ty)
            .expect("an interface receiver");
        let count = package.types.interface_methods(interface).len();
        let value = self.interface_receiver(&args[0]);
        let table = self.interface_table(&value);
        let empty = self.fresh();
        self.line(format!("{empty} = icmp eq ptr {table}, null"));
        let (missing, present) = (self.label(), self.label());
        self.line(format!("br i1 {empty}, label %{missing}, label %{present}"));
        self.out.push_str(&format!("{missing}:\n"));
        self.raise_panic("method call on an empty interface value", span);
        self.call_continuation(None, destinations, target, unwind);
        self.out.push_str(&format!("{present}:\n"));
        let code = self.interface_slot(&value, 1 + count + method);
        let (data, flags) = self.interface_parts(&value);
        let mut rendered = vec![format!("ptr {data}"), format!("ptr {flags}")];
        rendered.extend(self.arguments(&args[1..]));
        let child = self.fresh();
        self.line(format!(
            "{child} = call ptr {code}({})",
            rendered.join(", ")
        ));
        child
    }

    /// The receiver comes first: borrowed by reference, consumed by value, or a borrowed view.
    pub(super) fn interface_call(
        &mut self,
        method: usize,
        args: &[Operand],
        span: Span,
    ) -> Option<(String, String)> {
        let package = self.module.package;
        let receiver_ty = self.operand_ty(&args[0]);
        let results = package.interface_entry(receiver_ty, method).results.clone();
        let value = self.interface_receiver(&args[0]);
        let table = self.interface_table(&value);
        let empty = self.fresh();
        self.line(format!("{empty} = icmp eq ptr {table}, null"));
        let (missing, present, called, join) =
            (self.label(), self.label(), self.label(), self.label());
        self.line(format!("br i1 {empty}, label %{missing}, label %{present}"));
        self.out.push_str(&format!("{missing}:\n"));
        self.raise_panic("method call on an empty interface value", span);
        self.line(format!("br label %{join}"));
        self.out.push_str(&format!("{present}:\n"));
        let code = self.interface_slot(&value, 1 + method);
        let (data, flags) = self.interface_parts(&value);
        let mut rendered = vec![format!("ptr {data}"), format!("ptr {flags}")];
        rendered.extend(self.arguments(&args[1..]));
        let result = self.emit_call(&code, &results, &rendered);
        self.line(format!("br label %{called}"));
        self.out.push_str(&format!("{called}:\n"));
        self.line(format!("br label %{join}"));
        self.out.push_str(&format!("{join}:\n"));
        let (ret, value) = result?;
        let merged = self.fresh();
        self.line(format!(
            "{merged} = phi {ret} [ undef, %{missing} ], [ {value}, %{called} ]"
        ));
        Some((ret, merged))
    }
}
