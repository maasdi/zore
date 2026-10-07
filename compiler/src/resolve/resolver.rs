use std::collections::HashMap;

use super::ids::{ConstId, FunctionId};
use super::symbol::{ClosureDecl, ConstDecl, LocalDecl, LocalKind, Res, predeclared};
use super::units::{FileUnit, PackageInfo, PackageUnit};
use crate::ast::{self, BindingKind, BindingTarget, ExprKind, ForHeader, Item, StmtKind};
use crate::diagnostic::{Diagnostic, Severity};
use crate::source::Span;
use crate::types::{StructId, TypeStore};

pub struct Resolution<'a> {
    /// The entry package's name.
    pub package: String,
    /// The first function named `main` without a receiver in the entry package.
    pub entry_main: Option<&'a ast::FuncDecl>,
    pub packages: Vec<PackageInfo>,
    pub entry_package: usize,
    /// Indexed like `structs`, `functions`, and `consts`.
    pub struct_package: Vec<usize>,
    pub function_package: Vec<usize>,
    pub const_package: Vec<usize>,
    pub types: TypeStore,
    pub structs: Vec<&'a ast::StructDecl>,
    /// A method's receiver is its first parameter.
    pub functions: Vec<&'a ast::FuncDecl>,
    pub methods: HashMap<(StructId, String), FunctionId>,
    pub consts: Vec<ConstDecl<'a>>,
    /// Keyed by the span of the name.
    pub uses: HashMap<Span, Res>,
    /// Indexed by `FunctionId`.
    pub locals: Vec<Vec<LocalDecl>>,
    /// Closure `i` has `FunctionId(functions.len() + i)`.
    pub closures: Vec<ClosureDecl<'a>>,
    /// Keyed by the span of the declaring name.
    pub declarations: HashMap<Span, Res>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Slices borrow and `Array<T>` stores on the heap, so neither stores by value.
fn by_value_named_type(ty: &ast::Type) -> Option<&ast::Name> {
    match ty {
        ast::Type::Named(name) | ast::Type::Qualified { name, .. } => Some(name),
        ast::Type::Array { element, .. } => by_value_named_type(element),
        ast::Type::Slice { .. }
        | ast::Type::DynArray { .. }
        | ast::Type::Map { .. }
        | ast::Type::Func { .. } => None,
    }
}

fn owned_named_type(ty: &ast::Type) -> Option<&ast::Name> {
    match ty {
        ast::Type::Named(name) | ast::Type::Qualified { name, .. } => Some(name),
        ast::Type::Array { element, .. } | ast::Type::DynArray { element, .. } => {
            owned_named_type(element)
        }
        ast::Type::Map { value, .. } => owned_named_type(value),
        ast::Type::Slice { .. } | ast::Type::Func { .. } => None,
    }
}

pub fn resolve<'a>(units: &'a [PackageUnit<'a>]) -> Resolution<'a> {
    let mut resolver = Resolver {
        out: Resolution {
            package: String::new(),
            entry_main: None,
            packages: Vec::new(),
            entry_package: 0,
            struct_package: Vec::new(),
            function_package: Vec::new(),
            const_package: Vec::new(),
            types: TypeStore::new(),
            structs: Vec::new(),
            functions: Vec::new(),
            methods: HashMap::new(),
            consts: Vec::new(),
            uses: HashMap::new(),
            locals: Vec::new(),
            closures: Vec::new(),
            declarations: HashMap::new(),
            diagnostics: Vec::new(),
        },
        package_scopes: Vec::new(),
        imports: Vec::new(),
        current_package: 0,
        current_file: 0,
        file_of_struct: Vec::new(),
        file_of_function: Vec::new(),
        file_of_const: Vec::new(),
        scopes: Vec::new(),
        function: None,
        frames: Vec::new(),
    };
    resolver.all(units);
    resolver.out
}

/// What one file's import declarations make visible.
#[derive(Default)]
pub(super) struct FileImports {
    pub(super) entries: Vec<ImportEntry>,
    pub(super) by_name: HashMap<String, usize>,
}

pub(super) struct ImportEntry {
    pub(super) name: String,
    pub(super) package: usize,
    pub(super) span: Span,
    pub(super) used: bool,
}

pub(super) struct Resolver<'a> {
    pub(super) out: Resolution<'a>,
    /// Indexed by package.
    pub(super) package_scopes: Vec<HashMap<String, (Res, Span)>>,
    /// Indexed by package, then file.
    pub(super) imports: Vec<Vec<FileImports>>,
    pub(super) current_package: usize,
    pub(super) current_file: usize,
    /// The file each struct, function, and constant was declared in.
    pub(super) file_of_struct: Vec<usize>,
    pub(super) file_of_function: Vec<usize>,
    pub(super) file_of_const: Vec<usize>,
    pub(super) scopes: Vec<HashMap<String, (Res, Span)>>,
    pub(super) function: Option<FunctionId>,
    /// Outermost first.
    pub(super) frames: Vec<Frame>,
}

