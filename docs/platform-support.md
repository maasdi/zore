# Native host support

Zore builds for the host running the compiler. Cross-compilation is outside MVP.
The frontend (`check` and `fmt`) needs no clang or native linker.
Native code generation requires a 64-bit host; other pointer widths receive a
toolchain diagnostic before object generation.

| Host | Rust toolchain | Native linker/toolchain |
| --- | --- | --- |
| Linux | Native Rust 1.98+ | clang LLVM 15+ and host system libraries |
| macOS | Native Rust 1.98+ | Apple clang supporting LLVM opaque-pointer IR and Xcode command-line tools |
| Windows x64 | Rust 1.98+ `x86_64-pc-windows-msvc` | LLVM clang 15+, Visual Studio C++ Build Tools, Windows SDK |

Windows linking uses Rust's MSVC linker discovery instead of passing Unix
arguments to clang. Generated objects target x64 MSVC explicitly. Windows GNU
and ARM hosts receive an actionable native-build diagnostic; they are not
validated configurations. Executable names use `.exe` on Windows. Paths are
passed as separate process arguments, including compiler/source/cache paths with
spaces. `ZORE_CC` and `ZORE_RUSTC` select executable paths, not shell commands.

On Windows, install the Desktop development with C++ tools and Windows SDK,
install LLVM, and open an x64 Developer PowerShell. For example:

```powershell
$env:ZORE_CC = 'C:\Program Files\LLVM\bin\clang.exe'
cargo build --release --locked
.\target\release\zore.exe run examples\hello\main.ore
```

The runtime cache defaults to `%LOCALAPPDATA%\zore\cache` on Windows, the
user's XDG cache folder on Linux, and `~/Library/Caches/zore` on macOS.
`ZORE_CACHE_DIR` overrides its root on every host. A cache write failure falls
back to a temporary runtime build.

## Runtime behavior

Files and processes use Rust's host APIs. Files carry raw bytes; converting to
strings validates UTF-8. Windows permission arguments do not establish POSIX
mode bits, and `Stat.Mode` reports read-only/writable approximations. Process
arguments and working directories are passed directly without shell expansion;
run `cmd.exe` or PowerShell explicitly when a shell is required.
`zore run` forwards full Windows exit statuses, including values outside the
Unix 0–255 range, after removing its temporary program directory.

Tasks, channels, scoped joins, panic cleanup, and deterministic destruction use
the same ownership contract on every host. Linux/macOS sockets use the shared
readiness reactor. Windows accept, read and write operations use nonblocking
sockets and one shared `WSAPoll` service, with a loopback wake connection for
registration changes. Async callers suspend without occupying helper threads;
plain waits permit replacement pool workers. Read, write and accept
deadlines produce the same stable timeout errors and preserve partial write
counts. Registrations and in-flight poll snapshots retain the socket owner until
polling finishes. Dropping a registration removes it and wakes the service;
readiness and deadline completion wake each registration at most once.
This implementation does not claim IOCP or Unix reactor performance.
Windows cancellable deadlines and async sleeps use a separate shared timer
service. Both Windows services retain one idle thread after first use.
Files, standard input, connects and name resolution still use helpers.
Queued helper jobs reserve idle capacity before those workers wake, preventing
several dependent operations from relying on the same idle helper. The helper
pool remains bounded at 512 threads.

`std/path` always uses slash-separated text paths. Windows `std/path/filepath`
accepts both separators, emits backslashes, preserves drive and UNC volumes,
and uses the host to resolve `Abs`. `Separator` is `\\` and `ListSeparator` is
`;`; Linux/macOS retain `/` and `:`. Device/verbatim namespace paths are outside
the `filepath` subset and receive an explicit panic or `Abs` error.

`std/os/signal.NotifyContext` maps `Interrupt`/`Terminate` to SIGINT/SIGTERM
on Linux/macOS. On Windows, `Interrupt` receives Ctrl+C and Ctrl+Break;
`Terminate` receives delivered console close/logoff/shutdown events. Registration
is synchronous, overlapping subscriptions remain independent, and the last
subscription removes Zore's handler so the existing handler chain resumes.
Unselected events pass through that chain. The backend uses direct
`SetConsoleCtrlHandler` bindings without runtime dependencies.

`http.ListenAndServe` and `ListenAndServeWithConfig`, including the REST sample,
bootstrap this support automatically. Ctrl+C/Break permit orderly HTTP draining.
Close/logoff/shutdown cleanup is best effort: the callback publishes notification
and waits up to five seconds without runtime locks while ordinary tasks cancel
contexts and drain requests. It keeps waiting even after subscriptions stop,
allowing subsequent application cleanup. Windows may impose a shorter deadline;
a configured HTTP timeout does not extend the OS deadline.

Console attachment must remain stable while subscribed. Windows delivery rules
apply; applications loading user32/gdi32 may not receive console logoff/shutdown
notifications. Windows service stop controls require a separate service adapter.
Forced termination (`TerminateProcess`, `taskkill /F`) provides no cleanup window.
Normal return, error and panic cleanup retain their ordinary language guarantees.

## Validation

The CI matrix includes Ubuntu, macOS and Windows. The Windows job configures an
x64 MSVC developer environment and LLVM clang, then runs formatting, strict
lint, build, strict documentation checks and all host-applicable Rust tests.
Current runs and per-host results are linked from [PR #134](https://github.com/maasdi/zore/pull/134).

Focused host cases cover source/output/cache paths with spaces and Unicode,
CLI build/run/test, byte files and close-before-remove destruction, child
arguments containing spaces/quotes/backslashes, working directories, captured
non-UTF-8 bytes, combined output and nonzero exit statuses. Existing native
tests exercise task/channel progress, scoped cleanup, sockets, deadlines and
HTTP with explicit shutdown. Windows path rules are also exercised through the
same Zore implementation on Unix hosts.
An isolated helper regression fails with the previous queue rule and passes with
capacity reservation. Repository text checks out with LF endings on every host;
explicit CRLF lexer/parser/scanner cases still exercise Windows-style input.

Real Ctrl+C/Break tests run in isolated Windows consoles and cover overlapping
subscriptions, restored default exit, automatic HTTP bootstrap and active-request
drain. Runtime tests directly invoke all three termination callback mappings and
verify bounded waiting while subscriptions are removed. These synthetic checks
do not establish real close/logoff/shutdown delivery. Unix-specific shell and
POSIX signal tests retain host gates. The opt-in
[runtime population harness](../benchmarks/runtime-scaling/README.md) measures
Windows/Linux/macOS separately from native correctness CI; its
[measured report and limits](runtime-scaling.md) retain raw timer/socket/task
populations and cleanup observations. Earlier performance/retention harnesses
remain Linux/macOS tools.

## Curated native adapter builds

The first `adler32-v1` adapter supports Linux GNU/macOS x86_64 or aarch64 and
Windows x86_64 MSVC. It additionally compiles reviewed C11 source with the same
ZORE_CC/clang, so a complete clang installation with its standard C headers is
required. Windows uses the existing MSVC Build Tools/SDK setup. Checking available
Zore wrappers and validating recipe locks invokes no native toolchain. Other
adapter host/ABI combinations receive an explicit build error. This is host-native
linking; cross-compilation remains excluded. See [native libraries](native-libraries.md).
