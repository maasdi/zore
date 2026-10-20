# Distribution planning validation

This records evidence for the [#139 design](package-distribution.md), including
the implemented #144–#146 increments. The earlier experiment/model below are
historical evidence only. Catalogs, custom source digests and automated
publishing/advisories remain outside the first release path.

## Implemented release workflow coverage

`release_metadata` validates canonical identities, required licensing and optional
metadata. `tagged_dependencies` exercises actual tag resolution, complete request
expansion, minimum selection, version-2 lock provenance and offline consumption.
Their implementation is merged in #147/#148, with final Linux/macOS/Windows CI
passing 1,039 Rust tests on each Unix host and 1,018 on Windows after #148.

`release_workflows` adds independent author working repositories and bare remotes.
The author explicitly runs tests/preflight, commits, creates an annotated tag and
pushes that single tag with normal Git. A separate application resolves a
compatible diamond using real version selectors. It verifies one shared
initialization/type identity and normal/error/async panic resource cleanup after
all remotes become unavailable. Additional cases exercise:

- Read-only online tag listing, canonical-name filtering, numeric/pre-1.0 lines,
  transitive selections and explicit rejection of invalid advertised source.
- Offline historical/shared bindings labeled stale and incomplete, with unchanged
  manifests, locks, cache files and directories; missing/corrupt state never repaired.
- Successful idempotent push retry, rejected replacement, deliberately forced
  retag rejection by known bindings, new-version correction and deleted tags.
- Remote diagnostic/credential redaction and missing native toolchains for reports.

These are local real-Git integration tests, registered in the ordinary three-host
CI suite. They do not test a hosted service's authentication policy or promise
server-enforced immutable tags. Final #146 CI results belong in its PR record.

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

Tags in that historical experiment were manually resolved to commits with Git. The compiler never received a
version selector or resolved a tag. The accepted manifest `version` metadata
remained unchecked in that implementation.

| Step | Observed result |
| --- | --- |
| Install the separate-library diamond | Success; one shared project record in the entry lock |
| Check with `--locked --offline` and nonexistent C/Rust compiler overrides | Success without a native toolchain |
| Run with `--locked --offline` | `Hello, Ada!` followed by `person:Ada` |
| Test the application with `--locked --offline` | One shared-type test passed |
| Commit shared 0.2.0 and move its 0.1.0 tag to that commit; repeat locked install | Success using the original pinned commit; lock bytes unchanged |
| Add a conflicting direct shared pin while transitive dependencies retain the original pin | Install rejected the conflicting project; lock bytes unchanged |
| Restore the original manifest and make all three repository locations unavailable | Locked offline install/check/run/test all succeeded from the validated cache; output/type identity and lock bytes unchanged |

Ten compiler commands completed: nine successes and one expected hard-pin
conflict rejection. These were disposable local repositories, not hosted package
releases for readers to install.

The experiment supports the simpler tag-first recommendation: Git can publish a
release label, while an application locks its selected commit. It does not prove
that Zore resolves tags, detects retagged version identities or implements MVS.
There was no real hosted authentication or publishing-service test.

## Proposal-model checks after review

A separate in-memory model checked the revised minimum-version rule. Its release
manifests were a small synthetic dependency graph. It traversed each requested
release, collected requirements and chose the highest minimum in one compatible
line. Hard-pin equality was represented by a fixed release key; this was not a
Git source verifier or a production resolver.

| Model case | Result |
| --- | --- |
| Diamond requiring shared 1.2.3 and 1.2.4 | Selected 1.2.4 |
| Compatible minor requirements 1.2.3 and 1.4.0 | Selected 1.4.0 |
| App adds a shared 1.5.0 minimum | Selected 1.5.0 |
| Only 1.2.3 is required, while 1.9.0 exists | Selected 1.2.3 |
| A higher requested greeting release adds another transitive requirement | Expanded it and selected the resulting shared 1.5.0 minimum |
| Pre-1.0 requirements 0.3.1 and 0.3.4 | Selected 0.3.4 |
| Hard pin matches the selected release key | Accepted |
| Major 1 and major 2 requirements | Rejected |
| Pre-1.0 minor 0.3 and 0.4 requirements | Rejected |
| Hard pin disagrees with the selected release key | Rejected without overriding the pin |
| Required 1.2.8 is missing, while 1.9.0 exists | Rejected instead of substituting the latest |
| All 24 permutations of four root requests | Same selected versions |

All 12 model cases passed their expected outcomes. They check the proposal's
internal consistency. The implemented suites above now provide direct tag,
metadata, lock, cache and graph conformance, including requirements introduced by
higher releases and visit-order independence.

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

## Acceptance coverage and deferred services

The historical Git tests and model alone do not validate the implemented release
path. The increments add direct evidence for these acceptance requirements:

- Release metadata/version grammar, compatibility claims, optional-field warnings,
  minimal required fields or tag/manifest name/version mismatches.
- Actual tag resolution and full requirement-graph expansion across repositories,
  compatible selection, pre-1.0 lines, hard pins and source conflicts.
- Versioned lock/cache mapping, moved-tag detection, preservation of known commits,
  unavailable remote refs and offline metadata for unselected requested releases.
- Read-only outdated reports, newer incompatible lines, invalid advertised tags
  and explicit updates without unexpected manifest/lock writes.
- The real author/tag-push/consumer workflow, rejected pushes and credential
  redaction on Linux, macOS and Windows.

The named suites above repeat the independent-library application proof using
implemented version requirements and preserve nominal identity and cleanup.
A manual-tag/full-pin experiment or in-memory MVS model cannot replace those
acceptance tests. Catalogs, yanks, automated publishers and hosted discovery
retain separate scope.