#[derive(Clone, Copy)]
pub(super) struct Frame {
    pub(super) function: FunctionId,
    pub(super) scope_base: usize,
}

impl<'a> Resolver<'a> {
    pub(super) fn error(&mut self, message: impl Into<String>, span: Span) {
        self.out
            .diagnostics
            .push(Diagnostic::new(Severity::Error, message, span));
    }

    pub(super) fn unsupported(&mut self, what: &str, span: Span) {
        self.out.diagnostics.push(
            Diagnostic::new(
                Severity::Error,
                format!("{what} not supported by the checker yet"),
                span,
            )
            .note("planned for a later milestone"),
        );
    }

    fn all(&mut self, units: &'a [PackageUnit<'a>]) {
        self.out.entry_package = units.len().saturating_sub(1);
        self.out.package = units.last().map_or(String::new(), |unit| unit.name.clone());
        self.out.packages = units
            .iter()
            .map(|unit| PackageInfo {
                path: unit.path.clone(),
                name: unit.name.clone(),
                clause: unit.clause(),
            })
            .collect();
        self.package_scopes = vec![HashMap::new(); units.len()];
        self.imports = units
            .iter()
            .map(|unit| unit.files.iter().map(|_| FileImports::default()).collect())
            .collect();
        // Collect declarations first so bodies can use later ones.
        let mut methods = Vec::new();
        for (package, unit) in units.iter().enumerate() {
            for (file, file_unit) in unit.files.iter().enumerate() {
                (self.current_package, self.current_file) = (package, file);
                self.declare_items(unit, file_unit.file, &mut methods);
            }
        }
        for (package, unit) in units.iter().enumerate() {
            for (file, file_unit) in unit.files.iter().enumerate() {
                (self.current_package, self.current_file) = (package, file);
                self.bind_imports(file_unit);
            }
        }
        for (index, decl) in self.out.structs.clone().into_iter().enumerate() {
            self.enter(self.out.struct_package[index], self.file_of_struct[index]);
            let mut fields: HashMap<&str, Span> = HashMap::new();
            for field in &decl.fields {
                if let Some(&first) = fields.get(field.name.text.as_str()) {
                    self.duplicate(&field.name, first, "field");
                } else {
                    fields.insert(&field.name.text, field.name.span);
                }
                self.ty(&field.ty);
            }
        }
        for id in methods {
            self.enter(
                self.out.function_package[id.0 as usize],
                self.file_of_function[id.0 as usize],
            );
            self.declare_method(id);
        }
        self.reject_self_containing_structs();
        for index in 0..self.out.consts.len() {
            self.enter(self.out.const_package[index], self.file_of_const[index]);
            let value = self.out.consts[index].value;
            let ty = self.out.consts[index].ty;
            if let Some(ty) = ty {
                self.ty(ty);
            }
            self.expr(value);
        }
        self.out.locals = self.out.functions.iter().map(|_| Vec::new()).collect();
        for (index, func) in self.out.functions.clone().into_iter().enumerate() {
            self.enter(
                self.out.function_package[index],
                self.file_of_function[index],
            );
            self.function(FunctionId(index as u32), func);
        }
        self.report_unused_imports();
    }

    fn note_entry_main(&mut self, func: &'a ast::FuncDecl) {
        if self.current_package == self.out.entry_package
            && func.receiver.is_none()
            && func.name.text == "main"
            && self.out.entry_main.is_none()
        {
            self.out.entry_main = Some(func);
        }
    }

    fn enter(&mut self, package: usize, file: usize) {
        (self.current_package, self.current_file) = (package, file);
    }

    fn declare_items(
        &mut self,
        unit: &PackageUnit<'a>,
        file: &'a ast::File,
        methods: &mut Vec<FunctionId>,
    ) {
        let (package, file_index) = (self.current_package, self.current_file);
        for item in &file.items {
            match item {
                Item::Struct(decl) => {
                    let display = if package == self.out.entry_package {
                        decl.name.text.clone()
                    } else {
                        format!("{}.{}", unit.name, decl.name.text)
                    };
                    let (id, _) = self.out.types.add_struct(&display);
                    self.out.structs.push(decl);
                    self.out.struct_package.push(package);
                    self.file_of_struct.push(file_index);
                    self.declare_package(&decl.name, Res::Struct(id));
                }
                Item::Func(func) if func.is_async => {
                    self.note_entry_main(func);
                    self.unsupported("`async` functions are", func.name.span);
                }
                Item::Func(func) => {
                    self.note_entry_main(func);
                    let id = FunctionId(self.out.functions.len() as u32);
                    self.out.functions.push(func);
                    self.out.function_package.push(package);
                    self.file_of_function.push(file_index);
                    if func.receiver.is_some() {
                        methods.push(id);
                    } else {
                        self.declare_package(&func.name, Res::Function(id));
                    }
                }
                Item::Binding(binding) if binding.kind == BindingKind::Const => {
                    if let Some(res) = self.const_decl(binding)
                        && let [BindingTarget::Name(name)] = &binding.targets[..]
                    {
                        self.out.const_package.push(package);
                        self.file_of_const.push(file_index);
                        self.declare_package(name, res);
                    }
                }
                Item::Binding(binding) => {
                    self.out.diagnostics.push(
                        Diagnostic::new(
                            Severity::Error,
                            "package-level `let` and `var` are not supported by the checker yet",
                            binding.span,
                        )
                        .note("package variable initialization order is unresolved"),
                    );
                }
            }
        }
    }

    /// Makes this file's imports visible, rejecting the ones that clash.
    fn bind_imports(&mut self, file_unit: &FileUnit<'a>) {
        let (package, file) = (self.current_package, self.current_file);
        for (import, binding) in file_unit.file.imports.iter().zip(&file_unit.imports) {
            let table = &self.imports[package][file];
            if let Some(&first) = table.by_name.get(&binding.name) {
                let first_package = table.entries[first].package;
                let message = if first_package == binding.package {
                    format!("package `{}` is imported twice in this file", import.path)
                } else {
                    format!("two imports in this file are both named `{}`", binding.name)
                };
                let first_span = table.entries[first].span;
                self.out.diagnostics.push(
                    Diagnostic::new(Severity::Error, message, import.span)
                        .related(first_span, "first imported here"),
                );
                continue;
            }
            if let Some(&(_, declared)) = self.package_scopes[package].get(&binding.name) {
                self.out.diagnostics.push(
                    Diagnostic::new(
                        Severity::Error,
                        format!(
                            "import `{}` conflicts with a declaration of the same name",
                            binding.name
                        ),
                        import.span,
                    )
                    .related(declared, "declared here"),
                );
                continue;
            }
            if predeclared(&binding.name).is_some() {
                self.out.diagnostics.push(
                    Diagnostic::new(
                        Severity::Error,
                        format!("import `{}` shadows a predeclared name", binding.name),
                        import.span,
                    )
                    .note("declarations cannot reuse predeclared names such as `int` or `println`"),
                );
                continue;
            }
            let table = &mut self.imports[package][file];
            table
                .by_name
                .insert(binding.name.clone(), table.entries.len());
            table.entries.push(ImportEntry {
                name: binding.name.clone(),
                package: binding.package,
                span: import.span,
                used: false,
            });
        }
    }

    fn report_unused_imports(&mut self) {
        let mut unused = Vec::new();
        for files in &self.imports {
            for file in files {
                for entry in file.entries.iter().filter(|entry| !entry.used) {
                    unused.push((entry.span, entry.name.clone()));
                }
            }
        }
        for (span, name) in unused {
            self.out.diagnostics.push(
                Diagnostic::new(
                    Severity::Error,
                    format!("package `{name}` is imported and not used"),
                    span,
                )
                .note("remove the import, or use one of its exported names"),
            );
        }
    }

    pub(super) fn mark_import_used(&mut self, name: &str) {
        let table = &mut self.imports[self.current_package][self.current_file];
        if let Some(&index) = table.by_name.get(name) {
            table.entries[index].used = true;
        }
    }

    /// Resolves `package.Name` to an exported member of an imported package.
    fn package_member(&mut self, package: &ast::Name, member: &ast::Name) -> Option<Res> {
        let target = match self.lookup(&package.text) {
            Some(Res::Package(target)) => target,
            Some(_) => {
                self.error(format!("`{}` is not a package", package.text), package.span);
                return None;
            }
            None => {
                self.error(
                    format!("cannot find package `{}` in this file", package.text),
                    package.span,
                );
                return None;
            }
        };
        self.mark_import_used(&package.text);
        self.out.uses.insert(package.span, Res::Package(target));
        let exported = member.text.starts_with(|c: char| c.is_ascii_uppercase());
        match self.package_scopes[target].get(&member.text) {
            Some(&(res, _)) if exported => {
                self.out.uses.insert(member.span, res);
                Some(res)
            }
            Some(_) => {
                self.out.diagnostics.push(
                    Diagnostic::new(
                        Severity::Error,
                        format!(
                            "`{}` is not exported by package `{}`",
                            member.text, package.text
                        ),
                        member.span,
                    )
                    .note("only names that start with an uppercase letter can be used from another package"),
                );
                None
            }
            None => {
                self.error(
                    format!(
                        "package `{}` does not declare `{}`",
                        package.text, member.text
                    ),
                    member.span,
                );
                None
            }
        }
    }

    fn declare_method(&mut self, id: FunctionId) {
        let func = self.out.functions[id.0 as usize];
        let receiver = func
            .receiver
            .as_ref()
            .expect("only methods are declared as methods");
        let ast::Type::Named(type_name) = &receiver.ty else {
            self.error(
                "methods can be declared only on struct types defined in this package",
                receiver.ty.span(),
            );
            return;
        };
        let strukt = match self.lookup(&type_name.text) {
            Some(Res::Struct(strukt)) => strukt,
            Some(Res::Primitive(_)) => {
                self.error(
                    "methods can be declared only on struct types defined in this package",
                    type_name.span,
                );
                return;
            }
            _ => return,
        };
        if func.name.text == "drop" {
            self.check_drop_signature(func, receiver);
        }
        if func.name.text == "clone" {
            self.check_clone_signature(func, receiver, type_name);
        }
        let key = (strukt, func.name.text.clone());
        if let Some(&first) = self.out.methods.get(&key) {
            let first = self.out.functions[first.0 as usize].name.span;
            self.duplicate(&func.name, first, "method");
            return;
        }
        let decl = self.out.structs[strukt.0 as usize];
        if let Some(field) = decl.fields.iter().find(|f| f.name.text == func.name.text) {
            let first = field.name.span;
            self.duplicate(&func.name, first, "member");
            return;
        }
        self.out.methods.insert(key, id);
    }

    fn check_drop_signature(&mut self, func: &ast::FuncDecl, receiver: &ast::Param) {
        if receiver.mode != ast::ParamMode::Mut {
            self.out.diagnostics.push(
                Diagnostic::new(
                    Severity::Error,
                    "`drop` must have a `mut` receiver",
                    receiver.span,
                )
                .note(
                    "a destructor gets mutable access without ownership, never a shared or `own` receiver",
                ),
            );
        }
        if let Some(param) = func.params.first() {
            self.error("`drop` takes no parameters", param.span);
        }
        if let Some(result) = func.results.first() {
            self.error("`drop` returns no result", result.span());
        }
    }

    fn check_clone_signature(
        &mut self,
        func: &ast::FuncDecl,
        receiver: &ast::Param,
        type_name: &ast::Name,
    ) {
        if receiver.mode != ast::ParamMode::Borrow {
            self.out.diagnostics.push(
                Diagnostic::new(
                    Severity::Error,
                    "`clone` must have a shared receiver",
                    receiver.span,
                )
                .note("cloning borrows its argument and never consumes or mutates it"),
            );
        }
        if let Some(param) = func.params.first() {
            self.error("`clone` takes no parameters", param.span);
        }
        let returns_receiver = matches!(
            &func.results[..],
            [ast::Type::Named(result)] if result.text == type_name.text
        );
        if !returns_receiver {
            let span = func.results.first().map_or(func.name.span, ast::Type::span);
            self.out.diagnostics.push(
                Diagnostic::new(
                    Severity::Error,
                    format!("`clone` must return exactly `{}`", type_name.text),
                    span,
                )
                .note("a custom clone returns an independent value of its own receiver type"),
            );
        }
    }

    fn const_decl(&mut self, binding: &'a ast::Binding) -> Option<Res> {
        let [BindingTarget::Name(name)] = &binding.targets[..] else {
            return None;
        };
        let id = ConstId(self.out.consts.len() as u32);
        self.out.consts.push(ConstDecl {
            name,
            ty: binding.ty.as_ref(),
            value: &binding.value,
        });
        Some(Res::Const(id))
    }

    fn ty(&mut self, ty: &'a ast::Type) {
        match ty {
            ast::Type::Named(name) => match self.use_name(&name.text, name.span) {
                Some(Res::Primitive(_) | Res::Struct(_) | Res::Unsupported) | None => {}
                Some(_) => {
                    self.out.uses.remove(&name.span);
                    self.error(format!("`{}` is not a type", name.text), name.span);
                }
            },
            ast::Type::Qualified { package, name, .. } => {
                match self.package_member(package, name) {
                    Some(Res::Struct(_)) | None => {}
                    Some(_) => {
                        self.out.uses.remove(&name.span);
                        self.error(
                            format!("`{}.{}` is not a type", package.text, name.text),
                            name.span,
                        );
                    }
                }
            }
            ast::Type::Array { element, size, .. } => {
                self.ty(element);
                self.expr(size);
            }
            ast::Type::Slice { element, .. } | ast::Type::DynArray { element, .. } => {
                self.ty(element)
            }
            ast::Type::Map { key, value, .. } => {
                self.ty(key);
                self.ty(value);
            }
            ast::Type::Func {
                params, results, ..
            } => {
                for param in params {
                    self.ty(&param.ty);
                }
                for result in results {
                    self.ty(result);
                }
            }
        }
    }

    fn reject_self_containing_structs(&mut self) {
        let mut reported = vec![false; self.out.structs.len()];
        let by_value = self.struct_edges(by_value_named_type);
        self.report_struct_cycles(&by_value, &mut reported, |name| {
            format!("struct `{name}` contains itself by value and has no finite size")
        });
        // Drops are emitted inline per type, so self-ownership needs out-of-line drop functions.
        let owned = self.struct_edges(owned_named_type);
        self.report_struct_cycles(&owned, &mut reported, |name| {
            format!(
                "struct `{name}` contains itself through `Array<T>` or a map, which is not supported by this compiler yet"
            )
        });
    }

    fn struct_edges(&self, named: fn(&ast::Type) -> Option<&ast::Name>) -> Vec<Vec<usize>> {
        self.out
            .structs
            .iter()
            .map(|decl| {
                decl.fields
                    .iter()
                    .filter_map(|f| named(&f.ty))
                    .filter_map(|name| match self.out.uses.get(&name.span) {
                        Some(Res::Struct(id)) => Some(id.0 as usize),
                        _ => None,
                    })
                    .collect()
            })
            .collect()
    }

    /// Reports each cycle once.
    fn report_struct_cycles(
        &mut self,
        edges: &[Vec<usize>],
        reported: &mut [bool],
        message: impl Fn(&str) -> String,
    ) {
        const UNVISITED: u8 = 0;
        const ON_PATH: u8 = 1;
        const FINISHED: u8 = 2;
        let mut state = vec![UNVISITED; edges.len()];
        for start in 0..edges.len() {
            let mut stack = vec![(start, 0usize)];
            while let Some(&mut (node, ref mut next)) = stack.last_mut() {
                if *next == 0 && state[node] == UNVISITED {
                    state[node] = ON_PATH;
                }
                if state[node] == FINISHED {
                    stack.pop();
                    continue;
                }
                if let Some(&target) = edges[node].get(*next) {
                    *next += 1;
                    if state[target] == ON_PATH && !reported[target] {
                        reported[target] = true;
                        let decl = self.out.structs[target];
                        self.error(message(&decl.name.text), decl.name.span);
                    } else if state[target] == UNVISITED {
                        stack.push((target, 0));
                    }
                } else {
                    state[node] = FINISHED;
                    stack.pop();
                }
            }
        }
    }

    fn function(&mut self, id: FunctionId, func: &'a ast::FuncDecl) {
        let params: Vec<&'a ast::Param> = func.receiver.iter().chain(&func.params).collect();
        self.body(id, &params, &func.results, &func.body);
    }

    fn body(
        &mut self,
        id: FunctionId,
        params: &[&'a ast::Param],
        results: &'a [ast::Type],
        body: &'a ast::Block,
    ) {
        let enclosing = self.function.replace(id);
        self.frames.push(Frame {
            function: id,
            scope_base: self.scopes.len(),
        });
        // Parameters share the outermost body scope.
        self.scopes.push(HashMap::new());
        for param in params {
            self.ty(&param.ty);
            self.new_local(&param.name, LocalKind::Param(param.mode));
        }
        for (index, result) in results.iter().enumerate() {
            if let ast::Type::Named(name) = result
                && name.text == "error"
                && index + 1 != results.len()
            {
                self.error(
                    "an `error` result must be the last result and appear only once",
                    name.span,
                );
            }
            self.ty(result);
        }
        for stmt in &body.stmts {
            self.stmt(stmt);
        }
        self.scopes.pop();
        self.frames.pop();
        self.function = enclosing;
    }

    fn closure(&mut self, closure: &'a ast::Closure, span: Span) {
        let Some(parent) = self.function else {
            self.error(
                "a function literal can only appear inside a function body",
                span,
            );
            return;
        };
        let id = FunctionId(self.out.locals.len() as u32);
        self.out.locals.push(Vec::new());
        self.out.closures.push(ClosureDecl {
            id,
            closure,
            span,
            parent,
            captures: Vec::new(),
        });
        let params: Vec<&'a ast::Param> = closure.params.iter().collect();
        self.body(id, &params, &closure.results, &closure.body);
    }

    fn block(&mut self, block: &'a ast::Block) {
        self.scopes.push(HashMap::new());
        for stmt in &block.stmts {
            self.stmt(stmt);
        }
        self.scopes.pop();
    }

    fn stmt(&mut self, stmt: &'a ast::Stmt) {
        match &stmt.kind {
            StmtKind::Binding(binding) => self.binding(binding),
            StmtKind::Assign {
                targets, values, ..
            } => {
                for target in targets {
                    if let ast::AssignTarget::Place(expr) = target {
                        self.expr(expr);
                    }
                }
                for value in values {
                    self.expr(value);
                }
            }
            StmtKind::Expr(expr) => self.expr(expr),
            StmtKind::Return(values) => {
                for value in values {
                    self.expr(value);
                }
            }
            StmtKind::Break | StmtKind::Continue => {}
            StmtKind::If(if_stmt) => self.if_stmt(if_stmt),
            StmtKind::For(for_stmt) => {
                // The loop initializer's scope covers the header and body.
                self.scopes.push(HashMap::new());
                match &for_stmt.header {
                    ForHeader::Infinite => {}
                    ForHeader::Condition(condition) => self.expr(condition),
                    ForHeader::Counting {
                        init,
                        condition,
                        update,
                    } => {
                        self.stmt(init);
                        self.expr(condition);
                        self.stmt(update);
                    }
                    ForHeader::Each {
                        first,
                        second,
                        collection,
                    } => {
                        self.expr(collection);
                        let (key, item) = match second {
                            Some(second) => (Some(first), second),
                            None => (None, first),
                        };
                        if let (Some(BindingTarget::Name(key)), BindingTarget::Name(item)) =
                            (key, item)
                            && key.text == item.text
                        {
                            self.duplicate(item, key.span, "declaration");
                        } else if let Some(BindingTarget::Name(key)) = key {
                            self.new_local(key, LocalKind::Let);
                        }
                        if let BindingTarget::Name(item) = item {
                            self.new_local(item, LocalKind::Item);
                        }
                    }
                }
                self.block(&for_stmt.body);
                self.scopes.pop();
            }
            StmtKind::Block(block) => self.block(block),
        }
    }

    fn if_stmt(&mut self, if_stmt: &'a ast::If) {
        self.expr(&if_stmt.condition);
        self.block(&if_stmt.then_block);
        match &if_stmt.else_branch {
            Some(ast::Else::If(inner)) => self.if_stmt(inner),
            Some(ast::Else::Block(block)) => self.block(block),
            None => {}
        }
    }

    fn binding(&mut self, binding: &'a ast::Binding) {
        if let Some(ty) = &binding.ty {
            self.ty(ty);
        }
        // Names enter scope after their initializer.
        self.expr(&binding.value);
        if binding.kind == BindingKind::Const {
            if let Some(res) = self.const_decl(binding)
                && let [BindingTarget::Name(name)] = &binding.targets[..]
            {
                self.declare_local(name, res);
            }
            return;
        }
        let kind = if binding.kind == BindingKind::Let {
            LocalKind::Let
        } else {
            LocalKind::Var
        };
        let mut seen: HashMap<&str, Span> = HashMap::new();
        for target in &binding.targets {
            if let BindingTarget::Name(name) = target {
                if let Some(&first) = seen.get(name.text.as_str()) {
                    self.duplicate(name, first, "declaration");
                    continue;
                }
                seen.insert(&name.text, name.span);
                self.new_local(name, kind);
            }
        }
    }

    fn expr(&mut self, expr: &'a ast::Expr) {
        match &expr.kind {
            ExprKind::Name(name) => {
                if let Some(Res::Package(_)) = self.use_name(name, expr.span) {
                    self.mark_import_used(name);
                    self.out.uses.remove(&expr.span);
                    self.error(
                        format!("use of package `{name}` without selector"),
                        expr.span,
                    );
                }
            }
            ExprKind::Int(_)
            | ExprKind::Float
            | ExprKind::String(_)
            | ExprKind::Rune(_)
            | ExprKind::Bool(_)
            | ExprKind::Nil
            | ExprKind::Malformed => {}
            ExprKind::Paren(inner)
            | ExprKind::Await(inner)
            | ExprKind::Try(inner)
            | ExprKind::Unary { operand: inner, .. } => self.expr(inner),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Call { callee, args } => {
                self.expr(callee);
                for arg in args {
                    self.expr(arg);
                }
            }
            ExprKind::Field { base, name } => {
                if let ExprKind::Name(package) = &base.kind
                    && let Some(Res::Package(_)) = self.lookup(package)
                {
                    let package = ast::Name {
                        text: package.clone(),
                        span: base.span,
                    };
                    self.package_member(&package, name);
                } else {
                    self.expr(base);
                }
            }
            ExprKind::Index { base, index } => {
                self.expr(base);
                self.expr(index);
            }
            ExprKind::Slice { base, low, high } => {
                self.expr(base);
                for bound in [low, high].into_iter().flatten() {
                    self.expr(bound);
                }
            }
            ExprKind::ArrayLit { ty, elements } => {
                self.ty(ty);
                for element in elements {
                    self.expr(element);
                }
            }
            ExprKind::MapLit { ty, entries } => {
                self.ty(ty);
                for entry in entries {
                    self.expr(&entry.key);
                    self.expr(&entry.value);
                }
            }
            ExprKind::Closure(closure) => self.closure(closure, expr.span),
            ExprKind::StructLit {
                package,
                ty,
                fields,
            } => {
                let resolved = match package {
                    Some(package) => self.package_member(package, ty),
                    None => self.use_name(&ty.text, ty.span),
                };
                match resolved {
                    Some(Res::Struct(_) | Res::Unsupported) | None => {}
                    Some(_) => {
                        self.out.uses.remove(&ty.span);
                        self.error(format!("`{}` is not a struct type", ty.text), ty.span);
                    }
                }
                for field in fields {
                    self.expr(&field.value);
                }
            }
        }
    }
}
