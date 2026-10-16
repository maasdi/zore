# Open specification questions

This register lists only questions that are still open. Decided questions were
removed; the language specification is authoritative for every rule it locks,
and the git history of this file keeps the earlier entries. Resolve an open
question through spec §53 before treating its rule as locked.

Each open question names the stage that needs it in its "Needed before" column.
Unrelated work can proceed. A new gap blocks only the feature it affects.


| ID | Question / missing detail | Reference | Needed before |
| --- | --- | --- | --- |
| Q02 | Remaining grammar for strings and full `go` expressions. Conditional borrowed map-entry bindings are locked in §13.4. The rest of the expression, collection, closure, and map grammar is locked in §5.10, §7.5–7.8, §12.6, §13.3, and §16. | §5.6, §7.5–7.8, §8.4, §41.1 | Expression parser/lowering |
| Q05 | Remaining predeclared API inventory and type-layout validity rules. Conditional borrowed map-entry bindings are locked in §13.4; mutable view-containing entries remain excluded pending writeback provenance semantics. Package discovery, package initialization order, the standard-library signatures, and the entry point are locked in §3.19–§3.21 and §37. Functions and types support forward references, and method conflicts are defined (§7.8). Local and pinned Git dependencies, lockfiles and offline workflows are locked under §§3.4–3.4.2 and 3.20; curated native adapters are locked in §3.4.3; registry/solver remain outside this increment. | §3, §5.7, §7.8, §37, §42, §45 | Resolution/package checking and first native example |
| Q13 | Lexical gaps found while implementing M2: (a) §3.7 says horizontal whitespace is insignificant but never lists the characters; the lexer accepts only space, tab, and CR and rejects form feed, vertical tab, NBSP, other Unicode spaces, and a leading BOM as unexpected characters. (b) §7.6 excludes `++`/`--` and requires lexing to distinguish them from adjacent signs; the lexer rejects every adjacent `++`/`--`, so `a--b` and `x - -y` written as `x--y` are errors, and a space or parentheses is required. (c) Following §3.7's definition of newline as LF, a lone CR inside a double-quoted string or rune literal is accepted as content, not rejected as a physical newline. All three are conservative or follow the spec text literally; confirm or revise through §53. | §3.7–3.10, §7.6 | Lexer changes only; not blocking M3 |
| Q14 | Versioned package distribution is proposed in [the #139 planning record](design/package-distribution.md): tagged releases, compatible minimum-version graph selection, minimal release metadata, lock provenance and update discovery. These contracts must be specified and locked before implementation; current manifests still support only local paths/full Git pins and unchecked version metadata. | §§3.4–3.4.3, 40, 53 | Versioned release implementation |

## Interpretation notes

Examples such as §24's Move `User`, §48's undeclared resource types, and §20's
conceptual dereference syntax must be read under §0's normative-rule precedence.
They do not override derived Copy classification or introduce raw pointer syntax.

Internal decisions do not need new language syntax: LLVM version/bindings, host
target and ABI, pass ordering, string representation, allocator, scheduler, and
test-harness implementation. Record those with rationale as implementation needs
arise. No permanent internal choice has been made by listing it here.
