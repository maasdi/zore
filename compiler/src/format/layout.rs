use std::collections::HashSet;

use crate::ast::*;
use crate::lexer::{Punct, Token, TokenKind};
use crate::source::{SourceFile, Span};

pub(super) struct Layout {
    pub blocks: HashSet<u32>,
    pub tight_before: HashSet<u32>,
    pub spaced_before: HashSet<u32>,
    pub tight_after: HashSet<u32>,
    pub unary: HashSet<u32>,
    pub inline_separators: HashSet<u32>,
    pub blank_after: HashSet<u32>,
}

struct Builder<'a> {
    file: &'a SourceFile,
    tokens: &'a [Token],
    layout: Layout,
}

impl Layout {
    pub fn new(file: &SourceFile, tokens: &[Token], ast: &File) -> Self {
        let mut builder = Builder {
            file,
            tokens,
            layout: Self {
                blocks: HashSet::new(),
                tight_before: HashSet::new(),
                spaced_before: HashSet::new(),
                tight_after: HashSet::new(),
                unary: HashSet::new(),
                inline_separators: HashSet::new(),
                blank_after: HashSet::new(),
            },
        };
        if let Some(package) = &ast.package {
            builder.layout.blank_after.insert(package.span.end());
        }
        if let Some(import) = ast.imports.last() {
            builder.layout.blank_after.insert(import.span.end());
        }
        for item in &ast.items {
            match item {
                Item::Func(func) => {
                    if let Some(receiver) = &func.receiver {
                        builder.declared_ty(&receiver.ty);
                        let at = builder
                            .tokens
                            .partition_point(|t| t.span.start() < receiver.span.start());
                        if let Some(token) = builder.tokens[..at]
                            .iter()
                            .rfind(|t| t.kind == TokenKind::Punct(Punct::LParen))
                        {
                            builder.layout.spaced_before.insert(token.span.start());
                        }
                    }
                    builder.generics(&func.type_params, func.span);
                    builder.signature(&func.params, &func.results);
                    builder.block(&func.body);
                    builder.layout.blank_after.insert(func.span.end());
                }
                Item::Struct(decl) => {
                    builder.generics(&decl.type_params, decl.span);
                    builder.braces(decl.span);
                    for field in &decl.fields {
                        builder.declared_ty(&field.ty);
                    }
                    builder.layout.blank_after.insert(decl.span.end());
                }
                Item::Interface(decl) => {
                    builder.braces(decl.span);
                    for method in &decl.methods {
                        builder.signature(&method.params, &method.results);
                    }
                    builder.layout.blank_after.insert(decl.span.end());
                }
                Item::Named(decl) => builder.declared_ty(&decl.base),
                Item::Binding(binding) => builder.binding(binding),
            }
        }
        builder.layout
    }
}

