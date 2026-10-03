//! The resolver pass: collects package declarations, then walks bodies.

use std::collections::HashMap;

use super::ids::{ConstId, FunctionId};
use super::symbol::{ConstDecl, LocalDecl, LocalKind, Res};
use crate::ast::{self, BindingKind, BindingTarget, ExprKind, ForHeader, Item, StmtKind};
use crate::diagnostic::{Diagnostic, Severity};
use crate::source::Span;
use crate::types::{StructId, TypeStore};

pub struct Resolution<'a> {
    pub package: String,
    pub types: TypeStore,
    pub structs: Vec<&'a ast::StructDecl>,
    /// Functions and methods; a method's receiver is its first parameter.
    pub functions: Vec<&'a ast::FuncDecl>,
    pub methods: HashMap<(StructId, String), FunctionId>,
    pub consts: Vec<ConstDecl<'a>>,
    /// Name and type-name uses, by the span of the name.
    pub uses: HashMap<Span, Res>,
    /// Locals of each function, indexed by `FunctionId`.
    pub locals: Vec<Vec<LocalDecl>>,
    /// Declared local or local constant, by the span of its declaring name.
    pub declarations: HashMap<Span, Res>,
    pub diagnostics: Vec<Diagnostic>,
}

/// The eventual named type at the bottom of any array layers.
fn named_type(ty: &ast::Type) -> &ast::Name {
    match ty {
        ast::Type::Named(name) => name,
        ast::Type::Array { element, .. } => named_type(element),
    }
}

pub fn resolve(file: &ast::File) -> Resolution<'_> {
    let mut resolver = Resolver {
        out: Resolution {
            package: file
                .package
                .as_ref()
                .map_or(String::new(), |n| n.text.clone()),
            types: TypeStore::new(),
            structs: Vec::new(),
            functions: Vec::new(),
            methods: HashMap::new(),
            consts: Vec::new(),
            uses: HashMap::new(),
            locals: Vec::new(),
            declarations: HashMap::new(),
            diagnostics: Vec::new(),
        },
        package_scope: HashMap::new(),
        scopes: Vec::new(),
        function: None,
    };
    resolver.file(file);
    resolver.out
}

pub(super) struct Resolver<'a> {
    pub(super) out: Resolution<'a>,
    pub(super) package_scope: HashMap<String, (Res, Span)>,
    pub(super) scopes: Vec<HashMap<String, (Res, Span)>>,
    pub(super) function: Option<FunctionId>,
}

impl<'a> Resolver<'a> {
    pub(super) fn error(&mut self, message: impl Into<String>, span: Span) {
        self.out
            .diagnostics
            .push(Diagnostic::new(Severity::Error, message, span));
    }

    pub(super) fn unsupported(&mut self, what: &str, span: Span, milestone: &str) {
        self.out.diagnostics.push(
            Diagnostic::new(
                Severity::Error,
                format!("{what} not supported by the checker yet"),
                span,
            )
            .note(format!("planned for roadmap milestone {milestone}")),
        );
    }

    fn file(&mut self, file: &'a ast::File) {
        for import in &file.imports {
            self.out.diagnostics.push(
                Diagnostic::new(
                    Severity::Error,
                    "imports are not supported by the checker yet",
                    import.span,
                )
                .note("`zore check` currently treats one file as the whole package (M23)"),
            );
        }
        // Collect declarations first so bodies can use later ones.
        let mut methods = Vec::new();
        for item in &file.items {
            match item {
                Item::Struct(decl) => {
                    let (id, _) = self.out.types.add_struct(&decl.name.text);
                    self.out.structs.push(decl);
                    self.declare_package(&decl.name, Res::Struct(id));
                }
                Item::Func(func) if func.is_async => {
                    self.unsupported("`async` functions are", func.name.span, "M25–M29");
                }
                Item::Func(func) => {
                    let id = FunctionId(self.out.functions.len() as u32);
                    self.out.functions.push(func);
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
                        .note("package variable initialization order is unresolved (Q05)"),
                    );
                }
            }
        }
        for decl in self.out.structs.clone() {
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
            self.declare_method(id);
        }
        self.reject_self_containing_structs();
        for index in 0..self.out.consts.len() {
            let value = self.out.consts[index].value;
            let ty = self.out.consts[index].ty;
            if let Some(ty) = ty {
                self.ty(ty);
            }
            self.expr(value);
        }
        for (index, func) in self.out.functions.clone().into_iter().enumerate() {
            self.function(FunctionId(index as u32), func);
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
                    "a destructor gets mutable access without ownership, never a shared or `own` receiver (§14.3)",
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
            ast::Type::Array { element, size, .. } => {
                self.ty(element);
                self.expr(size);
            }
        }
    }

    fn reject_self_containing_structs(&mut self) {
        let count = self.out.structs.len();
        let edges: Vec<Vec<usize>> = self
            .out
            .structs
            .iter()
            .map(|decl| {
                decl.fields
                    .iter()
                    .filter_map(|f| match self.out.uses.get(&named_type(&f.ty).span) {
                        Some(Res::Struct(id)) => Some(id.0 as usize),
                        _ => None,
                    })
                    .collect()
            })
            .collect();
        const UNVISITED: u8 = 0;
        const ON_PATH: u8 = 1;
        const FINISHED: u8 = 2;
        let mut state = vec![UNVISITED; count];
        let mut reported = vec![false; count];
        for start in 0..count {
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
                        self.error(
                            format!(
                                "struct `{}` contains itself by value and has no finite size",
                                decl.name.text
                            ),
                            decl.name.span,
                        );
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
        self.function = Some(id);
        self.out.locals.push(Vec::new());
        // Parameters share the outermost body scope.
        self.scopes.push(HashMap::new());
        for param in func.receiver.iter().chain(&func.params) {
            self.ty(&param.ty);
            self.new_local(&param.name, LocalKind::Param(param.mode));
        }
        for (index, result) in func.results.iter().enumerate() {
            if let ast::Type::Named(name) = result
                && name.text == "error"
                && index + 1 != func.results.len()
            {
                self.error(
                    "an `error` result must be the last result and appear only once",
                    name.span,
                );
            }
            self.ty(result);
        }
        for stmt in &func.body.stmts {
            self.stmt(stmt);
        }
        self.scopes.pop();
        self.function = None;
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
                self.use_name(name, expr.span);
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
            ExprKind::Field { base, .. } => self.expr(base),
            ExprKind::Index { base, index } => {
                self.expr(base);
                self.expr(index);
            }
            ExprKind::ArrayLit { ty, elements } => {
                self.ty(ty);
                for element in elements {
                    self.expr(element);
                }
            }
            ExprKind::StructLit { ty, fields } => {
                match self.use_name(&ty.text, ty.span) {
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
