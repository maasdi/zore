use crate::resolve::FunctionId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Suspension {
    Call(FunctionId),
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
        "zore/io.ReadLine" | "zore/os.ReadFile" | "zore/os.ReadBytes" | "zore/os.WriteFile"
        | "zore/os.WriteBytes" | "zore/net.listen" | "zore/net.dial" => Some(NativeWait::Helper),
        "zore/net.accept"
        | "zore/net.read"
        | "zore/net.readBytes"
        | "zore/net.write"
        | "zore/net.writeBytes" => Some(if cfg!(any(target_os = "linux", target_os = "macos")) {
            NativeWait::Socket
        } else {
            NativeWait::Helper
        }),
        _ => None,
    }
}
