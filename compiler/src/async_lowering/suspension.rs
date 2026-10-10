use crate::resolve::FunctionId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Suspension {
    Call(FunctionId),
    /// An awaited call through a value of async function type.
    Value,
    /// A call through an interface value, served by the entry's start adapter.
    Interface(usize),
    Task,
    Channel,
    Sleep,
    Mutex,
    Io(FunctionId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeWait {
    Helper,
    Socket,
}

pub fn native_wait(name: &str) -> Option<NativeWait> {
    match name {
        "zore/os.ReadFile"
        | "zore/os.WriteFile"
        | "zore/os.MkdirAll"
        | "zore/os.Remove"
        | "zore/os.RemoveAll"
        | "zore/os.dirNames"
        | "zore/os.statFields"
        | "zore/os.fileOpen"
        | "zore/os.fileRead"
        | "zore/os.fileWrite"
        | "zore/os.fileWriteString"
        | "zore/os.fileClose"
        | "zore/os/exec.run"
        | "zore/net.listen"
        | "zore/net.dial" => Some(NativeWait::Helper),
        "zore/net.accept" | "zore/net.read" | "zore/net.write" => {
            Some(if cfg!(any(target_os = "linux", target_os = "macos")) {
                NativeWait::Socket
            } else {
                NativeWait::Helper
            })
        }
        _ => None,
    }
}
