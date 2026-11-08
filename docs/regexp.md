# Bounded regular expressions

`std/regexp` searches UTF-8 text with an ordered Thompson NFA. It has no external
provider or runtime dependency. Errors and resource limits are explicit; the
full syntax and contracts are in [specification §37.23](../spec/language-spec.md#3723-bounded-regular-expressions--locked).

```ore
package main

import "std/regexp"

func run() error {
    let text = "name=é count=42"
    let pattern = regexp.Compile("([a-z]+)=([^ ]+)")?
    let first, present, err = pattern.Find(text)
    if err != nil { return err }
    if present {
        println(text[first.Start:first.End])
        let key = first.Group(1)
        println(text[key.Start:key.End])
    }
    let changed = pattern.ReplaceAll(text, "${1}: $2 ($$)")?
    println(changed)
    let matches = pattern.FindAll(text, 10)?
    println(matches.len())
    return nil
}

func main() {
    let err = run()
    if err != nil { panic("regex example failed") }
}
```

`Compile` returns an error for a bad pattern. For a pattern written in the
program itself, `MustCompile` returns the pattern directly and panics if it is
invalid, because that is a mistake in the program:

```text
let digits = regexp.MustCompile("[0-9]+")
```

`Matches(text)` answers whether any substring matches. Add `^` and `$` for a
full-text match. `Find` returns a match, presence, and error. `FindAll(text,
limit)` returns at most `limit` matches, and never more than `MaxMatches`;
reaching that count is a successful stop. A limit below 1 is an error.

`Start` and `End` are half-open byte offsets into the original text. Matching
consumes Unicode scalar values, so offsets stay on valid slicing boundaries.
`Group(n)` numbers groups the same way as replacements: `Group(0)` is the whole
match and `Group(1)` is the first parenthesized group, like `$0` and `$1`.
`Groups()` is how many parenthesized groups there are; asking for a number past
it panics. An unmatched optional group has offsets `-1`; check
`group.Matched()` before slicing. An empty participating capture instead has
equal nonnegative offsets. Result arrays own offsets and retain no text view.

Supported patterns include literals, `.`, absolute `^`/`$`, ordered alternatives,
capturing/noncapturing groups, scalar classes/ranges, greedy/lazy repetitions,
and counted repetitions through 1000. A Zore string spells a digit pattern
`"\\d+"`, since its backslash is escaped. `\d`, `\w`, `\s` and word
boundaries use ASCII definitions on every host; non-ASCII letters/digits do not
belong to those shorthands. Their uppercase forms are complements. Literal
runes, scalar ranges, escaped controls, `\xHH` and `\u{H...}` are supported.
Matching is case sensitive, with no normalization or grapheme segmentation.
Backreferences, lookaround, named groups, flags, Unicode properties and POSIX
class expressions are rejected.

The earliest start wins, then alternatives and greedy/lazy choices follow
their written order. `a|ab` finds `a` in `ab`. Empty matches advance one scalar;
an empty match adjacent to a preceding nonempty match is omitted. The empty
pattern finds byte offsets 0 and 2 in `é`; `a*` finds one match in `aa`.

`ReplaceAll` expands `$0` for the whole match, `$1` through `$16` for captures,
`${n}` for a delimited reference, and `$$` for a literal dollar. References
consume all following digits; `${1}2` means group 1 followed by `2`. Unmatched
groups expand to empty text. Invalid references fail even without a match.
Backslashes are literal in replacements. Use `ReplaceAllLiteral` for user text
whose dollar signs must remain unchanged. Neither method returns partial output
on failure. Replacement has no match count limit; its output is limited to
`MaxOutputBytes`.

## Ownership and execution

`Regexp` is a Move value. Drop releases it; no close call is needed. `Share()`
returns an independent owned handle to the same immutable program, without
recompilation. Scoped tasks can borrow a pattern. Independent tasks receive an
owned original or shared handle:

```ore
async func checkRecord(pattern own regexp.Regexp) (bool, error) {
    return pattern.Matches("count=42")
}

func concurrent() error {
    let pattern = regexp.Compile("^count=[0-9]+$")?
    let shared = pattern.Share()
    let child = go checkRecord(shared)
    let matched, err = child.wait()
    if err != nil { return err }
    println(matched)
    return nil
}
```

Compilation and search/replacement use the existing helper pool. Plain callers
wait with worker compensation; async callers suspend their task. Share/drop
only retain/release handles. Searches hold no global lock. Running helper jobs
have no cancellation API in this increment.

## Bounds and validation

Patterns have a 4096-byte limit, 16 captures, nesting depth 64, and at most 4096
compiled instructions. Input/templates and replacement output are each limited
to 1 MiB; `FindAll` returns at most 1024 matches. Public constants expose
the byte, capture, state, match and work limits.

A search or replacement may spend `BaseWork` (16,777,216) units plus
`WorkPerByte` (64) units for each byte of its text and replacement template.
That is enough for ordinary patterns, such as a list of a dozen words, to scan
the largest accepted input, while a pattern that keeps hundreds of
alternatives alive at every character fails instead of running for a long
time. Compilation has `BaseWork` units and charges expansion visits, including
nested empty repetitions.
The VM uses explicit epsilon stacks and at most one ordered thread per
instruction, with no recursive input backtracking. Its working storage depends
on states and captures, not text length; class comparisons, template scanning
and replacement-part expansion also consume work, including empty captures.
Exceeded bounds return errors, distinct from no match. Limits cannot be raised
yet. For larger documents choose application-defined record boundaries;
arbitrary chunks change anchor and cross-boundary matching behavior.

See [conformance](../tests/conformance/regexp.md). CI also runs
`python tests/hardening/regexp_oracle.py`, which compares every match of each
search, including anchored patterns and empty matches, with Python's
independent engine for the shared syntax, alongside native API,
Unicode, adversarial, cleanup and ownership tests.
