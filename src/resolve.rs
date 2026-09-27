//! Name resolution: assign stable IDs and bind every name use (spec §3.18,
//! §5.7–5.8, §7.8, §27).
//!
//! The result is side tables keyed by the source span of each name, so the
//! type checker never resolves strings again. Member names (fields) need
//! types and are looked up by the type checker.

use std::collections::HashMap;

use crate::ast::{
    self, BindingKind, BindingTarget, ExprKind, ForHeader, Item, ParamMode, StmtKind,
};
use crate::diagnostic::{Diagnostic, Severity};
use crate::hir::{FunctionId, LocalId, LocalKind};
use crate::source::Span;
use crate::types::{StructId, TypeId, TypeStore};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ConstId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Res {
    Local(LocalId),
    Const(ConstId),
    Function(FunctionId),
    Struct(StructId),
    Primitive(TypeId),
    Println,
    /// A predeclared name whose feature the checker does not support yet;
    /// every use has been diagnosed.
    Unsupported,
}

pub struct ConstDecl<'a> {
    pub name: &'a ast::Name,
    pub ty: Option<&'a ast::Type>,
    pub value: &'a ast::Expr,
}

pub struct LocalDecl {
    pub name: String,
    pub span: Span,
    pub kind: LocalKind,
}

pub struct Resolution<'a> {
    pub package: String,
    pub types: TypeStore,
    pub structs: Vec<&'a ast::StructDecl>,
    pub functions: Vec<&'a ast::FuncDecl>,
    pub consts: Vec<ConstDecl<'a>>,
    /// Name and type-name uses, by the span of the name.
    pub uses: HashMap<Span, Res>,
    /// Locals of each function, indexed by `FunctionId`.
    pub locals: Vec<Vec<LocalDecl>>,
    /// Declared local or local constant, by the span of its declaring name.
    pub declarations: HashMap<Span, Res>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Predeclared names (§3.17–3.18). All are protected from shadowing.
fn predeclared(name: &str) -> Option<Res> {
    if let Some(ty) = TypeStore::primitive(name) {
        return Some(Res::Primitive(ty));
    }
    match name {
        "println" => Some(Res::Println),
        "float32" | "float64" | "error" | "Array" | "Task" | "clone" | "drop" => {
            Some(Res::Unsupported)
        }
        _ => None,
    }
}

fn unsupported_predeclared(name: &str) -> &'static str {
    match name {
        "float32" | "float64" => "floating-point types are",
        "error" => "the `error` type is",
        "Array" => "`Array` is",
        "Task" => "`Task` is",
        _ => "`clone` and `drop` are",
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

struct Resolver<'a> {
    out: Resolution<'a>,
    package_scope: HashMap<String, (Res, Span)>,
    scopes: Vec<HashMap<String, (Res, Span)>>,
    function: Option<FunctionId>,
}

impl<'a> Resolver<'a> {
    fn error(&mut self, message: impl Into<String>, span: Span) {
        self.out
            .diagnostics
            .push(Diagnostic::new(Severity::Error, message, span));
    }

