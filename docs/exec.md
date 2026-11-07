# Environment and subprocesses

`std/os` reads the current process environment. `LookupEnv` distinguishes an
unset key from a present empty value; `Getenv` returns an empty string for both.
`Environ` returns a sorted, owned `key=value` snapshot. Editing it changes only
that array. Zore does not expose global environment mutation.

`std/os/exec` passes arguments directly to a program. It never invokes a shell
or expands `$HOME`, quotes, globs, or redirections. `Command` prepends the program
name to its argument array. Set `Cmd.Dir` to select the child working directory.
For deterministic program selection across hosts, use an absolute executable
path, especially when replacing or clearing the child PATH.

## Simple helpers

`Run` waits with stdin, stdout, and stderr connected to nothing. `Output` captures
stdout; `CombinedOutput` captures stdout and stderr through a shared pipe. Both
return captured bytes even when the program fails. Nonzero exits return
`exit status N`; Unix signals return `signal: N`; host failures begin `exec: `.

Captures retain at most 16 MiB by default. `OutputLimit(n)` and
`CombinedOutputLimit(n)` accept limits from zero through 1 GiB. On overflow,
they retain exactly the requested prefix, return `exec: output limit exceeded`,
and kill and reap the direct child. Invalid limits fail before spawning. A
zero-byte limit accepts empty output and detects the first output byte. Use
streaming for output larger than the capture limit.

This changes the earlier unbounded capture behavior. Existing `Cmd` literals,
`Command`, `Run`, `Output`, and `CombinedOutput` keep their signatures.

## Configure and own a child

```zore
import "std/os/exec"

var options = exec.DefaultOptions()
options.Env.push("MODE=worker")
options.Env.push("OPTIONAL=")
let child, err = exec.Command("/absolute/helper", Array<string>{"literal $HOME"}[:]).StartWith(options)
if err != nil { return err }
let status, failure = child.Wait()
```

Environment overrides apply only to this child. Later duplicate keys win using
the host's key comparison rules; Windows compares keys without case. Set
`ClearEnv` to start with only the supplied entries. Entries need a nonempty key
followed by `=` and cannot contain NUL. Invalid configuration returns an error
without spawning. Host environment strings use UTF-8 replacement for invalid
encodings.

`Start()` uses default options. `StartContext(ctx, options)` checks `ctx.Err()`
before spawning and observes later cancellation. Cancellation terminates and
reaps the direct child and returns the context's error from `Wait`. A child exit
already observed by the runtime keeps its original result when cancellation
arrives later. The completion monitor ends on natural exit, cancellation, or
cleanup, including when the context is `Background()`.

`Process` is an owned Move value. It cannot be copied, and a closed or failed
handle cannot operate on a later process. `Wait` returns `Status` and an error;
repeated waits return the same cached result. `Status.Success()`, `ExitCode()`,
and `Signal()` retain the host's exit facts even on failure. An unavailable exit
code is `-1`; signals are Unix signal numbers and otherwise zero. `Kill` requests
forceful termination and succeeds for an already exited child. `Close` consumes
the handle, closes untaken pipes, kills a running child, and waits for reaping.
Dropping a process performs the same cleanup. Natural exit is reaped even if
no caller invokes `Wait`.

## Stream through std/io

Request pipes in options before starting. Each corresponding accessor transfers
one owned endpoint, exactly once. `PipeReader` implements `io.ReadCloser`;
`PipeWriter` implements `io.WriteCloser`. Dropping an endpoint closes it. Taken
endpoints can outlive `Wait`; untaken endpoints close with the process.

```zore
async func collect(reader own exec.PipeReader) (Array<byte>, error) {
    var input = reader
    return io.ReadAll(input)
}

var options = exec.DefaultOptions()
options.StdinPipe = true
options.StdoutPipe = true
options.StderrPipe = true
let child = command.StartWith(options)?
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
```

Close stdin when the child should see EOF. Drain stdout and stderr concurrently
when both are piped, before waiting. A child can block writing a full output pipe;
waiting before reading, or draining the two streams sequentially, can deadlock.
`Wait` waits for child exit, not for readers to finish. For bounded retention
with custom streaming, `io.ReadAtMost` returns a prefix and `io.LimitExceeded`;
the caller must then close or kill the child rather than leave its writer
blocked.

## Host limits

Pipe reads, writes, and capture use the runtime's bounded blocking-helper pool.
Child completion is checked by an asynchronous task that suspends on a short
timer between nonblocking exit checks. Wait and explicit Close await a shared
completion channel, so neither a running child nor each waiter reserves a
helper. Spawn, Kill, cancellation, exit checks, and process release execute
outside the helper queue: blocked pipe jobs cannot prevent them from releasing
the children those jobs depend on. Spawn and fallback native cleanup compensate
scheduler workers during their brief synchronous host operations; implicit drop
kills first and compensates its synchronous completion wait.

Host thread, process, descriptor, argument-size, environment-size, and permission
limits can cause errors. Pipe read allocations are capped at 64 KiB per operation.

On Linux and macOS, forceful termination sends SIGKILL; on Windows it uses
TerminateProcess. Neither mechanism runs child cleanup handlers. The runtime
waits for and reaps the direct child after termination.

Termination affects only the direct child. Descendants can survive and retain
inherited pipe handles, preventing EOF even after the child exits. The API does
not provide process groups, Windows job objects, graceful signals, inherited
standard streams, or process-tree cleanup. Arrange descendant ownership in the
program being launched when those guarantees are needed.
