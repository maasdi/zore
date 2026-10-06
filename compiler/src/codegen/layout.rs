//! How Zore types are laid out as LLVM types, including drop-flag blocks.

use std::fmt::Write;

use super::llvm::Module;
use crate::types::{TypeId, TypeKind};

impl Module<'_> {
    pub(super) fn flag_ty(&self, ty: TypeId) -> String {
        match self.package.types.kind(ty) {
            TypeKind::Struct(id) => {
                format!(
                    "%\"{}.DropFlags.{}\"",
                    self.package.name,
                    self.package.strukt(id).name
                )
            }
            _ => "i1".into(),
        }
    }

    pub(super) fn ty(&self, ty: TypeId) -> String {
        match self.package.types.kind(ty) {
            TypeKind::Bool => "i1".into(),
            TypeKind::Int(int) => format!("i{}", int.bits),
            TypeKind::Float(float) if float.bits == 32 => "float".into(),
            TypeKind::Float(_) => "double".into(),
            TypeKind::Rune => "i32".into(),
            TypeKind::String => "{ ptr, i64 }".into(),
            TypeKind::Error => "{ i1, ptr, i64 }".into(),
            TypeKind::Struct(id) => {
                format!(
                    "%\"{}.{}\"",
                    self.package.name,
                    self.package.strukt(id).name
                )
            }
            TypeKind::Array { element, size } => format!("[{size} x {}]", self.ty(element)),
            TypeKind::Slice { .. } | TypeKind::DynArray { .. } => "{ ptr, i64 }".into(),
            TypeKind::Map { .. } => "ptr".into(),
            // The closure body's code, then its captured environment.
            TypeKind::Func(_) => "{ ptr, ptr }".into(),
        }
    }

    pub(super) fn results_ty(&self, results: &[TypeId]) -> String {
        match results {
            [] => "void".into(),
            [one] => self.ty(*one),
            many => {
                let fields: Vec<String> = many.iter().map(|&t| self.ty(t)).collect();
                format!("{{ {} }}", fields.join(", "))
            }
        }
    }

    /// Named LLVM struct types for every Zore struct and its drop flags.
    pub(super) fn type_declarations(&self) -> String {
        let mut out = String::new();
        for strukt in &self.package.structs {
            let fields: Vec<String> = strukt.fields.iter().map(|f| self.ty(f.ty)).collect();
            writeln!(
                out,
                "%\"{}.{}\" = type {{ {} }}",
                self.package.name,
                strukt.name,
                fields.join(", ")
            )
            .unwrap();
            let flag_fields: Vec<String> =
                strukt.fields.iter().map(|f| self.flag_ty(f.ty)).collect();
            let fields = std::iter::once("i1".to_string())
                .chain(flag_fields)
                .collect::<Vec<_>>();
            writeln!(
                out,
                "%\"{}.DropFlags.{}\" = type {{ {} }}",
                self.package.name,
                strukt.name,
                fields.join(", ")
            )
            .unwrap();
        }
        out
    }
}
