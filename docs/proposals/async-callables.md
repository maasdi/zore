# Async closures and method values

Issue [#117](https://github.com/maasdi/zore/issues/117) extends the locked callable
model. Specification §§16.1–16.7 defines the source contract.

```ore
let load = async func(id int) (User, error) {
    return await fetchUser(id)
}
(let user, var err) = await load(7)

let update = store.Update
err = await update(user)
```

The async property belongs to each literal and its function type. It never
comes from the surrounding body. Plain nested callbacks still block on waiting
operations; explicit async literals suspend. Calls must be awaited or spawned.

Captures use the existing shared, exclusive and owning inference. Awaited calls
hold the callable exclusively through suspension. Borrowed captures retain their
whole-local loans until their last use, and owning captures remain in the
callable's environment across repeated calls. A receiver field path captures its
whole root local; an `own` receiver that consumes a Move value makes a call-once
callable. Methods on concrete structs and named types use the same forwarding
body. Generic and interface method values keep their existing restrictions;
wrap a direct call in an async literal when allowed by ordinary checking.

Spawning transfers the callable into task-owned storage. It follows the same
independent-input proof as declared async function values: views, borrowed Move
parameters, mutable parameters and unknown function parameters cannot be smuggled
into detached work. A task cannot mutate the caller by changing its copied
capture. These rules do not introduce scoped tasks or implicit joins.

The compiler shares the existing callable triple and pinned heap-frame ABI.
An async callable's code pointer constructs a child frame. That frame borrows
the callable's environment, so freeing a completed child must leave its reusable
captures intact. The generated spawn wrapper owns the transferred callable and
releases it once its child finishes, including errors and panic. Call-once
calls use the existing consuming MIR call and cleanup flags.

Implementation stages within one PR:
1. Lock syntax, types, captures, suspension, spawn and cleanup semantics.
2. Preserve async flags in both parsers, body contexts and method forwarding;
   reuse existing capture inference, region checks and async lowering.
3. Distinguish awaited callable frames from owning spawn wrappers and validate
   normal/error/panic cleanup, replacement and argument initialization failures.

See the [conformance inventory](../../tests/conformance/async-callables.md) and
[runnable example](../../examples/async-callables/README.md). No new runtime
dependency or package-level mutable state is required.
