# Versioned package distribution

Status: proposal for [#139](https://github.com/maasdi/zore/issues/139), revised after
[PR #143 review](https://github.com/maasdi/zore/pull/143#pullrequestreview-5548103997).
This preserves the design reasoning. Release metadata (#144) is locked in spec
§3.4.4 and tagged minimum-version resolution (#145) in §3.4.5. The
[release metadata](../release-metadata.md) and [tagged dependency](../versioned-dependencies.md)
guides describe implemented behavior. Update discovery below remains proposed
work for #146.

## A library author releases a version

Prepare a committed library with a name, version and license. Run its tests,
then publish an ordinary Git tag:

```sh
git tag shared/v1.2.3
git push origin refs/tags/shared/v1.2.3
```

The proposed tag convention is `<project-name>/v<MAJOR.MINOR.PATCH>`, including
for a repository with one project. It also works for monorepos. Lightweight and
annotated tags are both allowed; each must resolve to a commit. The selected
project's manifest must declare the same name and version as its tag.

No catalog branch, extra retention refs, atomic multi-ref push or `zore publish`
command is required. The optional `zore check --release` metadata helper is
available without becoming a publishing prerequisite. Existing Git permissions and credentials
control who can push a tag.

## An application uses the library

Supported dependency syntax under §3.4.5:

```toml
name = "webapp"
[dependencies]
shared = { git = "https://example.org/team/shared.git", version = "1.2.3" }
```

Here `1.2.3` means **at least 1.2.3 within the same compatibility line**. It is
not an exact pin and does not mean "download the newest release". A matching
lockfile pins the exact version and commit that the application uses.

The existing install/build workflow remains:

```sh
zore install ./app
zore check --locked --offline ./app/main.ore
zore run --locked --offline ./app/main.ore
zore test --locked --offline ./app
```

Install would resolve required tags once, verify their source and write the
application lock. Commit that lock with the app. Repeated install uses the same
selections. Check/build/run/test remain cache-only and never rewrite a lock.

## Compatible requirements work together

Use a simple minimum-version selection model: choose the highest minimum needed
by the dependency graph, without searching every published version.

| Requirements for shared | Selected version |
| --- | --- |
| greeting needs 1.2.3; audit needs 1.2.4 | 1.2.4 |
| greeting needs 1.2.3; audit needs 1.4.0 | 1.4.0 |
| the app also requests 1.5.0 | 1.5.0 |
| 1.2.3 is the only requirement; 1.9.0 also exists remotely | 1.2.3 |
| one library needs 1.x and another needs 2.x | Conflict; incompatible lines |
| one library needs 0.3.1 and another needs 0.3.4 | 0.3.4 |
| one library needs 0.3.x and another needs 0.4.x | Conflict; incompatible pre-1.0 lines |

The `x` notation in this table describes a line; it is not manifest syntax.
For versions 1.0.0 and later, the major number defines a compatibility line.
Before 1.0.0, the minor number also matters: `0.3` and `0.4` are separate lines.
This is an explicit pre-1.0 restriction on the MVS-style model, not a claim that
all Go module rules apply to Zore. A version number is the author's compatibility
promise; it cannot prove that an API change is safe.

An application author can raise a shared minimum directly, without waiting for
both library authors. For example, adding shared 1.5.0 to the app's dependencies
raises the selection in the first example. The app cannot force a version below
a library's requirement or silently cross an incompatible line. A forced source
replacement or side-by-side major versions would need a separate design.

### Resolution rule

Before loading code, construct a requirement graph whose nodes are requested
releases. Start with the entry project's declarations. For each newly requested
release, resolve its tag, validate its manifest and visit its dependency
requirements. Visit each source/version pair once. Include requirements from all
requested releases, even when another request names a higher version of the same
library; discovering that higher release may add requirements of its own.

For each project, select the highest minimum appearing in that graph. Do not
inspect unrequested newer releases. Only the selected sources enter package
loading and compilation. This graph rule is monotone: discovered requirements
are not discarded during traversal, and there is no guessing or backtracking.
The selected package graph must still pass ordinary dependency/import cycle,
visibility, type and ownership checks.

Require one repository/project identity and one compatibility line per project
name. Otherwise report both dependency paths and the conflicting sources or
requirements. Missing releases and invalid metadata are errors, not invitations
to choose a different tag. Results must not depend on traversal or fetch order.

The first version grammar is canonical decimal `MAJOR.MINOR.PATCH`, without
leading zeroes except `0` itself. Compare components numerically. Range operators,
wildcards, prereleases and build suffixes are deferred.

## Names and full commit pins

Keep existing project names and import paths. The explicit repository URL,
portable project subdirectory and project name identify the library. URL
spellings are not guessed equivalent; a repository move is an explicit change.
There is no global ownership claim on a short name such as `shared`.

A graph still loads one source selection per project name. The simple source
rule is: **if two declarations pick the same library, they must land on the same
commit, or it is an error.** Minimum-version requirements first agree on their
selected version using the rule above.

Existing `path` and `git`/`rev` declarations keep their behavior. A full revision
is a hard pin, not another minimum: MVS must never replace it. If a hard pin and
a version-selected source disagree, show the pin and requirement paths and ask
the developer to align the pin or use a compatible version requirement. Local
paths and Git selections remain distinct, even for a local spelling of a cached
checkout. Unsupported source overrides do not happen implicitly.

`git` with exactly one of `rev` or `version`, plus the existing optional `subdir`,
would be the supported dependency forms. An existing full-pin library needs no
migration. Libraries choosing version requirements gain compatible patch/minor
selection instead of requiring every dependent author to synchronize exact pins.

## Locks, cache and moved tags

Reuse the current Git commit/tree verification and source cache. Check each
file's bytes, supported mode and path against the selected Git objects. Retain
the existing manifest SHA-256 records and registered native recipe fingerprints.
Do not add a custom source-tree digest or new artifact encoding in this increment.
A stronger independent checksum/transparency system can be evaluated separately.

A versioned lock records the selected version, canonical tag and exact commit/tree.
It also retains version-to-commit bindings for every release manifest used to
resolve the graph, including unselected minimum releases. That keeps their
requirements reproducible on another machine without building multiple versions.
Use the same verified bindings in the cache. Extend the lock format explicitly;
keep existing version-1 path/revision locks valid for those forms.

A matching lock is authoritative. Install can fetch its exact commits on a new
machine without resolving current tag tips. Build/check/run/test remain offline
with respect to dependency discovery. Unchanged locked graphs never upgrade just
because someone published a tag or because a command was repeated.

After dependency edits, explicit `zore lock` recomputes minimum-version selection.
It may raise a selected version when requirements rise, or lower it when the
higher requirement is removed; the lock diff makes that visible. Existing
version-to-commit bindings remain fixed. A moved tag for a known version is a
release-identity conflict, not a new implementation that lock regeneration
silently approves. Correcting source requires a new version/tag.

Offline resolution needs cached metadata and source for every requested release
needed to build the requirement graph, not only the eventual winners. A complete
locked graph needs no current tags or server. Cache misses/corruption receive the
existing actionable diagnostics; no installer code or package build script runs.

The first lookup trusts the repository's tag. Locks protect selected bytes;
they do not prove that a tag was never moved before the first lookup. Deleting
refs can also make an uncached commit unavailable after server garbage collection.
Keep release tags available; use validated caches or source mirrors when needed.
A catalog would not by itself remove this trust or retention limit.

## Release metadata and contents

Require only **name, version and license** for a versioned release. A license can
be an SPDX expression or a confined license-file path. Retain the corresponding
license/notice contents in the committed source. Existing local/full-pin projects
are not retroactively required to become published releases.

Description, readme, documentation, examples, test instructions, supported Zore
versions and supported platforms are optional. Maintainer validation can warn
about omissions without blocking a first release. Do not repeat missing-readme
warnings in every application build. When compatibility bounds or confined paths
are provided, validate and honor them rather than treating them as decoration.
The metadata keys and grammar are locked in spec §3.4.4. The local helper checks
one project's metadata and native recipe; Git tag lookup and whole-graph minimum selection are implemented under §3.4.5;
the complete author workflow remains #146 work.

Use the committed Git tree as the artifact, including tests/docs/examples/native
sources that are present. Keep the current portable-path and regular-file rules;
no archive inclusion language, source filters or arbitrary build scripts. A
release's path dependencies must stay inside its repository snapshot. Preserve
curated native recipe validation and build-time toolchain checks. Checking a
portable API still must not require a native linker.

The selected Git URL is authoritative source attribution. Optional repository
links are descriptive metadata, not a way to redirect resolution or assume two
URLs have the same package identity.

## Publishing and finding updates

Publish one tag at a time with ordinary Git. Use the repository's normal HTTPS
credential helper or SSH agent; no Zore account or embedded credentials. A failed
push has the usual Git retry path. If a tag already exists, inspect its target
instead of forcing it; changed contents need a new version. Optional validation
must not execute library code or upload anything secretly. Library test execution
is an explicit maintainer/CI action.

Proposed update workflow, not implemented:

```sh
zore outdated ./app
# Edit a direct minimum, or add a higher minimum for a shared dependency.
zore lock ./app
# Review and commit the manifest/lock diff.
```

`outdated` would list each locked version, the newest advertised compatible tag
and newer incompatible lines separately. It would not edit a manifest/lock or
change a selection. Tags must follow the canonical naming/version convention;
source metadata is still verified before any advertised version is selected.
The report needs explicit repository access; offline mode can report cached data
only and must label it as potentially stale.

Repository readmes, examples and hosting release pages provide initial discovery.
A hosted search service or curated directory can come later. No search result or
repository redirect silently changes a dependency's source.

## Yanks and later registry work

The first tag-based path has no machine-readable yank/deprecation protocol.
Authors can publish a corrected version and document warnings in their release
notes/readme. Deleting a tag makes new resolution fail if it is not cached; it is
not a reliable yank mechanism and does not revoke existing copies or locks.

If package experience justifies a catalog or advisory service, design it later:
a yank should block new selections while preserving existing locked bytes;
deprecation should warn without changing selection. Define offline staleness and
authority then. Do not build a catalog, multi-ref publisher, global name registry,
advanced solver or independent transparency system merely to provide those
future policies in the first release path.

## Delivery and evidence

1. [#144](https://github.com/maasdi/zore/issues/144): lock the release/version
   grammar and minimal metadata; validate tagged source and compatibility claims.
   Reuse existing source integrity checks; no new canonical digest format.
2. [#145](https://github.com/maasdi/zore/issues/145): lock and implement tag lookup,
   minimum-version graph selection, full-pin conflicts, lock provenance and
   offline consumption. Cover pre-1.0 lines and newly discovered requirements.
3. [#146](https://github.com/maasdi/zore/issues/146): validate the complete ordinary
   Git publishing/consumer workflow and add read-only update discovery. An
   optional preflight helper is allowed; no mandatory publisher or catalog.

Before implementing each increment, lock its specification under §53 and add
positive/rejection conformance on Linux, macOS and Windows. No source keywords,
runtime dependencies or new ownership rules are needed. Preserve nominal type
identity, visibility, borrowing, Move behavior and normal/error/panic/async cleanup.

The [validation record](package-distribution-validation.md) separates current
Git-pin experiments from proposal-model checks and still-unimplemented compiler
behavior. Required implementation cases include multi-pass requirement expansion,
order independence, compatible upgrades, incompatible lines, hard pins, tag/manifest
mismatches, moved/deleted tags, lock drift, missing/corrupt caches, offline graphs,
credential redaction and independent-library identity/cleanup.
