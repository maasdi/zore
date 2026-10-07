//! Functions declared without a body call into the runtime through a generated shim.

use std::fmt::Write;

use super::llvm::Module;
use crate::mir::Body;
use crate::types::{TypeId, TypeKind, TypeStore};

/// `zore/strings.HasPrefix` is `zore_native_strings_has_prefix` in the runtime.
fn symbol(name: &str) -> String {
    let (path, function) = name.rsplit_once('.').unwrap_or(("", name));
    let package = path.rsplit('/').next().unwrap_or(path);
    let mut snake = String::new();
    for (index, c) in function.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if index > 0 {
                snake.push('_');
            }
            snake.push(c.to_ascii_lowercase());
        } else {
            snake.push(c);
        }
    }
    format!("zore_native_{package}_{snake}")
}

enum Result {
    None,
    Scalar(TypeId),
    /// Written through an out pointer: a `string` or an `Array<string>`.
    Aggregate(TypeId),
    /// `(int, error)` or `(bool, error)`, written as `{ i64, i8, ptr, i64 }`.
    ValueError(TypeId),
    /// `error`, written as `{ i8, ptr, i64 }`.
    ErrorOnly,
    /// `(string, error)`, written as `{ ptr, i64, i8, ptr, i64 }`.
    StringError,
    /// `(Array<...>, error)`, written as `{ ptr, i64, i64, i8, ptr, i64 }`.
    ArrayError(TypeId),
}

