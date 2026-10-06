//! Scopes: declaring names and looking them up.

use super::ids::{FunctionId, LocalId};
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

    /// Like `lookup`, but a local of an enclosing function seen from inside a
    /// closure resolves to that closure's capture of it.
    fn lookup_capturing(&mut self, name: &str) -> Option<Res> {
        let found = self
            .scopes
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, scope)| scope.get(name).map(|&(res, _)| (index, res)));
        match found {
            Some((scope, Res::Local(local))) => Some(Res::Local(self.capture(scope, local))),
            Some((_, res)) => Some(res),
            None => self.lookup(name),
        }
    }

    /// The local standing for `local`, declared in scope `scope`, inside the
    /// innermost body: a chain of captures through every closure in between.
    fn capture(&mut self, scope: usize, mut local: LocalId) -> LocalId {
        let crossed: Vec<FunctionId> = self
            .frames
            .iter()
            .filter(|frame| frame.scope_base > scope)
            .map(|frame| frame.function)
            .collect();
        for closure in crossed {
            let index = closure.0 as usize - self.out.functions.len();
            if let Some(&(_, existing)) = self.out.closures[index]
                .captures
                .iter()
                .find(|(outer, _)| *outer == local)
            {
                local = existing;
                continue;
            }
            let parent = self.out.closures[index].parent;
            let outer = &self.out.locals[parent.0 as usize][local.0 as usize];
            let decl = LocalDecl {
                name: outer.name.clone(),
                span: outer.span,
                kind: LocalKind::Capture(local),
            };
            let locals = &mut self.out.locals[closure.0 as usize];
            let captured = LocalId(locals.len() as u32);
            locals.push(decl);
            self.out.closures[index].captures.push((local, captured));
            local = captured;
        }
        local
    }

    pub(super) fn use_name(&mut self, name: &str, span: Span) -> Option<Res> {
        let Some(res) = self.lookup_capturing(name) else {
            self.error(format!("cannot find `{name}` in this scope"), span);
            return None;
        };
        if res == Res::Unsupported && name == "Array" {
            self.error(
                "`Array` needs an element type, as in `Array<int>` (§6.2)",
                span,
            );
        } else if res == Res::Unsupported {
            self.unsupported(unsupported_predeclared(name), span, "M19–M25");
        }
        self.out.uses.insert(span, res);
        Some(res)
    }
}
