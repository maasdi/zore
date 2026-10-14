use std::collections::{BTreeSet, HashSet};

use super::{Analysis, Findings, Holdings, LoanId, Site};
use crate::ast::ParamMode;
use crate::diagnostic::{Diagnostic, Severity};
use crate::mir::{Local, Operand, Place, Rvalue, Statement};
use crate::ownership::borrow::{Loan, LoanKind, LoanTarget, Path};
use crate::resolve::FunctionId;
use crate::source::Span;
use crate::types::TypeKind;

impl Analysis<'_> {
    pub(super) fn borrowed_capture_loans(
        &mut self,
        captures: &[(Place, bool)],
        modes: &[ParamMode],
        site: &mut Site,
        state: &Holdings,
    ) -> BTreeSet<LoanId> {
        let mut loans = BTreeSet::new();
        for (index, (place, _)) in captures.iter().enumerate() {
            let Some(mode) = modes.get(index) else {
                continue;
            };
            if *mode == ParamMode::Own {
                continue;
            }
            let path = Path::of(self.package, self.body, place);
            loans.extend(self.inherited_loans(place, &path, state));
            let loan = Loan {
                kind: if *mode == ParamMode::Mut {
                    LoanKind::Exclusive
                } else {
                    LoanKind::Shared
                },
                target: LoanTarget::Place(path),
                span: site.span,
                name: self.describe(place),
                from_slicing: false,
                captured: true,
                binding: false,
            };
            loans.insert(self.intern(site.next_key(), loan));
        }
        loans
    }

    pub(super) fn scope_contains(&self, scope: &crate::mir::TaskScope, local: Local) -> bool {
        if self.body.returns.contains(&local) {
            return false;
        }
        if let Some(decl) = self
            .package
            .function(self.body.function)
            .locals
            .get(local.0 as usize)
        {
            return decl.span.file() == scope.span.file()
                && decl.span.start() >= scope.span.start()
                && decl.span.end() <= scope.span.end();
        }
        local.0 >= scope.temp_start && local.0 < scope.temp_end
    }

    pub(super) fn check_scope_assignment(
        &self,
        target: &Place,
        loans: &BTreeSet<LoanId>,
        span: Span,
        findings: &mut Findings,
    ) {
        for &id in loans {
            if let LoanTarget::Scope { registry, .. } = self.loans[id].target {
                let scope = self
                    .body
                    .task_scopes
                    .iter()
                    .find(|scope| scope.registry == registry)
                    .expect("scope metadata");
                if !self.scope_contains(scope, target.local)
                    || !target.projections.is_empty()
                        && matches!(
                            self.package.types.kind(self.place_ty(target)),
                            TypeKind::ScopedTask(_, _)
                        )
                {
                    findings.report_once(
                        span,
                        Diagnostic::new(
                            Severity::Error,
                            "a scoped task handle or result view cannot escape its task scope",
                            span,
                        ),
                    );
                }
            }
        }
        if let TypeKind::ScopedTask(_, id) = self.package.types.kind(self.place_ty(target)) {
            let own = self
                .body
                .task_scopes
                .iter()
                .filter(|scope| self.scope_contains(scope, target.local))
                .min_by_key(|scope| scope.span.end() - scope.span.start());
            if own.is_some_and(|scope| scope.id != id) {
                findings.report_once(
                    span,
                    Diagnostic::new(
                        Severity::Error,
                        "a scoped task handle cannot move into another task scope",
                        span,
                    ),
                );
            }
        }
    }

    fn closure_definition(&self, initial: &Place) -> Option<(&Rvalue, FunctionId)> {
        let mut place = initial.clone();
        let mut seen = HashSet::new();
        while seen.insert(place.local) {
            let mut assignments = self
                .body
                .blocks
                .iter()
                .flat_map(|block| &block.statements)
                .filter_map(|statement| match statement {
                    Statement::Assign {
                        place: target,
                        rvalue,
                        ..
                    } if target == &place => Some(rvalue),
                    _ => None,
                });
            let definition = assignments.next()?;
            if assignments.next().is_some() {
                return None;
            }
            match definition {
                Rvalue::Closure { function, .. } => return Some((definition, *function)),
                Rvalue::Use(Operand::Move(source) | Operand::Copy(source)) => {
                    place = source.clone()
                }
                _ => return None,
            }
        }
        None
    }

    pub(super) fn check_scoped_results(
        &self,
        closure: &Operand,
        span: Span,
        findings: &mut Findings,
    ) {
        let (Operand::Move(place) | Operand::Copy(place)) = closure else {
            return;
        };
        let Some((
            Rvalue::Closure {
                captures,
                capture_modes: borrowed,
                ..
            },
            function,
        )) = self.closure_definition(place)
        else {
            return;
        };
        for origins in &self.contracts[function.0 as usize].results {
            for (index, origin) in origins.iter().enumerate() {
                if !borrowed
                    .get(index)
                    .is_none_or(|mode| *mode == ParamMode::Own)
                {
                    continue;
                }
                let mut child_owned = origin.storage;
                if origin.contents
                    && let Some((callee, _)) = captures.get(index)
                    && matches!(
                        self.package.types.kind(self.place_ty(callee)),
                        TypeKind::Func(_)
                    )
                {
                    child_owned |= match self.closure_definition(callee) {
                        Some((
                            Rvalue::Closure {
                                capture_modes: modes,
                                owning,
                                ..
                            },
                            target,
                        )) => {
                            let params = self.package.function(target).params.len();
                            *owning
                                && self.contracts[target.0 as usize]
                                    .results
                                    .iter()
                                    .any(|result| {
                                        result[params..].iter().enumerate().any(
                                            |(capture, origin)| {
                                                origin.storage
                                                    && modes
                                                        .get(capture)
                                                        .is_none_or(|mode| *mode == ParamMode::Own)
                                            },
                                        )
                                    })
                        }
                        _ => true,
                    };
                }
                if child_owned {
                    findings.report_once(span, Diagnostic::new(Severity::Error, "a scoped task result cannot view child-owned or unproven callable storage", span));
                }
            }
        }
    }
}
