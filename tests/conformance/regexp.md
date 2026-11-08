# Bounded regular expressions

The locked contract is specification §37.23. Coverage is registered in runtime
parser/VM tests, `tests/codegen/regexp.rs` under the native target, and
`tests/packages/regexp.rs` under the package target.

| Case | Required result |
| --- | --- |
| Literal, alternation, classes, greedy/lazy and counted quantifiers | Leftmost, ordered results with byte offsets |
| Optional and repeated captures | Absent spans versus empty spans; last participating capture |
| UTF-8 runes, non-ASCII ranges and combining text | Scalar matching, original byte boundaries, no normalization |
| ASCII shorthands and word boundaries | Same definitions on every host |
| Invalid syntax, UTF-8, escapes, flags, backreferences and lookaround | Explicit error with operation prefix; syntax offset |
| Empty patterns and nullable repetitions | Finite, scalar-boundary iteration; adjacent empty suppression |
| Numbered and delimited replacement, escaped dollars, literal replacement | Exact bytes, absent group expansion, invalid references fail even with no match |
| Pattern/depth/capture/state/input/output/work limits | Bounded failure, empty result on error |
| Adversarial nested alternation/repetition | No recursive backtracking; deterministic work-budget failure |
| Share/move/drop, panic/error cleanup and scoped concurrent use | Immutable program remains alive for valid handles and is released exactly once |
| Private fields, Move reuse and async wrappers | Ordinary compiler acceptance/rejection and helper suspension |

This increment excludes PCRE completeness, Unicode properties/flags, streaming
matching, parser combinators and cancellation of a running helper operation.
