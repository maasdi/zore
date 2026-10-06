# Control-flow conformance cases

Authority: spec §5.8–5.10 and §7.7. These are pending parser, scope, typing,
ownership, CFG, and runtime cases, not executable tests or passing coverage.

| Scenario | Expected result |
| --- | --- |
| Empty or standalone statement block | Valid block scope |
| Block used as an initializer expression | Reject value-producing block |
| Nested `let count = count + 1` with an outer count | Initializer resolves outer binding; new binding visible afterward |
| Self-reference with no enclosing binding | Reject unresolved name |
| Multiple binding initializer | All new names become visible after initializer |
| Function body redeclares parameter/receiver name | Reject same-scope duplicate |
| Further nested block shadows ordinary parameter | Allowed; distinct semantic IDs |
| `if flag { work() }` and `if (flag) { work() }` | Bool condition, required block |
| If condition has integer or string type | Reject non-bool condition |
| If body without braces | Reject |
| If header contains an initializer clause | Reject unsupported form |
| `if a { first() } else if b { second() } else { third() }` | Conditions tested in order; only selected body executes |
| Newline between `}` and `else` | Reject separated else after semicolon insertion |
| Read branch-local binding after conditional | Reject out-of-scope name |
| Move resource in one branch then unconditionally read it | Reject potentially moved value |
| `for { work() }` with no exit | Infinite form |
| `for flag { work() }` | Test bool condition before each iteration |
| Conditional loop condition false initially | Body not executed |
| Counting loop | Init once; condition, body, update in order |
| Counting-loop continue | Exit iteration scope, run update, recheck condition |
| Conditional-loop continue | Exit iteration scope, recheck condition |
| Infinite-loop continue | Exit iteration scope, begin next body execution |
| Counting-loop break | Exit loop; do not run update |
| Nested loop break/continue | Target nearest loop only |
| Break/continue outside loop or targeting an enclosing function's loop | Reject |
| Labelled break/continue, `range`, goto | Reject unsupported forms |
| Counting header with missing clause or declaration as update | Reject unsupported header |
| Read counting initializer binding after loop | Reject out-of-scope name |
| Read body-local binding in counting update | Reject out-of-scope name |
| Return/break/continue exits owned-resource scopes | Perform required cleanup once on actual path |
| Continue in counting loop | Retain initializer-scope resources; clean required body resources |
| Loop carries moved/borrowed state to next iteration | Validate backedge state, reject invalid subsequent use |
| No-result function falls through or uses bare return | Valid |
| Result function uses bare return | Reject |
| No-result function returns a value | Reject |
| `return 1, 2` in `(int, int)` function | Match result count/types |
| Incorrect return result count/type | Reject |
| Named return parameters | Reject |
| Result function has a reachable path to body end | Reject missing return |
| Both conditional branches return results | No fallthrough if every reachable branch returns |
| Result function ends in provably infinite loop with no reachable exit | Valid non-completing path |
| Loop can break to function end | Does not establish return completeness |
| Return operands have effects | Evaluate left to right; propagate early errors and clean temporaries |
| Return an owned value | Transfer it; do not destroy returned ownership locally |
| Return borrowed local beyond its valid lifetime | Reject |

Use event logs and drop counters for paths/loops, and bounded execution for
runtime cases. Infinite-loop completeness cases should be checked statically,
not executed without a bound. Do not assume a non-returning panic contract or
unspecified error typing ahead of their separate decisions. Result forwarding
cases are covered in `functions-structs.md` under §7.8.

## Collection loops (§5.10)

Executable counterparts: `tests/parser/parser.rs`, `tests/typecheck/check.rs`,
`tests/ownership/ownership.rs`, and `tests/codegen/native.rs`;
`examples/collections` runs natively.

| Scenario | Expected result |
| --- | --- |
| `for item in c` over a fixed array, slice, or `Array<T>` | Visits elements in index order |
| `for i, item in c` | `i` is the `int` index |
| `for key, value in m` | Visits each entry once, in a stable unspecified order |
| `for value in m` | Reject; a map loop names both parts |
| `_` for either name | Valid; that part is not bound |
| Loop over a string, number, or constant | Reject |
| Same name for both parts | Reject duplicate declaration |
| Loop names used after the loop | Reject out-of-scope name |
| Assign to the item | Reject; the item is a shared borrow |
| Move the item or pass it to `mut`/`own` | Reject |
| Copy a Copy item, or pass it to a shared parameter | Valid |
| Collection whose elements hold `mut []T` | Reject; use a counting loop |
| Assign, push, pop, remove, or mutably borrow the collection in the body | Reject, even when the item is unused |
| Read the collection in the body | Valid |
| View copied from an item | Keeps the element's provenance, not the loop's borrow |
| Collection is a temporary (call result, literal, slice expression) | Held for the loop and destroyed when it ends |
| `break`, `return`, or a panic in the body | The held collection is destroyed once |
| `continue` | Proceeds to the next element |
