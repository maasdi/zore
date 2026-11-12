use crate::hir;
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
    Deadline,
}

pub fn native_wait(function: &hir::Function) -> Option<NativeWait> {
    function
        .native_wait
        .or_else(|| standard_wait(&function.name))
}

fn standard_wait(name: &str) -> Option<NativeWait> {
    match name {
        "std/context.waitFor" | "std/time.waitUntil" => Some(NativeWait::Deadline),
        "std/os.ReadFile"
        | "std/os.WriteFile"
        | "std/os.MkdirAll"
        | "std/os.Remove"
        | "std/os.RemoveAll"
        | "std/os.Rename"
        | "std/os.dirNames"
        | "std/os.statFields"
        | "std/os.fileOpenWith"
        | "std/os.mkdirNew"
        | "std/os.fileRead"
        | "std/os.fileWrite"
        | "std/os.fileWriteString"
        | "std/os.fileSeek"
        | "std/os.fileSync"
        | "std/os.fileStat"
        | "std/os.fileClose"
        | "std/os.treeStage"
        | "std/os.treeMkdir"
        | "std/os.treeCreate"
        | "std/os.treeLink"
        | "std/os.treeWrite"
        | "std/os.treeFinishFile"
        | "std/os.treeCommit"
        | "std/os.treeAbort"
        | "std/os/exec.pipeRead"
        | "std/os/exec.pipeWrite"
        | "std/regexp.compile"
        | "std/regexp.search"
        | "std/regexp.replace"
        | "std/crypto/rand.secureRead"
        | "std/net.listen"
        | "std/net.dial"
        | "std/net.lookupText" => Some(NativeWait::Helper),
        "std/net.accept" | "std/net.read" | "std/net.write" => Some(
            if cfg!(any(target_os = "linux", target_os = "macos", windows)) {
                NativeWait::Socket
            } else {
                NativeWait::Helper
            },
        ),
        _ => None,
    }
}
