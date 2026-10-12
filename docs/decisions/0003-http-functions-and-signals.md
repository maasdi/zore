# 0003 — Direct HTTP callbacks and signal-driven shutdown

Status: accepted; contracts locked in specification §37.6 and §37.8.

RouterBuilder.Handle accepts declared function names and inline closures with
shared-read Copy captures containing no views or function values. Registration
errors are retained by the builder and reported once by Build. HandleFactory
preserves the explicit advanced factory API.

A typed HIR pass turns each accepted registration into a private Copy record
holding the captures. Generated factory and handler methods use the same owned
interface representation as existing HTTP routes. Each request gets a fresh
invocation; the original callback executes after registry unlock. Normal MIR
ownership checking and deterministic cleanup still process the generated code.
The compiler-only Handle declaration is a non-emitted entry, and every call to
it must adapt successfully. Opaque locals/parameters, mutable captures, Move
captures, call-once callbacks, and views are rejected with source diagnostics.
No general function type, generic, channel, or mutex restriction is relaxed.

Text and JSON helpers preserve observable errors and HTTP framing rules.
ListenAndServeContext consolidates construction, listener ownership, result
reporting, and cancellation. Graceful draining uses a new context with a default
ten-second timeout, so an already canceled serving context does not abort it.
Drain expiry requests cooperative cancellation and returns instead of waiting
indefinitely for arbitrary handler code.
ListenAndServe(address, router) is the application bootstrap: it uses default
configuration, registers SIGINT/SIGTERM, calls ListenAndServeContext, and waits
for subscription cleanup before returning either success or a serving/startup
error. ListenAndServeWithConfig accepts explicit configuration. Both need no
application signal or context setup. NewServer and ListenAndServeContext retain
explicit lifecycle control for other shutdown sources. Separate entry points
preserve the language's fixed-arity calls without optional-parameter syntax.

Signal notification uses platform sigaction bindings and lock-free generation
counters. The OS callback allocates nothing, acquires no runtime locks, and
calls no Zore code. Ordinary tasks poll each subscription at bounded intervals;
independent snapshots let overlapping subscriptions observe the same signal.
Reference counts preserve and restore full prior dispositions. WithCancelOn
bridges the notification channel into a context without spawning an opaque
cancel closure. The returned stop function waits for native unsubscription.
There are no new dependencies, and the compiler embeds the signal source when
building standalone executables.

Tests cover concurrent callbacks, immutable capture snapshots, rejected
captures, sticky registration, response framing, startup failure, real SIGINT
and SIGTERM, overlapping subscriptions, default-disposition restoration,
parent cancellation, repeated stop, and the migrated REST example.
