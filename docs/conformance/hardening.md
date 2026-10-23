# Generated ownership and resource hardening

This extends the executable #114 matrix in [MVP conformance](mvp.md), the seeded
lexer/parser generators, and the self-hosted differential tests. It adds evidence
for combinations of existing rules; it does not establish exhaustive soundness
or introduce language semantics.

| Existing executable evidence | Additional generated assertion in `tests/codegen/hardening.rs` |
| --- | --- |
| `nested_returned_and_cloned_views_keep_their_backing_through_moves_and_calls`; paired ownership provenance rejections | `seeded_cloned_view_move_chains_preserve_acceptance_and_conflict_rejection`: bounded move/clone/drop chains through flat/nested arrays and a struct owning both a view and an array. The independent oracle permits reads and rejects backing mutation before the final view read. |
| Partial move/reinitialization and error cleanup regressions | `seeded_partial_construction_propagation_matches_plain_and_async`: eight seeded success/error initializations; the Rust oracle independently derives reverse destruction of initialized fields and discarded error-path results. |
| Named scoped-task cleanup/Pending regressions | `seeded_scoped_captures_match_plain_async_cleanup_after_observed_join`: cloned nested view captures, a moved/reinitialized resource field, normal return, `?`, parent panic and destructor panic. A closed gate releases the child only after observed plain blocking or async Pending at the join. Exact child reads/destruction traces match plain and async execution. |
| Fixed JSON malformed syntax, value clone and partial-decode tests | `seeded_json_nested_documents_reject_truncation_and_preserve_owned_values`: twelve nested documents with known integer/string leaves, owned clones after original destruction, and three independently invalid mutations per document (truncation, trailing token, trailing comma). |
| Named HTTP framing/limits tests | `seeded_http_fragment_boundaries_keep_framing_and_rejection_oracles`: eight seeded fragment widths over seven fixed framing/status/body oracles, including chunk extensions, ambiguous framing, signed lengths, body limits and unsupported HTTP version. This fixture substitutes reads in the real parser; it is supplementary parser coverage, not evidence of socket fragmentation or cancellation. |
| Fixed context cancellation/shutdown cases | `seeded_cancellation_trees_release_timers_only_after_quiescence`: eight seeded bounded deadline trees; observe actual Pending and exact live timer registrations, cancel leaf/root repeatedly, wait for zero tasks, then assert zero timers and every cancellation reason. |
| Real loopback shutdown cases | `seeded_tcp_partial_bodies_cancel_and_quiesce`: three seeded write widths over an incomplete content-length body. Observe blocked accept/body reads, cancel shutdown, assert the request context and handler completion, join Serve, wait for all tasks and assert zero timers. TCP may coalesce writes; the parser fixture independently controls read boundaries. |

The scope panic cases deliberately drop their ordinary parent task handle. Under
the existing task-panic contract, the parent task reports its panic and cleans
up, while the initial task continues and exits successfully. No exit-status
expectation is inferred from the mere presence of a panic on stderr. Destruction
must print the child read and `10, 20, 30` exactly once before `quiescent`.

All generated native programs enable `ZORE_CHECK_LEAKS=1`, and explicitly call
`waitIdle(0)` before initial-task exit. The observer checks both the scheduler task
registry and the running-task count. Consequently the existing text-buffer,
channel, mutex, native-resource and signal-subscription exit checks execute after
tasks stop; they are not silently skipped because detached work remains. These
counters do not cover every allocator allocation or OS descriptor. Timer assertions
are additional checks, not substitutes for quiescence.

## Reproduction and failure artifacts

The default corpus uses seed `158`. Select any `u64` seed with:

```sh
ZORE_HARDENING_SEED=159 cargo test --locked --test native hardening:: -- --test-threads=1
cargo test --locked --test native bounded_process:: -- --test-threads=1
```

