# Environment and subprocesses

`std/os` reads the current process environment. `Getenv` returns the value and
whether the variable is set, so an unset variable and one set to an empty value
are told apart, the same way `url.Values.Get` and `http.Header.Get` report
presence. Write `let value, _ = os.Getenv("NAME")` when presence does not
matter. `Environ` returns a sorted, owned `key=value` snapshot. Editing it
changes only that array. Zore does not expose global environment mutation.

`std/os/exec` passes arguments directly to a program. It never invokes a shell
or expands `$HOME`, quotes, globs, or redirections. `Command` prepends the program
name to its argument array. Set `Cmd.Dir` to select the child working directory.
For deterministic program selection across hosts, use an absolute executable
path, especially when replacing or clearing the child PATH.

## Run a program

| You want to | Call |
| --- | --- |
| Run it and wait | `cmd.Run()` or `cmd.RunWith(ctx, options)` |
| Get what it prints | `cmd.Output()` or `cmd.OutputWith(ctx, options)` |
| Get what it prints, errors included, in order | `cmd.CombinedOutput()` or `cmd.CombinedOutputWith(ctx, options)` |
| Talk to it while it runs | `cmd.Start(ctx, options)` |

The short forms use `context.Background()` and `DefaultOptions()`. The `With`
forms take a context, so a deadline or cancellation stops the child, and
options, so you can set its environment and streams:

```ore
let ctx, stop = context.WithTimeout(context.Background(), 30 * time.Second)
var options = exec.DefaultOptions()
options.SetEnv("RUST_LOG", "info")
options.Stdout = exec.Inherit
options.Stderr = exec.Inherit
let err = exec.Command("cargo", Array<string>{"build"}[:]).RunWith(ctx, options)
stop()
```

Nonzero exits return `exit status N`; Unix signals return `signal: N`; host
failures begin `exec: `. A canceled or expired context returns its own error.

## Where the child's streams go

Each of `Stdin`, `Stdout`, and `Stderr` in `Options` is a `Stream`:

| Stream | Meaning |
| --- | --- |
| `exec.Null` (the default) | Connected to nothing: no input, and output is thrown away |
| `exec.Inherit` | Shares this program's own stream, so the child prints to the same terminal or file |
| `exec.Pipe` | A pipe this program reads or writes after `Start` |

`RunWith` refuses `Pipe`, because nothing would read it. The capture helpers
take over the streams they capture: `OutputWith` captures standard output, and
`CombinedOutputWith` captures both output and error through one shared pipe in
the order they were written. The other streams follow the options, but cannot
be `Pipe`.

Captures keep at most `Options.OutputLimit` bytes, 16 MiB by default, and
accept limits from zero through 1 GiB. On overflow they return exactly the first
`OutputLimit` bytes with `exec: output limit exceeded`, and kill and reap the
child. Invalid limits fail before spawning. Use `Start` with `Pipe` for output
larger than any limit.

Earlier versions captured without a limit. Existing `Cmd` literals, `Command`,
`Run`, `Output`, and `CombinedOutput` keep their signatures.

## The child's environment

```ore
var options = exec.DefaultOptions()
options.SetEnv("MODE", "worker")
options.SetEnv("OPTIONAL", "")
```

Environment entries apply only to this child. `SetEnv` panics when the key is
empty or contains `=` or NUL, because that is a mistake in the program. Entries
added to `Env` directly need a nonempty key followed by `=` and cannot contain
NUL; invalid ones return an error without spawning. Later duplicate keys win
using the host's key comparison rules; Windows compares keys without case. Set
`ClearEnv` to start with only the supplied entries. Host environment strings
use UTF-8 replacement for invalid encodings.

## Own a running child

`Start(ctx, options)` checks `ctx.Err()` before spawning and watches for later
cancellation. Cancellation terminates and reaps the direct child and returns the
context's error from `Wait`. A child exit already observed by the runtime keeps
its original result when cancellation arrives later. The completion monitor
ends on natural exit, cancellation, or cleanup, including when the context is
`Background()`.

