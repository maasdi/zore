//! Scopes: declaring names and looking them up.

use super::ids::LocalId;
use super::resolver::Resolver;
use super::symbol::{LocalDecl, LocalKind, Res, predeclared, unsupported_predeclared};
use crate::ast;
use crate::diagnostic::{Diagnostic, Severity};
use crate::source::Span;

impl Resolver<'_> {
    pub(super) fn shadows_predeclared(&mut self, name: &ast::Name) -> bool {
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

    pub(super) fn duplicate(&mut self, name: &ast::Name, first: Span, what: &str) {
        self.out.diagnostics.push(
            Diagnostic::new(
                Severity::Error,
                format!("duplicate {what} `{}`", name.text),
                name.span,
            )
            .related(first, "first declared here"),
        );
    }

    pub(super) fn declare_package(&mut self, name: &ast::Name, res: Res) {
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

    pub(super) fn declare_local(&mut self, name: &ast::Name, res: Res) {
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

    pub(super) fn new_local(&mut self, name: &ast::Name, kind: LocalKind) {
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

    pub(super) fn lookup(&self, name: &str) -> Option<Res> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .or_else(|| self.package_scope.get(name))
            .map(|&(res, _)| res)
            .or_else(|| predeclared(name))
    }

    pub(super) fn use_name(&mut self, name: &str, span: Span) -> Option<Res> {
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
}
