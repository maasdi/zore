use super::{Checker, MutableUse, Value, generic};
use crate::hir::ExprKind;
use crate::source::Span;
use crate::types::{TypeId, TypeKind, TypeStore};
use crate::{ast, hir};

impl Checker<'_> {
    pub(super) fn map_with_entry(
        &mut self,
        map: hir::Expr,
        key_ty: TypeId,
        value_ty: TypeId,
        method: &ast::Name,
        args: &[ast::Expr],
        span: Span,
    ) -> Option<Value> {
        if !self.argument_count(&method.text, 2, args, span) {
            return None;
        }
        let mutable = method.text == "withEntryMut";
        let key = self
            .expr(&args[0], Some(key_ty))
            .and_then(|key| self.coerce(key, key_ty));
        let callback = self
            .expr(&args[1], None)
            .and_then(|value| self.with_default_type(value))
            .and_then(|expression| self.single_value(expression));
        let key = key?;
        let callback = callback?;
        if mutable && !self.mutable_place(&map, MutableUse::Argument) {
            return None;
        }
        if self.entry_views_are_forbidden(value_ty, mutable) {
            self.error(
                if mutable {
                    "mutable entries cannot contain borrowed views"
                } else {
                    "shared entry access cannot grant mutable views"
                },
                map.span,
            );
            return None;
        }
        let mode = if mutable {
            ast::ParamMode::Mut
        } else {
            ast::ParamMode::Borrow
        };
        let guarded = match self.types.kind(value_ty) {
            TypeKind::Interface(interface) => self.types.interface_view(interface, mutable),
            _ => value_ty,
        };
        let Some(signature) = self
            .types
            .func_signature(callback.ty())
            .cloned()
            .filter(|signature| !signature.is_async && signature.params == [(mode, guarded)])
        else {
            self.error(
                format!(
                    "`{}` needs a synchronous function taking {}{}",
                    method.text,
                    if mutable { "mut " } else { "" },
                    self.name(value_ty)
                ),
                callback.span,
            );
            return None;
        };
        if self.results_contain_views(&signature.results) {
            self.error(
                "an entry callback cannot return borrowed views or function values",
                callback.span,
            );
            return None;
        }
        if let ExprKind::Local(local) = callback.kind {
            self.mark_exclusive(local);
        }
        Some(Value::Typed(hir::Expr {
            kind: ExprKind::MapWithEntry {
                map: Box::new(map),
                key: Box::new(key),
                callback: Box::new(callback),
                mutable,
            },
            types: std::iter::once(TypeStore::BOOL)
                .chain(signature.results)
                .collect(),
            span,
        }))
    }

    pub(super) fn check_map_entries(&mut self, functions: &mut [hir::Function]) {
        for function in functions.iter_mut().filter(|function| function.compiled()) {
            generic::each_expr(&mut function.body, &mut |expression| {
                let ExprKind::MapWithEntry { map, mutable, .. } = &expression.kind else {
                    return;
                };
                let TypeKind::Map { value, .. } = self.types.kind(map.ty()) else {
                    return;
                };
                if self.entry_views_are_forbidden(value, *mutable) {
                    self.error(
                        if *mutable {
                            "mutable entries cannot contain borrowed views"
                        } else {
                            "shared entry access cannot grant mutable views"
                        },
                        map.span,
                    );
                }
                if self.results_contain_views(&expression.types[1..]) {
                    self.error(
                        "an entry callback cannot return borrowed views or function values",
                        expression.span,
                    );
                }
            });
        }
    }

    fn entry_views_are_forbidden(&self, value: TypeId, mutable: bool) -> bool {
        if mutable {
            self.results_contain_views(&[value])
        } else {
            self.contains_mut_view(value)
        }
    }

    fn results_contain_views(&self, results: &[TypeId]) -> bool {
        results.iter().any(|&ty| {
            self.type_contains(ty, &|kind| {
                matches!(
                    kind,
                    TypeKind::Slice { .. } | TypeKind::Func(_) | TypeKind::InterfaceView { .. }
                )
            })
        })
    }
}
