# Open specification questions

This register lists only questions that are still open. Decided questions were
removed; the language specification is authoritative for every rule it locks,
and the git history of this file keeps the earlier entries. Resolve an open
question through spec §53 before treating its rule as locked.

Each open question names the stage that needs it in its "Needed before" column.
Unrelated work can proceed. A new gap blocks only the feature it affects.


| ID | Question / missing detail | Reference | Needed before |
| --- | --- | --- | --- |
| Q02 | Remaining grammar for strings and full `go` expressions. Map borrowed entry APIs are listed under Q05. The rest of the expression, collection, closure, and map grammar is locked in §5.10, §7.5–7.8, §12.6, §13.3, and §16. | §5.6, §7.5–7.8, §8.4, §41.1 | Expression parser/lowering |
| Q05 | Complete predeclared API inventory, borrowed map-entry APIs, and remaining type-layout validity rules. Package discovery, package initialization order, the standard-library signatures, and the entry point are locked in §3.19–§3.21 and §37. Functions and types support forward references, and method conflicts are defined (§7.8). Registry/solver remain out of MVP. | §3, §5.7, §7.8, §37, §42, §45 | Resolution/package checking and first native example |
| Q10 | The maintainer accepted the opt-in lexical scope design direction in `docs/proposals/scoped-tasks.md`. Exact syntax and semantics remain unresolved until a specification and conformance update locks them; the feature is not implemented. Ordinary tasks keep their locked detach and process-exit behavior. The locked task-borrowing rules already reject task-local spawned borrows, so Q10 is not required for current MVP safety. | §16.4, §17.8, §18.4–18.11 | Task borrowing and exit-cleanup guarantees |
| Q14 | HTTP/1.1 and request-target URL contracts are accepted and locked in §37.6. The implementation uses owned protocol state and shared socket slices; type-aware indirect-call provenance preserves those loans. Wall-clock Date support is included. JSON codecs are locked in §37.7 and implemented in issue #106; the REST example uses them and stdin-triggered shutdown. Direct function registration, OS signal shutdown, and simpler server/response error handling are now the active follow-up scope in [the API proposal](proposals/http-functions-and-signals.md). Their revised contracts and implementation remain pending. | §11.7, §16.4, §18.4, §20.2, §22.2, §37.6 | Direct handlers and OS signal integration |
| Q13 | Lexical gaps found while implementing M2: (a) §3.7 says horizontal whitespace is insignificant but never lists the characters; the lexer accepts only space, tab, and CR and rejects form feed, vertical tab, NBSP, other Unicode spaces, and a leading BOM as unexpected characters. (b) §7.6 excludes `++`/`--` and requires lexing to distinguish them from adjacent signs; the lexer rejects every adjacent `++`/`--`, so `a--b` and `x - -y` written as `x--y` are errors, and a space or parentheses is required. (c) Following §3.7's definition of newline as LF, a lone CR inside a double-quoted string or rune literal is accepted as content, not rejected as a physical newline. All three are conservative or follow the spec text literally; confirm or revise through §53. | §3.7–3.10, §7.6 | Lexer changes only; not blocking M3 |

## Interpretation notes

Examples such as §24's Move `User`, §48's undeclared resource types, and §20's
conceptual dereference syntax must be read under §0's normative-rule precedence.
They do not override derived Copy classification or introduce raw pointer syntax.

Internal decisions do not need new language syntax: LLVM version/bindings, host
target and ABI, pass ordering, string representation, allocator, scheduler, and
test-harness implementation. Record those with rationale as implementation needs
arise. No permanent internal choice has been made by listing it here.
