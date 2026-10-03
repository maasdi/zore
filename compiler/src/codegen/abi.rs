//! The runtime ABI: declared runtime symbols, the calling convention for Zore
//! functions, runtime output calls, and the native entry shim.

use super::llvm::{FunctionBuilder, Module};
use crate::mir::{self, Operand};
use crate::resolve::FunctionId;
use crate::source::Span;
use crate::types::TypeKind;

pub(super) const RUNTIME_DECLARATIONS: &str = "\
declare void @zore_println_str(ptr, i64)
declare void @zore_println_i64(i64)
declare void @zore_println_u64(i64)
declare void @zore_println_bool(i1 zeroext)
declare void @zore_println_rune(i32)
declare i32 @zore_string_compare(ptr, i64, ptr, i64)
declare void @zore_raise_panic(ptr, i64)
declare zeroext i1 @zore_panic_pending()
declare void @zore_enter_drop()
declare void @zore_leave_drop()
declare void @zore_abort() noreturn
declare double @llvm.trunc.f64(double)
declare double @llvm.fabs.f64(double)
";

impl Module<'_> {
    /// The native entry point the Rust runtime calls into.
    pub(super) fn entry_shim(&self, entry: FunctionId) -> String {
        let name = &self.package.function(entry).name;
        format!(
            "define void @zore_entry() {{\nentry:\n  call void @\"{}.{name}\"()\n  ret void\n}}\n",
            self.package.name
        )
    }
}

impl FunctionBuilder<'_, '_> {
    pub(super) fn call(&mut self, id: FunctionId, args: &[Operand]) -> Option<(String, String)> {
        let package = self.module.package;
        let callee = package.function(id);
        let mut rendered = Vec::new();
        for arg in args {
            if let Operand::Ref(place) = arg {
                let address = self.address(place);
                rendered.push(format!("ptr {address}"));
                let ty = self.place_ty(place);
                if !package.is_copy(ty) {
                    let flags = if place
                        .projections
                        .iter()
                        .any(|p| matches!(p, mir::Projection::Index(_)))
                    {
                        self.scratch_flags(ty)
                    } else {
                        self.flag_address(place)
                    };
                    rendered.push(format!("ptr {flags}"));
                }
                continue;
            }
            let ty = self.ty(self.operand_ty(arg));
            let value = self.value(arg);
            rendered.push(format!("{ty} {value}"));
        }
        let ret = self.module.results_ty(&callee.results);
        let target = format!(
            "@\"{}.{}\"({})",
            package.name,
            callee.name,
            rendered.join(", ")
        );
        if callee.results.is_empty() {
            self.line(format!("call void {target}"));
            return None;
        }
        let result = self.fresh();
        self.line(format!("{result} = call {ret} {target}"));
        Some((ret, result))
    }

    pub(super) fn println(&mut self, arg: &Operand, span: Span) {
        let arg_ty = self.operand_ty(arg);
        let kind = self.module.package.types.kind(arg_ty);
        if let TypeKind::Float(_) = kind {
            self.module.unsupported(
                "printing floating-point values is",
                span,
                "the float text format for `println` is still TBD (§37.1)",
            );
            return;
        }
        let value = self.value(arg);
        match kind {
            TypeKind::String => {
                let (ptr, len) = self.string_parts(&value);
                self.line(format!("call void @zore_println_str(ptr {ptr}, i64 {len})"));
            }
            TypeKind::Bool => {
                self.line(format!("call void @zore_println_bool(i1 zeroext {value})"))
            }
            TypeKind::Rune => self.line(format!("call void @zore_println_rune(i32 {value})")),
            TypeKind::Int(int) => {
                let wide = if int.bits == 64 {
                    value
                } else {
                    let name = self.fresh();
                    let extend = if int.signed { "sext" } else { "zext" };
                    self.line(format!("{name} = {extend} i{} {value} to i64", int.bits));
                    name
                };
                let function = if int.signed {
                    "zore_println_i64"
                } else {
                    "zore_println_u64"
                };
                self.line(format!("call void @{function}(i64 {wide})"));
            }
            TypeKind::Float(_)
            | TypeKind::Error
            | TypeKind::Struct(_)
            | TypeKind::Array { .. }
            | TypeKind::Slice { .. } => unreachable!("checked printable type"),
        }
    }
}
