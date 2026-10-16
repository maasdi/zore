# Distribution planning validation

This records evidence for the [#139 proposal](package-distribution.md). It does
not claim that version selectors, release catalogs, release metadata validation,
publishing, yanks or registry discovery have been implemented.

## Independent-repository experiment

Executed on Linux against the merged #138 implementation
(`4e951bb4b54792023f8f07bf89d4866ef3a664e8`), using an isolated temporary Git
source/cache tree and the normal host native toolchain.

The experiment copied `shared`, `greeting` and `audit` from
[`examples/local-dependencies`](../../examples/local-dependencies) into three
separate Git repositories. Each library retained its own project manifest and
received an annotated `<name>/v0.1.0` tag. The greeting/audit manifests pinned the
shared repository by full commit ID. The application pinned greeting and audit
by full commit ID; its added test passed a `shared.Person` returned by greeting
to audit, checking that the transitive nominal type remained shared.

Tags were manually resolved to commits with Git. The compiler never received a
version selector or resolved a tag. The accepted manifest `version` metadata
remained unchecked, exactly as in the current implementation.

| Step | Observed result |
| --- | --- |
| Install the separate-library diamond | Success; one shared project record in the entry lock |
| Check with `--locked --offline` and nonexistent C/Rust compiler overrides | Success without a native toolchain |
| Run with `--locked --offline` | `Hello, Ada!` followed by `person:Ada` |
| Test the application with `--locked --offline` | One shared-type test passed |
| Commit shared 0.2.0 and move its 0.1.0 tag to that commit; repeat locked install | Success using the original pinned commit; lock bytes unchanged |
| Add a conflicting direct shared pin while transitive dependencies retain the original pin | Install rejected the conflicting project; lock bytes unchanged |
| Restore the original manifest and make all three repository locations unavailable | Locked offline install/check/run/test all succeeded from the validated cache; output/type identity and lock bytes unchanged |

Ten compiler commands completed: nine successes and one expected conflict
rejection. The original shared source commit was
`0ceb6c95f893def0244253162b82f374a497ee6a`; the changed source commit was
`7948c7433207ad41cd26f27c68ede6aa236b143e`. These are disposable local fixture
commits, not publicly hosted releases or dependencies for readers to install.

This demonstrates why immutable source selection should remain authoritative
over mutable release labels. It does not show that the compiler recognizes or
rejects moved version releases: that behavior needs the proposed catalog/lock
implementation. There was no real remote publication or authentication test.

## Existing conformance to retain

The following commands exercise the current Git and native package foundation:

```sh
cargo test --locked --test git_dependencies
cargo test --locked --test native_adapters
cargo test --locked --test packages
```

Relevant existing cases include:

| Concern | Executable evidence |
| --- | --- |
| Offline graph, shared identity and one initialization | `git_dependencies::populated_offline_diamond_checks_builds_runs_and_tests_with_shared_identity` |
| Conflicting source pins and names | `git_dependencies::incompatible_pins_and_project_name_mismatches_are_diagnosed` |
| Immutable tree/cache bytes and objects | `git_dependencies::corrupt_cache_bytes_metadata_and_extra_paths_are_rejected`, `corrupt_git_objects_are_rejected_even_when_checkout_is_unchanged` |
| Explicit resolution/lock drift and safe concurrent cache publication | `git_dependencies::lock_uses_exact_changed_pin_and_builds_never_rewrite_locks`, `incomplete_fetches_are_ignored_and_concurrent_cache_publication_is_safe` |
| Credential redaction | `git_dependencies::credential_helper_or_server_stderr_is_never_in_diagnostics` |
| Separately maintained native package through a Git diamond | `native_adapters::pinned_git_native_provider_is_shared_by_a_transitive_offline_diamond` |
| Native ownership across waits, errors and panic | `native_adapters::owned_native_resources_survive_async_waits_and_clean_up_on_error_and_panic`, `scoped_unobserved_native_results_are_destroyed_on_normal_and_parent_panic_exits` |
| Approved native bytes and root confinement | `native_adapters::modified_native_sources_cannot_be_approved_by_regenerating_the_lock`, `native_source_symlinks_cannot_escape_the_declaring_project` |

These cases cover existing behavior on their supported hosts; the symlink case
is Unix-only. The #138 final-head CI passed all 1,000 Rust tests on each Unix host
and 981 on Windows, plus the ordinary formatting/lint/build/doc checks. This is
baseline evidence, not a test result for new versioned-distribution features.

## Required future evidence

Each implementation increment must add and run its own conformance cases before
being marked complete. In particular, the existing foundation does not validate:

- Semantic-version/metadata parsing, compiler/platform compatibility decisions,
  canonical artifact SHA-256 vectors or release-content validation.
- Exact-version catalog resolution, requested-versus-resolved provenance,
  catalog/source digest mismatch and immutable record reassignment detection.
- New lock-format parsing, compatibility, offline catalog lookup and preservation
  of previous release identities during explicit lock regeneration.
- Availability/deprecation policy for new selections versus existing locks,
  stale offline advisories and retention after remote ref changes.
- Unauthorized publication, atomic source/catalog publication, unsupported atomic
  servers, concurrent writers, interruption/retry or partially uploaded releases.
- Cross-repository discovery or any hosted registry/authentication service.

The proposed release system must repeat the independent-library application
proof on Linux, macOS and Windows using its actual version selectors and
publishing workflow. A successful manual-tag/full-pin experiment cannot replace
that acceptance test.
