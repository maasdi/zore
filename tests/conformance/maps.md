# Map conformance cases

Authority: spec §13.3, with §5.6, §7.2, §7.5–7.8, §11.7, §12.3, §15,
§31.2, and §41.4. These are pending parser, typing, ownership, and runtime
expectations, not executable tests or passing coverage.

## Construction and keys

| Scenario | Expected result |
| --- | --- |
| `map[string]int{"Ada": 10, "Lin": 20}` | Owned Move map with two entries |
| `map[string]int{}` | Valid empty map, not nil |
| Bool, integer/alias, rune, or string key type | Accepted |
| Float, error, Task, channel, array, slice, map, struct, or closure key type | Rejected, even if Copy or equality-comparable |
| Two equal strings with different backing buffers used as keys | Match by content; equal hashes required |
| Key/value expression has incompatible already-typed value | Reject; no implicit numeric conversion |
| Contextual integer literal fitting the key/value type | Accepted under ordinary literal rules |
| Trailing comma, including before newline after final entry | Accepted under semicolon-insertion rules |
| Final entry followed by newline without necessary comma | Reject inserted semicolon in entry list |
| Omitted K/V, spreads, or multiple-result expression as one key/value | Reject |
| Ambiguous unparenthesized literal in condition | Require parentheses as for other composite literals |
| Duplicate equal constant keys | Compile-time rejection |
| Duplicate keys only discovered at runtime | Evaluate that entry's key and value, then panic; no later entry evaluated |
| Runtime duplicate value is Move | Destroy uninserted value and previously initialized entries exactly once |
| Third initializer propagates `?` or panics | Destroy initialized entries and applicable temporaries; do not evaluate remaining entries |
| Key/value expressions have observable effects | Key then value for each entry, entries left to right |

## Lookup and result shape

| Scenario | Expected result |
| --- | --- |
| `let found, value = m[key]` with eligible Copy V and present key | true plus copied value; entry retained |
| Same lookup with missing key | false plus V's zero value |
| Present key stores zero value | true plus zero, distinct from absent false plus zero |
| `let value = m[key]` | Reject result-count mismatch; no one-result lookup |
| `let _, value = m[key]` or `let found, _ = m[key]` | Valid explicit discards |
| `_, _ = m[key]` | Explicitly discard both results |
| `return m[key]` from function declaring matching `(bool, V)` | Forward both results once under §7.8 |
| Pass m[key] directly as a single argument expecting V | Reject; no argument result splicing |
| Move V lookup, including passing indexed expression to a shared V parameter | Reject; no Move copy, implicit clone, or entry-reference result |
| Copy V contains a mutable view requiring an exclusive reborrow | Reject copy through shared map access |
| Copy V contains only eligible shared views | Copy retains external backing provenance; no lifetime extension |
| Base and key have observable effects | Base then key, once each |
| `m[key]?` | Reject; subscript is not one of §15.2's permitted propagation operands |
| Bare `m[key]` statement | Reject as a bare value expression under §7.8 |

## Error-valued maps

| Scenario | Expected result |
| --- | --- |
| Lookup/removal from `map[string]error` | Result shape `(bool, error)`, error last |
| Present entry containing nil error | true, nil |
| Missing entry | false, nil |
| Named returned error is unused on a reachable path | Reject under §15.6, even for nil |
| Explicitly discard returned error with `_` | Allowed |
| `m.remove(key)?` inside function with trailing error result | Ordinary propagation consumes error and yields presence bool on success |
| Same propagation in function without trailing error | Reject |
| Use error type as map key rather than value | Reject key type |

## Assignment and replacement

| Scenario | Expected result |
| --- | --- |
| Mutable map, missing key, `m[key] = value` | Insert; transfer Move V or copy eligible Copy V |
| Mutable map, existing key | Drop prior owned value before storing replacement |
| Let-owned map or map reached through shared borrow | Reject mutation |
| Map reached through mut parameter | Mutation permitted under ordinary exclusivity |
| `m[key] += value` | Reject compound map assignment |
| `m[key].Field = value` or borrowing entry as a general place | Reject; map target is not an addressable entry place |
| Target base/key then RHS have observable effects | Evaluate in that order, once each, before modifying map |
| RHS propagates `?` before replacement begins | Prior entry retained; clean owned temporaries; earlier effects not rolled back |
| Previous value's destructor panics during replacement | Old entry already detached; no second drop; pending new value and remaining entries cleaned under unwind rules |
| Containing custom destructor runs after replacement failure | Receives valid map with replaced key absent, not uninitialized bucket storage |
| Multiple targets in same map cannot be proven independent | Reject; different key expressions alone are insufficient |
| Key or RHS suspends | Retained map/key/value state must satisfy ordinary borrow and async rules |

## Removal and cleanup

| Scenario | Expected result |
| --- | --- |
| `let found, value = m.remove(key)` on mutable map, key present | Detach entry and return true plus V; do not destroy returned V |
| Remove Move resource then map goes out of scope | Map does not drop removed value; new owner drops it exactly once |
| Removal from empty map | false plus harmless zero V; map remains valid |
| Remove same key twice | First transfers stored value; second yields zero and false |
| Removal of Copy value | Same presence-first shape and entry removal |
| Removal through let binding/shared borrow | Reject mut receiver requirement |
| Removed value contains external borrowed views | Provenance retained; moving out does not extend backing lifetime |
| Remove value from map field inside custom-drop struct | Valid collection operation; containing map remains initialized |
| Attempt indexed partial Move extraction instead of remove | Reject under §31.2; remove does not relax indexed moves |
| Drop nonempty map | Every remaining initialized value cleaned exactly once; inter-entry order unspecified |
| Zero resource returned on removal miss is discarded | Ordinary harmless empty-state cleanup; no fabricated resource release |
| Completed removal followed by `?`/panic | No rollback or reinsertion; returned owned value cleaned at its current owner |

Use instrumented resources for exact-once accounting and bounded runtime tests
when the compiler exists. Borrowed in-place entry APIs, iteration, and map
length/capacity operations remain Q02/Q05 decisions; do not invent their syntax.
