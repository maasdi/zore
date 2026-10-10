use crate::ast::{self, BindingKind, BindingTarget, Item};

use super::units::PackageUnit;

/// A package-level `let` and the function whose body computes its value.
pub struct Initializer<'a> {
    pub binding: &'a ast::Binding,
    pub function: ast::FuncDecl,
}

/// One per package-level `let` with a single name, in package and declaration order.
pub fn initializers<'a>(units: &[PackageUnit<'a>]) -> Vec<Initializer<'a>> {
    let mut found = Vec::new();
    for unit in units {
        for file in &unit.files {
            for item in &file.file.items {
                let Item::Binding(binding) = item else {
                    continue;
                };
                let [BindingTarget::Name(name)] = &binding.targets[..] else {
                    continue;
                };
                if binding.kind != BindingKind::Let {
                    continue;
                }
                found.push(Initializer {
                    binding,
                    function: ast::FuncDecl {
                        is_async: false,
                        receiver: None,
                        name: ast::Name {
                            text: format!("{}$init", name.text),
                            span: name.span,
                        },
                        params: Vec::new(),
                        results: binding.ty.iter().cloned().collect(),
                        body: ast::Block {
                            stmts: vec![ast::Stmt {
                                kind: ast::StmtKind::Return(vec![binding.value.clone()]),
                                span: binding.value.span,
                            }],
                            span: binding.span,
                        },
                        native: false,
                        span: binding.span,
                    },
                });
            }
        }
    }
    found
}