impl Module<'_> {
    pub(super) fn native_function(&mut self, body: &Body) -> String {
        let function = self.package.function(body.function);
        let symbol = symbol(&function.name);
        let mut header = Vec::new();
        let mut prologue = String::new();
        let mut release = String::new();
        let mut args = Vec::new();
        let mut declared = Vec::new();
        for (index, &param) in function.params.iter().enumerate() {
            let ty = function.locals[param.0 as usize].ty;
            header.push(format!("{} %p{index}", self.ty(ty)));
            match self.types().kind(ty) {
                TypeKind::String | TypeKind::Slice { .. } => {
                    if matches!(self.types().kind(ty), TypeKind::String) {
                        writeln!(
                            release,
                            "  call void @zore_string_release(ptr %d{index}, i64 %n{index})"
                        )
                        .unwrap();
                    }
                    writeln!(
                        prologue,
                        "  %d{index} = extractvalue {{ ptr, i64 }} %p{index}, 0\n  %n{index} = extractvalue {{ ptr, i64 }} %p{index}, 1"
                    )
                    .unwrap();
                    args.push(format!("ptr %d{index}, i64 %n{index}"));
                    declared.push("ptr, i64".to_string());
                }
                TypeKind::Int(_) => {
                    args.push(format!("i64 %p{index}"));
                    declared.push("i64".to_string());
                }
                TypeKind::Bool => {
                    args.push(format!("i1 zeroext %p{index}"));
                    declared.push("i1 zeroext".to_string());
                }
                _ => unreachable!("bundled functions take strings, slices, integers, and booleans"),
            }
        }
        let result = match &function.results[..] {
            [] => Result::None,
            [one] => match self.types().kind(*one) {
                TypeKind::String | TypeKind::DynArray { .. } => Result::Aggregate(*one),
                TypeKind::Error => Result::ErrorOnly,
                _ => Result::Scalar(*one),
            },
            [value, error] if *error == TypeStore::ERROR && *value == TypeStore::STRING => {
                Result::StringError
            }
            [value, error]
                if *error == TypeStore::ERROR
                    && matches!(self.types().kind(*value), TypeKind::DynArray { .. }) =>
            {
                Result::ArrayError(*value)
            }
            [value, error] if *error == TypeStore::ERROR => Result::ValueError(*value),
            _ => unreachable!("bundled functions have a supported result shape"),
        };
        let returns = self.results_ty(&function.results);
        let (call, declaration) = match result {
            Result::None => (
                format!(
                    "  call void @{symbol}({})\n{release}  ret void\n",
                    args.join(", ")
                ),
                format!("declare void @{symbol}({})", declared.join(", ")),
            ),
            Result::Scalar(ty) => {
                let (ret, marker) = if ty == TypeStore::BOOL {
                    ("i1", "zeroext ")
                } else {
                    ("i64", "")
                };
                (
                    format!(
                        "  %r = call {marker}{ret} @{symbol}({})\n{release}  ret {ret} %r\n",
                        args.join(", ")
                    ),
                    format!("declare {marker}{ret} @{symbol}({})", declared.join(", ")),
                )
            }
            Result::Aggregate(ty) => {
                let layout = self.ty(ty);
                let mut rendered = vec!["ptr %out".to_string()];
                rendered.extend(args);
                let mut parameters = vec!["ptr".to_string()];
                parameters.extend(declared);
                (
                    format!(
                        "  %out = alloca {layout}\n  call void @{symbol}({})\n  %r = load {layout}, ptr %out\n{release}  ret {layout} %r\n",
                        rendered.join(", ")
                    ),
                    format!("declare void @{symbol}({})", parameters.join(", ")),
                )
            }
            Result::ErrorOnly => {
                let mut rendered = vec!["ptr %out".to_string()];
                rendered.extend(args);
                let mut parameters = vec!["ptr".to_string()];
                parameters.extend(declared);
                (
                    format!(
                        "  %out = alloca {{ i8, ptr, i64 }}\n  call void @{symbol}({})\n  %r = load {{ i8, ptr, i64 }}, ptr %out\n  %failed = extractvalue {{ i8, ptr, i64 }} %r, 0\n  %message = extractvalue {{ i8, ptr, i64 }} %r, 1\n  %length = extractvalue {{ i8, ptr, i64 }} %r, 2\n  %present = icmp ne i8 %failed, 0\n  %e0 = insertvalue {{ i1, ptr, i64 }} undef, i1 %present, 0\n  %e1 = insertvalue {{ i1, ptr, i64 }} %e0, ptr %message, 1\n  %e2 = insertvalue {{ i1, ptr, i64 }} %e1, i64 %length, 2\n{release}  ret {{ i1, ptr, i64 }} %e2\n",
                        rendered.join(", ")
                    ),
                    format!("declare void @{symbol}({})", parameters.join(", ")),
                )
            }
            Result::StringError => {
                let mut rendered = vec!["ptr %out".to_string()];
                rendered.extend(args);
                let mut parameters = vec!["ptr".to_string()];
                parameters.extend(declared);
                (
                    format!(
                        "  %out = alloca {{ ptr, i64, i8, ptr, i64 }}\n  call void @{symbol}({})\n  %r = load {{ ptr, i64, i8, ptr, i64 }}, ptr %out\n  %data = extractvalue {{ ptr, i64, i8, ptr, i64 }} %r, 0\n  %size = extractvalue {{ ptr, i64, i8, ptr, i64 }} %r, 1\n  %failed = extractvalue {{ ptr, i64, i8, ptr, i64 }} %r, 2\n  %message = extractvalue {{ ptr, i64, i8, ptr, i64 }} %r, 3\n  %length = extractvalue {{ ptr, i64, i8, ptr, i64 }} %r, 4\n  %present = icmp ne i8 %failed, 0\n  %s0 = insertvalue {{ ptr, i64 }} undef, ptr %data, 0\n  %s1 = insertvalue {{ ptr, i64 }} %s0, i64 %size, 1\n  %e0 = insertvalue {{ i1, ptr, i64 }} undef, i1 %present, 0\n  %e1 = insertvalue {{ i1, ptr, i64 }} %e0, ptr %message, 1\n  %e2 = insertvalue {{ i1, ptr, i64 }} %e1, i64 %length, 2\n  %t0 = insertvalue {returns} undef, {{ ptr, i64 }} %s1, 0\n  %t1 = insertvalue {returns} %t0, {{ i1, ptr, i64 }} %e2, 1\n{release}  ret {returns} %t1\n",
                        rendered.join(", ")
                    ),
                    format!("declare void @{symbol}({})", parameters.join(", ")),
                )
            }
            Result::ArrayError(array) => {
                let mut rendered = vec!["ptr %out".to_string()];
                rendered.extend(args);
                let mut parameters = vec!["ptr".to_string()];
                parameters.extend(declared);
                let array_ty = self.ty(array);
                (
                    format!(
                        "  %out = alloca {{ ptr, i64, i64, i8, ptr, i64 }}\n  call void @{symbol}({})\n  %r = load {{ ptr, i64, i64, i8, ptr, i64 }}, ptr %out\n  %data = extractvalue {{ ptr, i64, i64, i8, ptr, i64 }} %r, 0\n  %size = extractvalue {{ ptr, i64, i64, i8, ptr, i64 }} %r, 1\n  %capacity = extractvalue {{ ptr, i64, i64, i8, ptr, i64 }} %r, 2\n  %failed = extractvalue {{ ptr, i64, i64, i8, ptr, i64 }} %r, 3\n  %message = extractvalue {{ ptr, i64, i64, i8, ptr, i64 }} %r, 4\n  %length = extractvalue {{ ptr, i64, i64, i8, ptr, i64 }} %r, 5\n  %present = icmp ne i8 %failed, 0\n  %a0 = insertvalue {array_ty} undef, ptr %data, 0\n  %a1 = insertvalue {array_ty} %a0, i64 %size, 1\n  %a2 = insertvalue {array_ty} %a1, i64 %capacity, 2\n  %e0 = insertvalue {{ i1, ptr, i64 }} undef, i1 %present, 0\n  %e1 = insertvalue {{ i1, ptr, i64 }} %e0, ptr %message, 1\n  %e2 = insertvalue {{ i1, ptr, i64 }} %e1, i64 %length, 2\n  %t0 = insertvalue {returns} undef, {array_ty} %a2, 0\n  %t1 = insertvalue {returns} %t0, {{ i1, ptr, i64 }} %e2, 1\n{release}  ret {returns} %t1\n",
                        rendered.join(", ")
                    ),
                    format!("declare void @{symbol}({})", parameters.join(", ")),
                )
            }
            Result::ValueError(value) => {
                let mut rendered = vec!["ptr %out".to_string()];
                rendered.extend(args);
                let mut parameters = vec!["ptr".to_string()];
                parameters.extend(declared);
                let value_ty = self.ty(value);
                let narrow = if value == TypeStore::BOOL {
                    "  %v = trunc i64 %value to i1\n".to_string()
                } else {
                    "  %v = add i64 %value, 0\n".to_string()
                };
                (
                    format!(
                        "  %out = alloca {{ i64, i8, ptr, i64 }}\n  call void @{symbol}({})\n  %r = load {{ i64, i8, ptr, i64 }}, ptr %out\n  %value = extractvalue {{ i64, i8, ptr, i64 }} %r, 0\n  %failed = extractvalue {{ i64, i8, ptr, i64 }} %r, 1\n  %message = extractvalue {{ i64, i8, ptr, i64 }} %r, 2\n  %length = extractvalue {{ i64, i8, ptr, i64 }} %r, 3\n{narrow}  %present = icmp ne i8 %failed, 0\n  %e0 = insertvalue {{ i1, ptr, i64 }} undef, i1 %present, 0\n  %e1 = insertvalue {{ i1, ptr, i64 }} %e0, ptr %message, 1\n  %e2 = insertvalue {{ i1, ptr, i64 }} %e1, i64 %length, 2\n  %s0 = insertvalue {returns} undef, {value_ty} %v, 0\n  %s1 = insertvalue {returns} %s0, {{ i1, ptr, i64 }} %e2, 1\n{release}  ret {returns} %s1\n",
                        rendered.join(", ")
                    ),
                    format!("declare void @{symbol}({})", parameters.join(", ")),
                )
            }
        };
        self.intrinsics.insert(declaration);
        format!(
            "define {returns} @\"{}.{}\"({}) {{\nentry:\n{prologue}{call}}}\n\n",
            self.package.name,
            function.name,
            header.join(", ")
        )
    }
}
