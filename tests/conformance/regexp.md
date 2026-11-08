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
| An eight-word alternation over about 1 MB of text; replacing 5000 newlines | Succeeds: the work budget grows with the input and replacement has no match count limit |
| `FindAll` with a limit above `MaxMatches`, or below 1 | Stops at `MaxMatches`; an error |
| `Group(0)`, `Group(n)`, `Groups()`, and a group number past the pattern | The whole match, group `n` like `$n`, the group count; a panic |
| `MustCompile` of a valid and an invalid pattern | The pattern; a panic naming the `Compile` error |
| `Share` of a closed handle | A closed handle whose searches fail |
| Every match of random patterns, anchored and nullable ones included | The same spans as Python's engine with the same iteration rule |
| Large replacement templates with many absent capture references | Expansion consumes the same work budget even when no bytes are appended |
| Adversarial nested alternation/repetition | No recursive backtracking; deterministic work-budget failure |
| Share/move/drop, panic/error cleanup and scoped concurrent use | Immutable program remains alive for valid handles and is released exactly once |
| Private fields, Move reuse and async wrappers | Ordinary compiler acceptance/rejection and helper suspension |

This increment excludes PCRE completeness, Unicode properties/flags, streaming
matching, parser combinators and cancellation of a running helper operation.
