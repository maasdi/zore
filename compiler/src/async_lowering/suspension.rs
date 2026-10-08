use crate::resolve::FunctionId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Suspension {
    Call(FunctionId),
    Task,
}

pub(super) fn needs_fiber(package: &crate::hir::Package, callee: &crate::mir::Callee) -> bool {
    use crate::mir::Callee;
    match callee {
        Callee::ChannelSend
        | Callee::ChannelReceive
        | Callee::Select { .. }
        | Callee::MutexWithLock => true,
        Callee::Function(id) => {
            let function = package.function(*id);
            function.native
                && (function.name == "zore/time.Sleep"
                    || ["zore/io.", "zore/os.", "zore/net."]
                        .iter()
                        .any(|prefix| function.name.starts_with(prefix)))
        }
        _ => false,
    }
}
