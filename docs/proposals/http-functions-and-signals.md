# Direct HTTP handlers, signal shutdown, and simpler error handling

Status: proposed implementation scope, requested by the maintainer.
The APIs below are targets, not currently implemented signatures. The current
locked specification continues to govern until the contract changes and
conformance cases are prepared under its change policy.

## Problem and scope

The REST example requires a factory and a handler type for each route, several
separate startup error checks, and stdin input to stop the server. The next
implementation PR should make a function the normal handler, support SIGINT and
SIGTERM shutdown, and consolidate common response and startup operations.

Deliver all three changes in this PR. Preserve concurrent request execution,
request/writer lifetime checks, bounded framing and deadlines, and deterministic
cleanup. Factories remain available for applications that construct owned
per-request state.

## Proposed application code

This is the intended user flow after implementation:

```ore
package main

import "std/context"
import "std/errors"
import "std/net/http"
import "std/os/signal"

func health(w mut http.ResponseWriter, r mut http.Request) error {
    return w.Text(200, "ok")
}

func run() error {
    var routes = http.NewRouterBuilder()
    routes.Handle("GET /health", health)
    let router = routes.Build()?

    let signals = [signal.Signal; 2]{signal.Interrupt, signal.Terminate}
    let ctx, stop = signal.NotifyContext(context.Background(), signals[:])?
    let err = http.ListenAndServe(ctx, "127.0.0.1:8080", router, http.DefaultConfig())
    stop()
    return err
}

func main() {
    let err = run()
    if err != nil { println(errors.Message(err)) }
}
```

The single final error boundary reports genuine failures. Normal signal shutdown
returns nil after successful draining. Signal cleanup runs even if startup fails:
the example binds the serving result, calls stop, then returns the result.

## Function registration

The proposed normal entry accepts an owned function with the signature
`func(mut ResponseWriter, mut Request) error`. Registration records its first
route-validation error in the builder; Build returns that error or a validated
router. Subsequent registrations on a failed builder dispose of transferred
values normally and do not replace the first error.

Move the existing factory registration to HandleFactory. Existing interface
handlers and factories remain the advanced path. Keep CallHandler for immediate
borrowing calls where useful.

A registered function must execute concurrently across connections without
calling one mutable closure environment concurrently or holding the registry
mutex during application code or I/O. Calling all handlers under the router
lock is not an acceptable implementation.

The compiler should adapt known declared functions and known closure literals
to private factory/handler implementations. These carry validated captures as
ordinary fields and construct a fresh invocation for each request. The
application should not have to write those wrapper types.

For the first implementation, accepted closure captures are shared reads of
Copy values that contain no views or function values, including channel and
Mutex handles. Read-only captured strings and primitive configuration are also
supported. Capture validation must reject mutable captures, borrowed views,
call-once closures, and unsupported opaque callable origins with source
diagnostics before erasing the callable's capture information. Shared mutable
application data belongs behind a Mutex or a channel. Moving a captured resource
into request execution is handled by an explicit factory.

The adaptation must preserve normal owning-capture behavior and source spans.
It must not relax the general rules for function values in channels, Mutex
storage, generic arguments, or task spawning. Existing private registries can
continue to store owned factory interfaces rather than erased function values.

Lock this narrow registration contract in the HTTP specification before
implementation. Direct registration is currently explicitly excluded there;
the new contract replaces that exclusion.

## Response and startup conveniences

Add Text(status, text) as a ResponseWriter method returning one error. It sets
the text content type and exact UTF-8 byte length, commits the status, and writes
the bytes using existing framing and HEAD/body-eligibility rules.

Add a generic standalone http.JSON writer helper using the existing JSON
serializer; methods cannot introduce type parameters under the current language
contract. Its default naming must agree with json.Marshal; preserve the REST
example's existing lower-camel wire fields through an explicit naming variant.
Serialization must finish before commitment so encoding failure can still
produce the normal generic 500 response.

Keep low-level I/O errors observable and preserve sticky framing failures.
Conveniences should remove repeated setup, not hide failed reads or silently
turn malformed application responses into success.

ListenAndServe owns listener creation, startup, and context-driven draining.
Document a positive graceful-shutdown budget in configuration, defaulting to ten
seconds. It returns actual startup, listener, or drain-expiry errors. A listener
failure must wake the caller even when its context has not been canceled.
Shutdown uses a fresh bounded context after the serving context is canceled;
using the already canceled serving context would abort draining immediately.
On drain expiry, request cooperative cancellation and preserve the existing
limits on interrupting application code.

## OS signals

Add std/os/signal with portable Interrupt and Terminate selectors and
NotifyContext(parent, signals) returning a derived Context, an idempotent stop
function, and an error. Support Linux and macOS initially. Unsupported targets
or selectors return an explicit error. Do not advertise SIGKILL/SIGSTOP support.

SIGINT or SIGTERM cancels the derived context. Stop cancels it, unregisters its
subscription, and releases owned resources. Parent cancellation also releases
the subscription. Multiple subscriptions must not steal events from one
another; ordinary signals may coalesce.

Native signal handlers must not allocate, take runtime mutexes, call Zore code,
or send directly through language channels. They publish an async-signal-safe
notification; ordinary runtime code translates that notification into context
cancellation. First/last subscription transitions must install and restore the
previous process handlers correctly, with registration/stop races accounted for.

The runtime currently has no third-party dependencies and is linked through the
compiler driver. If a signal library is chosen, account for that driver path and
Cargo.lock explicitly. Do not add a dependency that works only in cargo tests
but fails when zore build links a generated executable.

## Verification and migration

- Named function registration and Copy-handle closure registration compile and
  serve requests concurrently; application callbacks run outside registry locks.
- Mutable, borrowed, call-once, and opaque unsupported callbacks produce clear
  diagnostics; their owned captures clean up correctly on registration failure.
- Builder errors are sticky, reported once by Build, and preserve route checks.
- Text/JSON responses retain HEAD, bodyless statuses, length checks, escaping,
  numeric validation, and commitment behavior.
- Listener startup/failure and signal registration failure propagate once;
  normal shutdown returns nil and releases the listener.
- Ready subprocesses receive real SIGINT and SIGTERM and drain active requests
  before exiting successfully. Verify idle connections, timeout behavior,
  repeated stop, multiple subscriptions, parent cancellation, and restored
  handlers in isolated processes.
- Replace the REST example's wrapper types and stdin shutdown, preserving its
  existing routes, JSON format, validation, and store synchronization.
- Update the locked specification, decisions, roadmap, and conformance coverage;
  pass formatting, clippy, build, documentation, and full Ubuntu/macOS tests.

