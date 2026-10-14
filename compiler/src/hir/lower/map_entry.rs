use super::{Checker, MutableUse, generic};
use crate::hir::ExprKind;
use crate::resolve::Res;
use crate::types::{TypeId, TypeKind, TypeStore};
use crate::{ast, hir};

impl Checker<'_> {
    pub(super) fn map_entry(
        &mut self,
        binding: &ast::EntryBinding,
        expression: &ast::Expr,
    ) -> Option<hir::Expr> {
        let ast::ExprKind::Call { callee, args, .. } = &expression.kind else {
            self.error(
                "an entry binding requires `map.entry(key)`",
                expression.span,
            );
            return None;
        };
        let ast::ExprKind::Field { base, name } = &callee.kind else {
            self.error(
                "an entry binding requires `map.entry(key)`",
                expression.span,
            );
            return None;
        };
        if name.text != "entry" || !self.argument_count("entry", 1, args, expression.span) {
            if name.text != "entry" {
                self.error(
                    "an entry binding requires `map.entry(key)`",
                    expression.span,
                );
            }
            return None;
        }
        let map = self
            .expr(base, None)
            .and_then(|value| self.with_default_type(value))
            .and_then(|expression| self.single_value(expression))?;
        let TypeKind::Map { key, value } = self.types.kind(map.ty()) else {
            self.error("an entry binding requires a map receiver", map.span);
            return None;
        };
        let key = self
            .expr(&args[0], Some(key))
            .and_then(|expression| self.coerce(expression, key))?;
        if binding.mutable && !self.mutable_place(&map, MutableUse::Argument) {
            return None;
        }
        if self.entry_views_are_forbidden(value, binding.mutable) {
            self.error(
                if binding.mutable {
                    "mutable entries cannot contain borrowed views"
                } else {
                    "shared entry access cannot grant mutable views"
                },
                map.span,
            );
            return None;
        }
        let Some(Res::Local(local)) = self.res.declarations.get(&binding.name.span).copied() else {
            return None;
        };
        self.set_local(&binding.name, value);
        Some(hir::Expr {
            kind: ExprKind::MapEntry {
                map: Box::new(map),
                key: Box::new(key),
                binding: local,
                mutable: binding.mutable,
            },
            types: vec![TypeStore::BOOL],
            span: expression.span,
        })
    }

    pub(super) fn check_map_entries(&mut self, functions: &mut [hir::Function]) {
        for function in functions.iter_mut().filter(|function| function.compiled()) {
            generic::each_expr(&mut function.body, &mut |expression| {
                let ExprKind::MapEntry { map, mutable, .. } = &expression.kind else {
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
            });
        }
    }

    fn entry_views_are_forbidden(&self, value: TypeId, mutable: bool) -> bool {
        if mutable {
            self.type_contains(value, &|kind| {
                matches!(
                    kind,
                    TypeKind::Slice { .. } | TypeKind::Func(_) | TypeKind::InterfaceView { .. }
                )
            })
        } else {
            self.contains_mut_view(value)
        }
    }
}