Generation is bounded in shape, operation count, nesting and input length. View
checking runs in an isolated test-binary subprocess (15 seconds). Native tools
have 120-second limits, generated programs 30 seconds, and both captured output
streams have 256 KiB limits. Pipe draining has a separate two-second deadline.
Unix commands own process groups; timeout, output overflow and even successful
parent exit terminate descendants before draining. Windows attaches kill-on-close Job Objects immediately after spawning. There
is a startup race: a child can create descendants or exit before job assignment.
In that race, Windows tree termination is not guaranteed; bounded pipe draining
still returns an error instead of hanging the caller. The successful-parent
Windows self-test asserts bounded return (success or a reported failure), while
Unix requires successful capture after group termination. Its pipe-holding
fixture also self-terminates after ten seconds. Self-tests exercise quiet timeouts, output flooding and a successful
parent leaving a pipe-holding descendant. The helper reaps its direct child;
Unix orphan descendant reaping belongs to the host's process reaper. Descendants
that deliberately escape their group/job are outside these tool fixtures.

Failure output identifies the seed and assertion family. `ZORE_HARDENING_FAILURES`
selects the artifact root (default `target/hardening-failures`). Each failure saves
`original.ore`, `failure.txt` with the observed/expected assertion output, original
operations, `reduced.ore`, reduced operations and the reduction attempt count.
Re-run the same test filter and seed. Reduction attempts at most four operation
deletions, retaining only the same explicit `hardening[class/assertion]` tag;
it does not claim a globally minimal source. Untagged infrastructure failures
and time/output-limit failures skip
reduction. `reduction_preserves_failure_class_and_skips_timeouts` asserts that a
candidate invalidated by deletion cannot replace the original observable failure.

The compiler's LLVM emission still runs in the test process, as in the existing
native harness. The bounded grammar limits its workload, but a compiler hang in
that stage is ultimately bounded by the CI job timeout, not the child timer.

## Continuous and sanitizer evaluation

[Seeded hardening workflow](../../.github/workflows/hardening.yml) runs manually,
weekly, or on a PR explicitly labeled `run-hardening`, separately from the normal
PR loop. Four fixed seeds run generated
programs at clang `-O1`; the ordinary corpus uses `-O0` to avoid costly repeated
native optimizer work. Existing native suites continue using `-O2`.
`ZORE_HARDENING_OPTIMIZE=1` selects `-O1`, and `=2` selects `-O2` locally.

A separate Linux x86-64 job pins nightly Rust 2026-10-01 and enables AddressSanitizer
for runtime unit tests and, via `ZORE_HARDENING_ASAN=1`, both the generated native
objects and their Rust runtime/link wrapper. `ASAN_OPTIONS` enables leak detection
and immediate failure. This evaluates real parser/runtime/native execution rather
than merely checking a sanitizer flag. It is scheduled, not a PR prerequisite.
No new cargo dependency, production runtime flag or public source feature is added.
This bounded seeded job is not a coverage-guided fuzzer. ThreadSanitizer, Miri,
full standard-library instrumentation, non-x86 Linux and macOS/Windows sanitizer
coverage are excluded. Sanitizer and scheduled workflow results remain pending
until those jobs execute; their configuration is not a passing result.

## Actual host evidence

| Host | Evidence on this revision |
| --- | --- |
| Linux x86-64, Rust 1.98.1, clang 19 | Focused executable results recorded in the PR; the normal corpus and subprocess/minimizer self-tests are run locally. |
| macOS | Not run locally. Normal corpus, process-group tests and TCP/timer cases are selected by the existing three-host CI. |
| Windows x86-64 | Not run locally. Ownership/partial construction/scope/JSON/parser cases and Job Object helper tests are selected by existing CI. Timer tree and real TCP cancellation stress are explicitly excluded pending Windows reactor support. |
| Linux AddressSanitizer / extra scheduled seeds | Configured opt-in evaluation; not run locally. |

Markdown requirements and configured CI do not count as executed host evidence.
