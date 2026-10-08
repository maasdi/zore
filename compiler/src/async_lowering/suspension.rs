use crate::resolve::FunctionId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Suspension {
    Call(FunctionId),
    Task,
    Channel,
    Sleep,
}

pub(super) fn needs_fiber(package: &crate::hir::Package, callee: &crate::mir::Callee) -> bool {
    use crate::mir::Callee;
    match callee {
        Callee::MutexWithLock => true,
        Callee::Function(id) => {
            let function = package.function(*id);
            function.native
                && ["zore/io.", "zore/os.", "zore/net."]
                    .iter()
                    .any(|prefix| function.name.starts_with(prefix))
        }
        _ => false,
    }
}
