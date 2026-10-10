use std::fmt::Write;

use super::llvm::{FunctionBuilder, Module};
use crate::ast::ParamMode;
use crate::hir::Implementation;
use crate::mir::{self, Operand, Place};
use crate::resolve::LocalKind;
use crate::source::Span;
use crate::types::{InterfaceId, TypeId, TypeKind};

/// The value inside, its drop flags, and the method table.
pub(super) const INTERFACE_TY: &str = "{ ptr, ptr, ptr }";

/// How an adapter reaches the method that serves its entry.
enum Target {
    Method {
        symbol: String,
        receiver: ParamMode,
        by_reference: bool,
        results: Vec<TypeId>,
    },
    Entry {
        index: usize,
        receiver: ParamMode,
    },
}

impl Module<'_> {
    /// Owned storage for a value converted to an interface: the value, then its drop flags.
    fn box_ty(&self, ty: TypeId) -> String {
        format!("{{ {}, {} }}", self.ty(ty), self.flag_ty(ty))
    }

    /// The destructor of an owned value comes first, then one adapter per entry.
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
        for (method, &implementation) in implementations.iter().enumerate() {
            entries.push(self.interface_adapter(
                index,
                method,
                source,
                interface,
                implementation,
                body,
            ));
        }
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

    fn interface_drop(&mut self, index: usize, source: TypeId, body: &mir::Body) -> String {
        let name = format!("@zore_vtable.{index}.drop");
        let box_ty = self.box_ty(source);
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
        f.drop_value("%data", source, "%flags");
        let size = f.byte_size(&box_ty, "1");
        f.line(format!("call void @zore_free(ptr %data, i64 {size})"));
        f.line("ret void");
        let code = format!(
            "define private void {name}(ptr %data, ptr %flags) {{\nentry:\n{}{}}}\n\n",
            f.hoisted, f.out
        );
        self.type_helper_code.push_str(&code);
        name
    }

    /// Takes the value inside and its flags before the entry's own parameters.
    fn interface_adapter(
        &mut self,
        index: usize,
        method: usize,
        source: TypeId,
        interface: InterfaceId,
        implementation: Implementation,
        body: &mir::Body,
    ) -> String {
        let name = format!("@zore_vtable.{index}.{method}");
        let package = self.package;
        let entry = package.types.interface_methods(interface)[method].clone();
        let mut params = vec!["ptr %data".to_string(), "ptr %flags".to_string()];
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
        params.extend(forwarded.iter().cloned());
        let target = match implementation {
            Implementation::Method(id) => {
                let function = package.function(id);
                let receiver_local = &function.locals[function.params[0].0 as usize];
                let LocalKind::Param(receiver) = receiver_local.kind else {
                    unreachable!("a method's receiver is its first parameter")
                };
                Target::Method {
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
                Target::Entry {
                    index: inner,
                    receiver: package.types.interface_methods(other)[inner].receiver,
                }
            }
        };
        let box_ty = self.box_ty(source);
        let results_ty = self.results_ty(&entry.results);
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
        let (result, served_receiver) = match target {
            Target::Method {
                symbol,
                receiver,
                by_reference,
                results,
            } => {
                let mut args = Vec::new();
                if by_reference {
                    args.push("ptr %data".to_string());
                    if package.needs_drop(source) {
                        args.push("ptr %flags".to_string());
                    }
                } else {
                    let ty = f.ty(source);
                    let value = f.fresh();
                    f.line(format!("{value} = load {ty}, ptr %data"));
                    if receiver == ParamMode::Borrow && package.copies_shared(source) {
                        f.retain_value(&value, source);
                    }
                    args.push(format!("{ty} {value}"));
                }
                args.extend(forwarded);
                (f.emit_call(&symbol, &results, &args), receiver)
            }
            Target::Entry { index, receiver } => {
                let inner = f.fresh();
                f.line(format!("{inner} = load {INTERFACE_TY}, ptr %data"));
                let code = f.interface_entry_code(&inner, index);
                let (data, flags) = f.interface_parts(&inner);
                let mut args = vec![format!("ptr {data}"), format!("ptr {flags}")];
                args.extend(forwarded);
                (f.emit_call(&code, &entry.results, &args), receiver)
            }
        };
        if entry.receiver == ParamMode::Own {
            if served_receiver != ParamMode::Own {
                f.drop_value("%data", source, "%flags");
            }
            let size = f.byte_size(&box_ty, "1");
            f.line(format!("call void @zore_free(ptr %data, i64 {size})"));
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

    /// The adapter serving entry `method`; slot 0 holds the destructor.
    fn interface_entry_code(&mut self, value: &str, method: usize) -> String {
        let table = self.interface_table(value);
        let slot = self.fresh();
        self.line(format!(
            "{slot} = getelementptr inbounds ptr, ptr {table}, i64 {}",
            method + 1
        ));
        let code = self.fresh();
        self.line(format!("{code} = load ptr, ptr {slot}"));
        code
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
        let value = match &args[0] {
            Operand::Ref(place) => {
                let address = self.address(place);
                let value = self.fresh();
                self.line(format!("{value} = load {INTERFACE_TY}, ptr {address}"));
                value
            }
            operand => self.value(operand),
        };
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
        let code = self.interface_entry_code(&value, method);
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
