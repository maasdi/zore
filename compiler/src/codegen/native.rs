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

#[derive(Clone, Copy)]
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
                TypeKind::Int(_) | TypeKind::Rune => {
                    let llvm = self.ty(ty);
                    args.push(format!("{llvm} %p{index}"));
                    declared.push(llvm);
                }
                TypeKind::Bool => {
                    args.push(format!("i1 zeroext %p{index}"));
                    declared.push("i1 zeroext".to_string());
                }
                _ => unreachable!(
                    "bundled functions take strings, slices, integers, runes, and booleans"
                ),
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
        let (call, declaration, adapter) = match result {
            Result::None => (
                format!(
                    "  call void @{symbol}({})\n{release}  ret void\n",
                    args.join(", ")
                ),
                format!("declare void @{symbol}({})", declared.join(", ")),
                None,
            ),
            Result::Scalar(ty) => {
                let (ret, marker) = if ty == TypeStore::BOOL {
                    ("i1".to_string(), "zeroext ")
                } else {
                    (self.ty(ty), "")
                };
                (
                    format!(
                        "  %r = call {marker}{ret} @{symbol}({})\n{release}  ret {ret} %r\n",
                        args.join(", ")
                    ),
                    format!("declare {marker}{ret} @{symbol}({})", declared.join(", ")),
                    None,
                )
            }
            other => {
                let (layout, conversion, value) = self.native_result(other, &returns);
                let mut rendered = vec!["ptr %out".to_string()];
                rendered.extend(args.clone());
                let mut parameters = vec!["ptr".to_string()];
                parameters.extend(declared.clone());
                (
                    format!(
                        "  %out = alloca {layout}\n  call void @{symbol}({})\n{conversion}{release}  ret {returns} {value}\n",
                        rendered.join(", ")
                    ),
                    format!("declare void @{symbol}({})", parameters.join(", ")),
                    Some((layout, conversion, value)),
                )
            }
        };
        self.intrinsics.insert(declaration);
        let plain = format!(
            "define {returns} @\"{}.{}\"({}) {{\nentry:\n{prologue}{call}}}\n\n",
            self.package.name,
            function.name,
            header.join(", ")
        );
        let name = format!("@\"{}.{}$io", self.package.name, function.name);
        match crate::async_lowering::native_wait(&function.name) {
            Some(crate::async_lowering::NativeWait::Helper) => {
                let mut types: Vec<_> = function
                    .params
                    .iter()
                    .map(|param| self.ty(function.locals[param.0 as usize].ty))
                    .collect();
                types.push("ptr".into());
                let layout = format!("{{ {} }}", types.join(", "));
                let mut job =
                    format!("define private void {name}$job\"(ptr %storage) {{\nentry:\n");
                let mut loaded = Vec::new();
                for (index, ty) in types.iter().enumerate() {
                    writeln!(job, "  %s{index} = getelementptr inbounds {layout}, ptr %storage, i32 0, i32 {index}\n  %a{index} = load {ty}, ptr %s{index}").unwrap();
                    if index + 1 < types.len() {
                        loaded.push(format!("{ty} %a{index}"));
                    }
                }
                writeln!(job, "  %result = call {returns} @\"{}.{}\"({})\n  store {returns} %result, ptr %a{}\n  ret void\n}}\n", self.package.name, function.name, loaded.join(", "), types.len() - 1).unwrap();
                plain + &job
            }
            Some(crate::async_lowering::NativeWait::Socket) => {
                let start = format!(
                    "define private ptr {name}$start\"({}, ptr %context) {{\nentry:\n{prologue}  %operation = call ptr @{symbol}_start({}, ptr %context)\n{release}  ret ptr %operation\n}}\n\n",
                    header.join(", "),
                    args.join(", ")
                );
                self.intrinsics.insert(format!(
                    "declare ptr @{symbol}_start({}, ptr)",
                    declared.join(", ")
                ));
                self.intrinsics
                    .insert("declare i8 @zore_native_net_poll(ptr, ptr, ptr)".into());
                let (layout, conversion, value) =
                    adapter.expect("socket results use an out pointer");
                let poll = format!(
                    "define private i8 {name}$poll\"(ptr %operation, ptr %context, ptr %result) {{\nentry:\n  %out = alloca {layout}\n  %ready = call i8 @zore_native_net_poll(ptr %operation, ptr %context, ptr %out)\n  %finished = icmp ne i8 %ready, 0\n  br i1 %finished, label %done, label %pending\npending:\n  ret i8 0\ndone:\n{conversion}  store {returns} {value}, ptr %result\n  ret i8 1\n}}\n\n"
                );
                plain + &start + &poll
            }
            None => plain,
        }
    }
    fn native_result(&self, result: Result, returns: &str) -> (String, String, String) {
        if let Result::Aggregate(ty) = result {
            let layout = self.ty(ty);
            return (
                layout.clone(),
                format!("  %r = load {layout}, ptr %out\n"),
                "%r".into(),
            );
        }
        let (layout, error_index) = match result {
            Result::ErrorOnly => ("{ i8, ptr, i64 }", 0),
            Result::ValueError(_) => ("{ i64, i8, ptr, i64 }", 1),
            Result::StringError => ("{ ptr, i64, i8, ptr, i64 }", 2),
            Result::ArrayError(_) => ("{ ptr, i64, i64, i8, ptr, i64 }", 3),
            _ => unreachable!(),
        };
        let mut conversion = format!(
            "  %r = load {layout}, ptr %out\n  %failed = extractvalue {layout} %r, {error_index}\n  %message = extractvalue {layout} %r, {}\n  %length = extractvalue {layout} %r, {}\n  %present = icmp ne i8 %failed, 0\n  %e0 = insertvalue {{ i1, ptr, i64 }} undef, i1 %present, 0\n  %e1 = insertvalue {{ i1, ptr, i64 }} %e0, ptr %message, 1\n  %e2 = insertvalue {{ i1, ptr, i64 }} %e1, i64 %length, 2\n",
            error_index + 1,
            error_index + 2
        );
        let (value_ty, value) = match result {
            Result::ErrorOnly => return (layout.into(), conversion, "%e2".into()),
            Result::ValueError(ty) => {
                writeln!(conversion, "  %value = extractvalue {layout} %r, 0").unwrap();
                if ty == TypeStore::BOOL {
                    conversion.push_str("  %v = trunc i64 %value to i1\n");
                } else {
                    conversion.push_str("  %v = add i64 %value, 0\n");
                }
                (self.ty(ty), "%v")
            }
            Result::StringError | Result::ArrayError(_) => {
                let value_ty = match result {
                    Result::ArrayError(ty) => self.ty(ty),
                    _ => "{ ptr, i64 }".into(),
                };
                writeln!(conversion, "  %data = extractvalue {layout} %r, 0\n  %size = extractvalue {layout} %r, 1\n  %v0 = insertvalue {value_ty} undef, ptr %data, 0\n  %v1 = insertvalue {value_ty} %v0, i64 %size, 1").unwrap();
                if matches!(result, Result::ArrayError(_)) {
                    writeln!(conversion, "  %capacity = extractvalue {layout} %r, 2\n  %v2 = insertvalue {value_ty} %v1, i64 %capacity, 2").unwrap();
                    (value_ty, "%v2")
                } else {
                    (value_ty, "%v1")
                }
            }
            _ => unreachable!(),
        };
        writeln!(conversion, "  %t0 = insertvalue {returns} undef, {value_ty} {value}, 0\n  %t1 = insertvalue {returns} %t0, {{ i1, ptr, i64 }} %e2, 1").unwrap();
        (layout.into(), conversion, "%t1".into())
    }
}
