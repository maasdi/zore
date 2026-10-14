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
readiness reactor. Windows sockets use nonblocking operations retried on helpers
at intervals up to two milliseconds; async callers suspend while those helpers
wait. Plain waits permit replacement pool workers. Read, write and accept
deadlines produce the same stable timeout errors and preserve partial write
counts. This implementation does not claim IOCP or Unix reactor performance.
Windows timers use cancellable timer threads rather than the shared Unix event
queue, so large populations of simultaneous timers cost additional threads.
Queued helper jobs reserve idle capacity before those workers wake, preventing
several dependent operations from relying on the same idle helper. The helper
pool remains bounded at 512 threads.

`std/path` always uses slash-separated text paths. Windows `std/path/filepath`
accepts both separators, emits backslashes, preserves drive and UNC volumes,
and uses the host to resolve `Abs`. `Separator` is `\\` and `ListSeparator` is
`;`; Linux/macOS retain `/` and `:`. Device/verbatim namespace paths are outside
the `filepath` subset and receive an explicit panic or `Abs` error.

`std/os/signal.NotifyContext` supports SIGINT/SIGTERM on Linux/macOS. On
Windows it returns `signal.NotifyContext: unsupported host` without publishing
a subscription. Consequently, automatic-signal `http.ListenAndServe` and
`ListenAndServeWithConfig`, including the default REST sample entry point,
return that error on Windows. Use `http.ListenAndServeContext` with an
application-owned cancellation context, or `Server.Serve` and `Shutdown`, for
Windows HTTP servers. No Windows signal semantics are claimed.

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

Only Unix-specific shell/signal tests are gated by host; Windows has direct
unsupported-signal coverage. The opt-in measurement harnesses and their Python
pipe/resource checks remain Linux/macOS tools; enabling native Windows CI does
not establish Windows benchmark support.
