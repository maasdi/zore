# Versioned package distribution

Status: proposal for [#139](https://github.com/maasdi/zore/issues/139), following
local dependencies (#136), pinned Git consumption (#137) and curated native
packages (#138). This is a planning record, not a locked compiler contract.
Only local paths and full Git commit pins work today. The manifest examples and
commands marked proposed below must not be presented as implemented features.

## Recommendation

Start with exact-version releases backed by a package author's Git repository.
Keep the entry application's lockfile authoritative and permit one selection per
project name in a graph. Use a repository-local release catalog for version
lookup and availability status. A hosted global registry is not needed for this
first distribution path.

| Route | Benefit | Cost or limit |
| --- | --- | --- |
| Full Git pins, available today | Immutable selected contents, validated cache, offline consumption | Consumers select commits manually; version is unchecked metadata |
| Exact versions with a repository-local catalog, proposed first | Named releases, publishing ownership through Git, no new hosted service | New catalog/metadata/publishing support; first lookup trusts the repository |
| Hosted global registry, later if needed | Cross-repository discovery and centrally enforced name/release policy | Service operation, account recovery, moderation, credentials and availability |

Git tags alone are insufficient release records: they can move and do not define
yank status. They can remain human-facing aliases, but a resolver must select an
explicit catalog record and record its immutable source identity in the lock.
No floating branches, `latest`, implicit upgrades or general version solver are
part of the first versioned path.

## Experience from the current implementation

- `install` fetches explicit missing sources and creates a missing entry lock;
  an existing lock must match. `lock` replaces it after reviewed manifest edits.
  Check/build/run/test neither fetch nor rewrite locks.
- The cache verifies the full Git source tree, file bytes and supported modes.
  Git source identity includes the exact URL, commit and project subdirectory.
  Local and Git copies remain distinct even when their contents match.
- A graph has one project per name. Diamond dependencies share nominal types and
  initialization; conflicting selections fail. Direct dependency declarations
  still control import visibility.
- Version strings are accepted as metadata, but are not retained as a validated
  release identity or used for resolution. There is no publishing command,
  release catalog, metadata compatibility check, yank policy or registry search.
- Native packages own their reviewed sources. Distribution must retain their
  approved recipe checks; publishing cannot register an arbitrary C/Rust backend.

These are useful foundations to reuse, not a reason to add another resolver,
artifact cache or ownership model.

## Identity, names and exact versions

The publishing authority is the explicit repository URL. A logical package is
identified by that URL, its portable project subdirectory and its manifest name.
URL spellings are not guessed equivalent. A move to another repository or project
subdirectory is an explicit source change, not a transparent identity redirect.
The selected commit determines the physical source identity used by the compiler.

Keep current project names and import paths. There is no global claim on `auth`:
two repositories may publish it, but one application graph cannot select both
under that name. Do not silently rename imports, merge nominal types, or load
multiple incompatible copies. Conflicts should show both dependency paths,
repository/project selections and requested versions, with an instruction to
align the declarations or use one compatible library release.

The proposed first selector accepts exact canonical `MAJOR.MINOR.PATCH` versions
only: decimal components without leading zeroes, except `0` itself. No ranges,
caret/tilde constraints, wildcards, prereleases or build suffixes initially.
Version numbers communicate the author's compatibility promise; the compiler
does not prove API compatibility. A major bump, including a major-zero minor
bump, still requires an explicit dependency edit. No automatic compatible choice
or fallback to a different version occurs.

Proposed dependency syntax, not currently accepted:

```toml
name = "webapp"
[dependencies]
auth = { git = "https://example.org/team/auth.git", version = "1.2.3" }
```

`git` plus exactly one of `rev` or `version` would be required, with the existing
optional `subdir`. A selected project's name/version/subdirectory must match the
release record. Path and full-revision dependencies remain supported and need no
release catalog. Full-revision and versioned Git declarations can share a project
only when they resolve to the same repository, project directory and source
revision; version assertions must still agree. A local path and a Git selection
remain distinct, including a local spelling of a cached checkout. A repository
cannot assign one version to two different source identities.

## Release contents and integrity

Use the existing supported Git tree as the source artifact: regular files and
directories, portable UTF-8 paths, no symlinks/submodules/filters/build scripts.
Include committed source, tests, documentation, examples, license and registered
native sources. Do not create a second archive inclusion/exclusion language.
The complete repository snapshot remains the fetched artifact for monorepos.
Path dependencies in a release must stay inside that snapshot; external libraries
must use explicit immutable pins or supported exact-version declarations.

Each catalog release record binds these identity fields permanently:

| Field | Purpose |
| --- | --- |
| Project name, exact version and project subdirectory | Match the requested package |
| Source commit and Git root-tree IDs | Attribute and fetch the exact source |
| Manifest SHA-256 | Verify the package metadata and dependency declarations |
| Source-tree SHA-256 | Check the full portable artifact independently of its Git IDs |

Define the source digest over a versioned, length-prefixed encoding of sorted
repository-relative UTF-8 file paths, supported modes and raw contents. Include
all tracked files, exclude Git administration data, and do not normalize source
line endings. The byte encoding and catalog schema must be locked before writing
a resolver; use cross-host digest vectors, not a host directory listing order.

A released identity cannot be overwritten by normal publishing, including a
republish with the same name/version but different contents. Corrections need a
new version. Keep a retention ref for each source commit and never force-update
or remove it through publishing tools. A content digest proves bytes, not author
identity, license validity, safety, or long-term availability on a remote server.

## Catalog, resolution and locks

Keep one versioned catalog on a dedicated Git branch per repository. Store release
identity separately from mutable availability/deprecation advisories. A catalog
snapshot is itself a Git commit. Its exact branch name and wire encoding belong
in the implementation's locked protocol, rather than an accidental parser choice.

For an unlocked exact-version request, install/lock reads one catalog snapshot per
repository and selects only the requested version. It verifies the record against
the fetched source, traverses dependencies in stable order, and reports conflicts
rather than backtracking. Missing versions, unsupported schemas and incompatible
metadata fail explicitly; they do not select another version.

Extend the lock format in a versioned way. Versioned records need the requested
version, catalog snapshot, source commit/tree and both SHA-256 digests, alongside
existing graph manifest records and native recipes. Existing version-1 Git/path
locks remain valid for their existing dependency forms. Do not silently downgrade
a new lock or drop provenance fields when reading one.

Repeated install with a matching lock uses those selections, including on a clean
machine; it fetches the locked commit rather than consulting a current catalog
tip. Build/check/run/test remain cache-only. Explicit `lock` preserves an existing
release identity when its logical request has not changed, while resolving edited
requests. A newly observed record that reassigns a known name/version is an error,
not drift that `lock` silently approves.

Cache validated catalog snapshots and source artifacts atomically. Offline
resolution of a new version needs an already cached catalog record and source;
otherwise give an actionable cache-miss error. A complete locked graph can be
checked, built, run and tested offline without the catalog branch or original
server. Offline operation cannot establish current remote advisory status.

## Consumer metadata

Add a separately specified manifest metadata table, keeping source-selection
fields distinct from descriptive claims. The publisher validates metadata before
uploading; consumers cross-check it against the immutable manifest.

| Metadata | Proposed release requirement |
| --- | --- |
| Description, readme and source/repository attribution | Required; readme path confined to the project; selected Git URL is authoritative source attribution |
| SPDX license expression or a confined license-file path | Required; retain corresponding license/notice files in the source artifact |
| Supported Zore versions | Required, simple explicit lower/upper bounds; evaluate the current compiler, without choosing another package version |
| Supported platforms | Explicit supported host triples or an explicit portable-source declaration; native recipes remain authoritative |
| Documentation/examples/tests | Required readme instructions; committed tests/examples when present, with visible coverage limits |
| Native/toolchain requirements | Derive registered recipe requirements and describe extra prerequisites; never execute package build scripts |

Metadata bounds are not a proof that every compiler/platform combination works.
Before 1.0, avoid assuming compatibility across Zore minor releases. Unknown
required metadata, invalid bounds and an unsupported current host/compiler must
produce actionable diagnostics. Native toolchain availability remains a build
concern; checking portable source should not require a C compiler or linker.
The exact metadata keys, optional fields and grammar need a locked specification
and parser conformance cases in the implementation PR.

## Publishing, ownership and authentication

Proposed maintainer workflow, not currently implemented:

1. Prepare a new exact version in a clean committed project. Run formatting,
   library tests and the advertised CI matrix. Review license files, dependency
   closure, native requirements and the artifact contents.
2. Run `zore publish --check --repository <url> <project>` to validate the release
   and show the version, source commit, digest and destination. This mode writes
   nothing remotely and executes no library code or build scripts. Test execution
   is an explicit maintainer/CI step, not a hidden installer action.
3. Run `zore publish --repository <url> <project>` to create the retention ref and
   catalog record atomically, using ordinary Git authentication. No guessed
   `origin`, registry account, embedded token or installer-side code execution.
4. A consumer adds the exact version, runs install, reviews and commits the lock.
   Missing access, bad metadata, missing versions, conflicting selections and
   existing releases each receive distinct diagnostics without credential output.

All writers authorized by the repository can publish its packages; Zore adds no
independent name-ownership service. HTTPS credential helpers and SSH agents retain
their existing role. Public consumption does not require a Zore account; private
repositories require their normal Git access. Credential values never enter
manifests, lockfiles, catalog records, hashes, logs or diagnostics.

Prepare the catalog update on its observed parent and use an atomic Git push of
the new source-retention ref and fast-forward catalog update. If the server lacks
atomic ref updates, fail before publishing. Concurrent publication must either
reuse an identical completed release or fail for review/retry; it cannot leave a
half-published release or overwrite another publisher. Read-only Git/file sources
remain consumable, but do not gain a publishing permission bypass.

Git permissions and hosting-side protected refs enforce repository ownership.
A writer bypassing the publishing tool can alter catalog contents, and an
administrator can rewrite remote history. A lock/cache can detect replacement of
an identity it already knows; first-time discovery cannot prove that unseen
history or mappings were never changed. Stronger independent transparency,
account recovery, guaranteed retention and globally enforced immutability require
additional infrastructure. Do not describe a Git catalog as providing them.

## Yanks, deprecation and incompatible releases

Yanking changes availability advice, never the source artifact or immutable
release record. New unlocked resolution rejects a known yanked version. Existing
locks continue selecting and verifying the original bytes, including when
repopulating a cache; display a known advisory without silently upgrading.
Deprecation warns with a reason or replacement suggestion and preserves selection.
A cached advisory is useful offline but is not evidence of current remote status.

Status changes require the same repository publishing authority and a reviewed
catalog update. They are not deletion or reassignment of the version. Security
advisories and user-defined denial policy may be added later; a yank must not be
misrepresented as revocation of already distributed source. An incompatible
release has a new version; resolving two requested versions in one graph reports
the conflict instead of loading two nominally different copies under one name.

## Discovery, migration and staged delivery

Start discovery with repository readmes, documentation, examples and a browsable
catalog listing versions, descriptions, license, compatibility and status. A
curated directory may link to explicit repository identities later. Neither a
search result nor a repository redirect silently chooses a dependency source.
Global name registration, hosted search, mirrored artifact hosting, advanced
solving and prerelease/range selection are separate decisions driven by use.

Local development keeps path dependencies. Commit and test the library, publish a
release, replace the application's path/full-pin declaration with an exact-version
request, regenerate the application lock and review the source change. Existing
Git pins need no migration. Import spelling and the public Zore API do not change.
A released library's development lock remains useful for its own CI; the consuming
application lock continues governing the whole graph.

Deliver the work in independently reviewable increments:

1. [#144](https://github.com/maasdi/zore/issues/144): lock release metadata/version
   grammar and canonical artifact digest; add local
   release validation with rejection cases and cross-host digest fixtures.
2. [#145](https://github.com/maasdi/zore/issues/145), after #144: lock the Git catalog
   and lock extension; implement exact-version consumption,
   conflict/provenance checks and cached offline operation, reusing the Git cache.
3. [#146](https://github.com/maasdi/zore/issues/146), after #144/#145: implement
   checked/atomic publishing and advisory updates with repository-based
   authority, including concurrency, interrupted pushes and credential failures.
4. Add discovery tooling or a hosted service only after real package experience
   establishes a need. Do not declare registry/distribution implemented when only
   this planning record or a metadata validator has landed.

Before each implementation, update and lock the relevant specification under §53.
No source keywords, runtime dependencies or new ownership rules are required by
this proposal. Loading and native linking still use ordinary package identity,
visibility, borrowing, Move semantics, errors, async retention and cleanup.

## Evidence and implementation acceptance

The [validation record](package-distribution-validation.md) distinguishes an
executed workflow using current full Git pins from the unimplemented versioned
protocol. Existing Git/native conformance provides further source integrity and
ownership coverage; it is not evidence for catalog/publishing behavior.

Every future increment needs positive and rejection coverage on Linux, macOS and
Windows: metadata/version grammar; deterministic digest and resolution; diamonds
and conflicts; native recipe checks; missing/corrupt cache and lock drift; offline
check/build/run/test; moved/deleted remote refs; yanked/deprecated versions;
unauthorized or failed publishing; concurrent and interrupted atomic updates;
and independent-library type identity plus normal/error/panic/async cleanup.
