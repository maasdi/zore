# Generated ownership and resource hardening

Seeded combinations on top of the #114 matrix in [MVP conformance](mvp.md); this is not a soundness proof.

| Test | Generated cases |
| --- | --- |
| `seeded_cloned_view_move_chains_preserve_acceptance_and_conflict_rejection` | Move/clone/drop chains through arrays and a struct owning a view and an array; reads pass, backing mutation before the last view read is rejected |
| `seeded_partial_construction_propagation_matches_plain_and_async` | Success and error initializations; destruction order of initialized fields and discarded results |
| `seeded_scoped_captures_match_plain_async_cleanup_after_observed_join` | Scoped task captures across return, `?`, parent panic and destructor panic; child reads and destruction match plain and async |
| `seeded_json_nested_documents_reject_truncation_and_preserve_owned_values` | Nested documents with owned clones and three invalid mutations each |
| `seeded_http_fragment_boundaries_keep_framing_and_rejection_oracles` | Fragment widths over header status and body-read results, including chunk sizes, lengths and limits; substitutes parser reads, not sockets |
| `seeded_cancellation_trees_release_timers_only_after_quiescence` (Linux, macOS) | Deadline trees cancelled at leaf and root; zero tasks and timers afterwards |
| `seeded_tcp_partial_bodies_cancel_and_quiesce` (Linux, macOS) | Loopback incomplete bodies at several write widths, cancelled mid-read, then shutdown |

Programs run with `ZORE_CHECK_LEAKS=1` and call `waitIdle(0)` first, so exit leak checks run after tasks stop. Children run under time and output limits.

## Replay

```sh
ZORE_HARDENING_SEED=159 cargo test --locked --test native hardening:: -- --test-threads=1
cargo test --locked --test native bounded_process:: -- --test-threads=1
```

The default seed is `158`. A failure names its seed and a `hardening[class/assertion]` tag, and saves `original.ore`, `reduced.ore` and `failure.txt` under `ZORE_HARDENING_FAILURES` (default `target/hardening-failures`). Reduction tries up to four deletions that keep the same tag; time and output limit failures are not reduced.

## CI

[`hardening.yml`](../../.github/workflows/hardening.yml) runs weekly, manually, or on a PR labeled `run-hardening`: four seeds at `-O1` (`ZORE_HARDENING_OPTIMIZE`), plus a Linux x86-64 AddressSanitizer job (`ZORE_HARDENING_ASAN=1`). It is not a coverage-guided fuzzer.