    fn unsupported(&mut self, what: &str, span: Span, milestone: &str) {
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
        // Collect package declarations first so bodies may refer to later
        // declarations (§5.3, §7.8).
        for item in &file.items {
            match item {
                Item::Struct(decl) => {
                    let (id, _) = self.out.types.add_struct(&decl.name.text);
                    self.out.structs.push(decl);
                    self.declare_package(&decl.name, Res::Struct(id));
                }
                Item::Func(func) if func.receiver.is_some() => {
                    self.unsupported("methods are", func.name.span, "M5–M8");
                }
                Item::Func(func) if func.is_async => {
                    self.unsupported("`async` functions are", func.name.span, "M25–M29");
                }
                Item::Func(func) => {
                    let id = FunctionId(self.out.functions.len() as u32);
                    self.out.functions.push(func);
                    self.declare_package(&func.name, Res::Function(id));
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
        self.check_struct_cycles();
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

    fn shadows_predeclared(&mut self, name: &ast::Name) -> bool {
        if predeclared(&name.text).is_none() {
            return false;
        }
        self.out.diagnostics.push(
            Diagnostic::new(
                Severity::Error,
                format!("`{}` shadows a predeclared name", name.text),
                name.span,
            )
            .note("declarations cannot reuse predeclared names such as `int` or `println`"),
        );
        true
    }

    fn duplicate(&mut self, name: &ast::Name, first: Span, what: &str) {
        self.out.diagnostics.push(
            Diagnostic::new(
                Severity::Error,
                format!("duplicate {what} `{}`", name.text),
                name.span,
            )
            .related(first, "first declared here"),
        );
    }

    fn declare_package(&mut self, name: &ast::Name, res: Res) {
        if self.shadows_predeclared(name) {
            return;
        }
        if let Some(&(_, first)) = self.package_scope.get(&name.text) {
            self.duplicate(name, first, "declaration");
            return;
        }
        self.package_scope
            .insert(name.text.clone(), (res, name.span));
    }

    fn declare_local(&mut self, name: &ast::Name, res: Res) {
        self.out.declarations.insert(name.span, res);
        if self.shadows_predeclared(name) {
            return;
        }
        let scope = self
            .scopes
            .last_mut()
            .expect("locals are declared in a scope");
        if let Some(&(_, first)) = scope.get(&name.text) {
            self.duplicate(name, first, "declaration");
            return;
        }
        scope.insert(name.text.clone(), (res, name.span));
    }

    fn new_local(&mut self, name: &ast::Name, kind: LocalKind) {
        let function = self.function.expect("locals belong to a function").0 as usize;
        let locals = &mut self.out.locals[function];
        let id = LocalId(locals.len() as u32);
        locals.push(LocalDecl {
            name: name.text.clone(),
            span: name.span,
            kind,
        });
        self.declare_local(name, Res::Local(id));
    }

    fn lookup(&self, name: &str) -> Option<Res> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .or_else(|| self.package_scope.get(name))
            .map(|&(res, _)| res)
            .or_else(|| predeclared(name))
    }

    /// Resolve a name use and record it.
    fn use_name(&mut self, name: &str, span: Span) -> Option<Res> {
        let Some(res) = self.lookup(name) else {
            self.error(format!("cannot find `{name}` in this scope"), span);
            return None;
        };
        if res == Res::Unsupported {
            self.unsupported(unsupported_predeclared(name), span, "M19–M25");
        }
        self.out.uses.insert(span, res);
        Some(res)
    }

    fn ty(&mut self, ty: &ast::Type) {
        match self.use_name(&ty.name.text, ty.name.span) {
            Some(Res::Primitive(_) | Res::Struct(_) | Res::Unsupported) | None => {}
            Some(_) => {
                self.out.uses.remove(&ty.name.span);
                self.error(format!("`{}` is not a type", ty.name.text), ty.name.span);
            }
        }
    }

    /// Reject structs that contain themselves by value; they have no finite
    /// layout (§7.8).
    fn check_struct_cycles(&mut self) {
        let count = self.out.structs.len();
        let edges: Vec<Vec<usize>> = self
            .out
            .structs
            .iter()
            .map(|decl| {
                decl.fields
                    .iter()
                    .filter_map(|f| match self.out.uses.get(&f.ty.name.span) {
                        Some(Res::Struct(id)) => Some(id.0 as usize),
                        _ => None,
                    })
                    .collect()
            })
            .collect();
        // 0 = unvisited, 1 = on the current path, 2 = finished.
        let mut state = vec![0u8; count];
        let mut reported = vec![false; count];
        for start in 0..count {
            let mut stack = vec![(start, 0usize)];
            while let Some(&mut (node, ref mut next)) = stack.last_mut() {
                if *next == 0 && state[node] == 0 {
                    state[node] = 1;
                }
                if state[node] == 2 {
                    stack.pop();
                    continue;
                }
                if let Some(&target) = edges[node].get(*next) {
                    *next += 1;
                    if state[target] == 1 && !reported[target] {
                        reported[target] = true;
                        let decl = self.out.structs[target];
                        self.error(
                            format!(
                                "struct `{}` contains itself by value and has no finite size",
                                decl.name.text
                            ),
                            decl.name.span,
                        );
                    } else if state[target] == 0 {
                        stack.push((target, 0));
                    }
                } else {
                    state[node] = 2;
                    stack.pop();
                }
            }
        }
    }

    fn function(&mut self, id: FunctionId, func: &'a ast::FuncDecl) {
        self.function = Some(id);
        self.out.locals.push(Vec::new());
        // Parameters and the outermost body block share one scope (§5.8).
        self.scopes.push(HashMap::new());
        for param in &func.params {
            self.ty(&param.ty);
            if param.mode == ParamMode::Mut {
                self.unsupported("`mut` parameters are", param.span, "M13–M17");
            }
            self.new_local(&param.name, LocalKind::Param(param.mode));
        }
        for result in &func.results {
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
                // The counting initializer's scope encloses the header and body.
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
        // Names enter scope after the initializer (§5.8).
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
