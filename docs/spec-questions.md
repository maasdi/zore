# Open specification questions

This register lists only questions that are still open. Decided questions were
removed; the language specification is authoritative for every rule it locks,
and the git history of this file keeps the earlier entries. Resolve an open
question through spec §53 before treating its rule as locked.

Each open question names the stage that needs it in its "Needed before" column.
Unrelated work can proceed. A new gap blocks only the feature it affects.


| ID | Question / missing detail | Reference | Needed before |
| --- | --- | --- | --- |
| Q02 | Remaining grammar for strings and full `go` expressions; collection loops are resolved in §5.10 (Q02h); closures and function types are resolved in Q02g, with owning and call-once closures in Q02i. Map borrowed entry APIs remain Q05. Arrays/indexing/slicing are resolved in Q02e and map construction/lookup/assignment/removal in Q02f; assignment, operators, calls, and struct construction are also locked. | §5.6, §7.5–7.8, §8.4, §41.1 | Expression parser/lowering |
| Q05 | Local package discovery/import mapping (resolved in Q24), complete predeclared API inventory, initial standard-library signatures beyond `println` (strings and strconv resolved in Q24; float text resolved in Q43), package variable initialization order (resolved for package-level `let` in Q41), and remaining type-layout validity rules. The entry-point contract and `println` are resolved in Q05a. Functions/types support forward references and method conflicts are defined (§7.8). Registry/solver remain out of MVP. | §3, §5.7, §7.8, §37, §42, §45 | Resolution/package checking and first native example |
| Q10 | The maintainer accepted the opt-in lexical scope design direction in `docs/proposals/scoped-tasks.md`. Exact syntax and semantics remain unresolved until a specification and conformance update locks them; the feature is not implemented. Ordinary tasks retain their locked detach and process-exit behavior. Q09b already rejects task-local spawned borrows, so Q10 is not required for current MVP safety. | §16.4, §17.8, §18.4–18.11 | Task borrowing and exit-cleanup guarantees |
| Q13 | Lexical gaps found while implementing M2: (a) §3.7 says horizontal whitespace is insignificant but never lists the characters; the lexer accepts only space, tab, and CR and rejects form feed, vertical tab, NBSP, other Unicode spaces, and a leading BOM as unexpected characters. (b) §7.6 excludes `++`/`--` and requires lexing to distinguish them from adjacent signs; the lexer rejects every adjacent `++`/`--`, so `a--b` and `x - -y` written as `x--y` are errors, and a space or parentheses is required. (c) Following §3.7's definition of newline as LF, a lone CR inside a double-quoted string or rune literal is accepted as content, not rejected as a physical newline. All three are conservative or follow the spec text literally; confirm or revise through §53. | §3.7–3.10, §7.6 | Lexer changes only; not blocking M3 |

## Interpretation notes

Examples such as §24's Move `User`, §48's undeclared resource types, and §20's
conceptual dereference syntax must be read under §0's normative-rule precedence.
They do not override derived Copy classification or introduce raw pointer syntax.

Internal decisions do not need new language syntax: LLVM version/bindings, host
target and ABI, pass ordering, string representation, allocator, scheduler, and
test-harness implementation. Record those with rationale as implementation needs
arise. No permanent internal choice has been made by listing it here.