`Process` is an owned Move value. It cannot be copied, and a closed or failed
handle cannot operate on a later process. `Pid` returns the host process
identifier, or 0 for a closed or failed handle. `Wait` returns `Status` and an
error; repeated waits return the same cached result. `Status.Success()`,
`ExitCode()`, and `Signal()` retain the host's exit facts even on failure. An
unavailable exit code is `-1`; signals are Unix signal numbers and otherwise
zero. `Kill` requests forceful termination and succeeds for an already exited
child. `Close` consumes the handle, closes untaken pipes, kills a running child,
and waits for reaping. Dropping a process performs the same cleanup. Natural
exit is reaped even if no caller invokes `Wait`.

## Stream through std/io

Set a stream to `exec.Pipe` before starting, then take its endpoint with
`StdinPipe`, `StdoutPipe`, or `StderrPipe`, exactly once. Taking a stream that
is not `Pipe` fails. `PipeReader` implements `io.ReadCloser`; `PipeWriter`
implements `io.WriteCloser`. Dropping an endpoint closes it. Taken endpoints
can outlive `Wait`; untaken endpoints close with the process.

```ore
async func collect(reader own exec.PipeReader) (Array<byte>, error) {
    var input = reader
    return io.ReadAll(input)
}

func talk(command exec.Cmd) error {
    var options = exec.DefaultOptions()
    options.Stdin = exec.Pipe
    options.Stdout = exec.Pipe
    options.Stderr = exec.Pipe
    let child = command.Start(context.Background(), options)?
    var input = child.StdinPipe()?
    let output = child.StdoutPipe()?
    let diagnostic = child.StderrPipe()?
    let stdoutTask = go collect(output)
    let stderrTask = go collect(diagnostic)
    let _, writeErr = io.WriteString(input, "request")
    let closeErr = input.Close()
    let stdout, readErr = stdoutTask.wait()
    let stderr, diagnosticErr = stderrTask.wait()
    let status, waitErr = child.Wait()
    _ = writeErr
    _ = closeErr
    _ = stdout
    _ = readErr
    _ = stderr
    _ = diagnosticErr
    _ = status
    return waitErr
}
```

Close stdin when the child should see EOF. Drain stdout and stderr concurrently
when both are piped, before waiting. A child can block writing a full output pipe;
waiting before reading, or draining the two streams sequentially, can deadlock.
`Wait` waits for child exit, not for readers to finish. For bounded retention
with custom streaming, `io.ReadAtMost` returns a prefix and `io.LimitExceeded`;
the caller must then close or kill the child rather than leave its writer
blocked. When only one stream needs reading, set the other to `Inherit` or
`Null` instead of `Pipe`.

## Host limits

Pipe reads and writes use the runtime's bounded blocking-helper pool; the
capture helpers read through the same pipes. A child's exit is noticed within
50 ms by an asynchronous task that checks without blocking, waiting a little
longer between checks the longer the child runs. Wait and explicit Close await
a shared completion channel, so neither a running child nor each waiter
reserves a helper. Spawn, Pid, Kill, cancellation, exit checks, and process
release execute outside the helper queue: blocked pipe jobs cannot prevent them
from releasing the children those jobs depend on. Spawn and fallback native
cleanup compensate scheduler workers during their brief synchronous host
operations; implicit drop kills first and compensates its synchronous
completion wait.

Host thread, process, descriptor, argument-size, environment-size, and permission
limits can cause errors. Pipe read allocations are capped at 64 KiB per operation.

On Linux and macOS, forceful termination sends SIGKILL; on Windows it uses
TerminateProcess. Neither mechanism runs child cleanup handlers. The runtime
waits for and reaps the direct child after termination.

Termination affects only the direct child. Descendants can survive and retain
inherited pipe handles, preventing EOF even after the child exits. The API does
not provide process groups, Windows job objects, graceful signals, or
process-tree cleanup. Use `Pid` to arrange such cleanup with other tools, and
arrange descendant ownership in the program being launched when those
guarantees are needed.