impl Builder<'_> {
    fn tokens_in(&self, span: Span) -> &[Token] {
        let start = self
            .tokens
            .partition_point(|t| t.span.start() < span.start());
        let end = self.tokens.partition_point(|t| t.span.start() < span.end());
        &self.tokens[start..end]
    }

    fn braces(&mut self, span: Span) {
        if let Some(open) = self
            .tokens_in(span)
            .iter()
            .find(|t| t.kind == TokenKind::Punct(Punct::LBrace))
        {
            self.layout.blocks.insert(open.span.start());
        }
    }

    fn generics(&mut self, params: &[TypeParam], span: Span) {
        if params.is_empty() {
            return;
        }
        let last = params.last().expect("nonempty type parameters").span.end();
        let end = self
            .tokens_in(self.file.span(last, span.end()).expect("generic end"))
            .iter()
            .find(|t| {
                t.span.start() >= last && matches!(t.kind, TokenKind::Punct(Punct::Gt | Punct::Shr))
            })
            .map(|t| t.span.end())
            .unwrap_or(last);
        self.type_punctuation(
            self.file
                .span(span.start(), end)
                .expect("generic signature"),
        );
        for param in params {
            self.ty(&param.constraint);
        }
    }

    fn type_punctuation(&mut self, span: Span) {
        let punctuation: Vec<_> = self
            .tokens_in(span)
            .iter()
            .filter_map(|token| match token.kind {
                TokenKind::Punct(Punct::Lt | Punct::Gt | Punct::Shr) => Some((token.span, true)),
                TokenKind::Punct(Punct::RBracket) => Some((token.span, false)),
                _ => None,
            })
            .collect();
        for (span, angle) in punctuation {
            if angle {
                self.layout.tight_before.insert(span.start());
            }
            if !angle || self.file.text().as_bytes()[span.start() as usize] == b'<' {
                self.layout.tight_after.insert(span.end());
            }
        }
    }

    fn ty(&mut self, ty: &Type) {
        self.type_punctuation(ty.span());
        match ty {
            Type::Array { size, element, .. } => {
                let separator = self
                    .tokens_in(
                        self.file
                            .span(element.span().end(), size.span.start())
                            .expect("array length separator"),
                    )
                    .iter()
                    .find(|t| matches!(t.kind, TokenKind::Semicolon(_)))
                    .map(|t| t.span.start());
                self.layout.inline_separators.extend(separator);
                self.expr(size);
                self.ty(element);
            }
            Type::DynArray { element, .. }
            | Type::Slice { element, .. }
            | Type::Channel { element, .. }
            | Type::Mutex { element, .. } => self.ty(element),
            Type::Map { key, value, .. } => {
                self.ty(key);
                self.ty(value);
            }
            Type::Func {
                params, results, ..
            } => {
                for param in params {
                    self.ty(&param.ty);
                }
                for result in results {
                    self.ty(result);
                }
                self.results(results);
            }
            Type::Instance { base, args, .. } => {
                self.ty(base);
                for arg in args {
                    self.ty(arg);
                }
            }
            Type::Task { results, .. } => {
                for result in results {
                    self.ty(result);
                }
            }
            Type::Named(_) | Type::Qualified { .. } => {}
        }
    }

    fn signature(&mut self, params: &[Param], results: &[Type]) {
        for param in params {
            self.declared_ty(&param.ty);
        }
        for result in results {
            self.ty(result);
        }
        self.results(results);
    }

    fn results(&mut self, results: &[Type]) {
        if let Some(first) = results.first() {
            let at = self
                .tokens
                .partition_point(|t| t.span.start() < first.span().start());
            if let Some(token) = self.tokens[..at].iter().rfind(|t| !t.span.is_empty()) {
                let start = if token.kind == TokenKind::Punct(Punct::LParen) {
                    token.span.start()
                } else {
                    first.span().start()
                };
                self.layout.spaced_before.insert(start);
            }
        }
    }

    fn declared_ty(&mut self, ty: &Type) {
        self.layout.spaced_before.insert(ty.span().start());
        self.ty(ty);
    }

    fn binding(&mut self, binding: &Binding) {
        if let Some(ty) = &binding.ty {
            self.declared_ty(ty);
        }
        self.expr(&binding.value);
    }

    fn block(&mut self, block: &Block) {
        if !block.span.is_empty() {
            self.braces(block.span);
        }
        for stmt in &block.stmts {
            self.stmt(stmt);
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Binding(binding) => self.binding(binding),
            StmtKind::MixedBinding(binding) => self.expr(&binding.value),
            StmtKind::Assign {
                targets, values, ..
            } => {
                for target in targets {
                    if let AssignTarget::Place(expr) = target {
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
            StmtKind::If(branch) => self.branch(branch),
            StmtKind::For(loop_) => {
                match &loop_.header {
                    ForHeader::Infinite => {}
                    ForHeader::Condition(expr) => self.expr(expr),
                    ForHeader::Counting {
                        init,
                        condition,
                        update,
                    } => {
                        let separators: Vec<_> = self
                            .tokens_in(
                                self.file
                                    .span(init.span.end(), update.span.start())
                                    .expect("loop clauses"),
                            )
                            .iter()
                            .filter(|t| {
                                (t.span.start() < condition.span.start()
                                    || t.span.start() >= condition.span.end())
                                    && matches!(t.kind, TokenKind::Semicolon(_))
                            })
                            .map(|t| t.span.start())
                            .collect();
                        self.layout.inline_separators.extend(separators);
                        self.stmt(init);
                        self.expr(condition);
                        self.stmt(update);
                    }
                    ForHeader::Each { collection, .. } => self.expr(collection),
                }
                self.block(&loop_.body);
            }
            StmtKind::Select(select) => {
                self.braces(select.span);
                for arm in &select.arms {
                    match &arm.comm {
                        SelectComm::Bind(binding) => self.binding(binding),
                        SelectComm::Expr(expr) => self.expr(expr),
                    }
                    self.block(&arm.body);
                }
                if let Some(block) = &select.default {
                    self.block(block);
                }
            }
            StmtKind::TaskScope(block) => {
                self.block(block);
            }
            StmtKind::Block(block) => self.block(block),
            StmtKind::Break | StmtKind::Continue => {}
        }
    }

    fn branch(&mut self, branch: &If) {
        self.expr(&branch.condition);
        self.block(&branch.then_block);
        match &branch.else_branch {
            Some(Else::If(branch)) => self.branch(branch),
            Some(Else::Block(block)) => self.block(block),
            None => {}
        }
    }

    fn expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Unary { operand, .. } => {
                self.layout.unary.insert(expr.span.start());
                let end = self
                    .tokens_in(expr.span)
                    .first()
                    .expect("unary operator")
                    .span
                    .end();
                self.layout.tight_after.insert(end);
                self.expr(operand);
            }
            ExprKind::Binary { lhs, rhs, .. } => {
                let operator = self
                    .tokens_in(
                        self.file
                            .span(lhs.span.end(), rhs.span.start())
                            .expect("binary operands"),
                    )
                    .iter()
                    .find(|t| {
                        t.span.start() >= lhs.span.end()
                            && t.span.start() < rhs.span.start()
                            && matches!(t.kind, TokenKind::Punct(_))
                    })
                    .map(|token| token.span);
                if let Some(span) = operator {
                    self.layout.tight_before.remove(&span.start());
                    self.layout.tight_after.remove(&span.end());
                }
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Paren(expr)
            | ExprKind::Await(expr)
            | ExprKind::Go(expr)
            | ExprKind::Try(expr) => self.expr(expr),
            ExprKind::Call { callee, args } => {
                if let Some(token) = self
                    .tokens_in(
                        self.file
                            .span(callee.span.end(), expr.span.end())
                            .expect("call arguments"),
                    )
                    .iter()
                    .find(|t| {
                        t.span.start() >= callee.span.end()
                            && t.kind == TokenKind::Punct(Punct::LParen)
                    })
                {
                    self.layout.tight_before.insert(token.span.start());
                }
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
            ExprKind::Slice { base, low, high } => {
                self.expr(base);
                if let Some(expr) = low {
                    self.expr(expr);
                }
                if let Some(expr) = high {
                    self.expr(expr);
                }
                let colon_start = low.as_ref().map_or(base.span.end(), |low| low.span.end());
                let colon = self
                    .tokens_in(
                        self.file
                            .span(colon_start, expr.span.end())
                            .expect("slice bounds"),
                    )
                    .iter()
                    .find(|t| {
                        t.span.start() >= colon_start
                            && t.span.end() <= expr.span.end()
                            && t.kind == TokenKind::Punct(Punct::Colon)
                    });
                if let Some(token) = colon {
                    self.layout.tight_after.insert(token.span.end());
                }
            }
            ExprKind::StructLit {
                type_args,
                fields,
                ty,
                ..
            } => {
                if let Some(open) = self
                    .tokens_in(expr.span)
                    .iter()
                    .find(|t| t.kind == TokenKind::Punct(Punct::LBrace))
                {
                    self.type_punctuation(
                        self.file
                            .span(ty.span.start(), open.span.start())
                            .expect("literal type"),
                    );
                }
                for ty in type_args {
                    self.ty(ty);
                }
                for field in fields {
                    self.expr(&field.value);
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
            ExprKind::Closure(closure) => {
                self.signature(&closure.params, &closure.results);
                self.block(&closure.body);
            }
            ExprKind::Channel { element, capacity } => {
                self.type_punctuation(expr.span);
                self.ty(element);
                if let Some(expr) = capacity {
                    self.expr(expr);
                }
            }
            ExprKind::Name(_)
            | ExprKind::Int(_)
            | ExprKind::Float
            | ExprKind::String(_)
            | ExprKind::Rune(_)
            | ExprKind::Bool(_)
            | ExprKind::Nil
            | ExprKind::Malformed => {}
        }
    }
}
