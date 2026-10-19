# Tagged minimum-version dependencies

A Git dependency can require a released version instead of pinning a commit:

```toml
name = "webapp"
[dependencies]
shared = { git = "https://example.org/team/shared.git", version = "1.2.3" }
```

Use a real repository and published tag in place of this illustrative URL. Its
`shared/v1.2.3` tag must resolve to a commit whose root `zore.toml` declares
`name = "shared"`, `version = "1.2.3"` and valid licensing. Add
`subdir = "libraries/shared"` for a project in a monorepo; that directory must
contain its own manifest. Subdirectory and `zore.toml` names use exact Git path
spelling on every host. Lightweight and annotated tags work. Release
[metadata](release-metadata.md), compatibility bounds and curated native source
recipes are validated before any code is loaded. Repository/documentation links
in a manifest cannot redirect the selected source URL.

`version` means a compatible minimum. It does not mean an exact pin or the latest
published version. Versions use canonical `MAJOR.MINOR.PATCH`, with no leading
zeroes, ranges, prereleases or build suffixes. Versions with major 1 or higher
share a compatibility line by major; before 1.0 the minor must also match.

| Requirements for shared | Selection |
| --- | --- |
| 1.2.3 and 1.2.4 | 1.2.4 |
| 1.2.3 and 1.4.0 | 1.4.0 |
| Only 1.2.3; 1.9.0 also exists | 1.2.3 |
| 0.3.1 and 0.3.4 | 0.3.4 |
| 0.3.1 and 0.4.0, or 1.x and 2.x | Conflict |

The `x` notation explains compatibility lines; it is not dependency syntax.
A direct application requirement can raise a transitive minimum. It cannot
force a lower version, cross a compatibility line or override a full revision.
`git` accepts exactly one of `version` or `rev`; existing path/full-revision
projects need no migration or release metadata. Full revisions and fetched
snapshot path dependencies remain hard pins that must agree with the selected
commit. One project name has one exact URL/subdirectory or one canonical local
root. Source and pin conflicts report both dependency paths.

## Install and consume

```sh
zore install ./app
zore check --locked --offline ./app/main.ore
zore run --locked --offline ./app/main.ore
zore test --locked --offline ./app
```

Install resolves each requested tag, expands its manifest requirements and selects
the highest required version in one line. It retains requirements from superseded
minima and from higher requested releases, without searching unrequested tags or
backtracking. For example, if shared 1.2.3 requires extra 1.0.0 and shared 1.2.4
requires second 1.0.0, both requirements remain even when shared 1.2.4 is selected.
Only selected versions enter package loading. Direct import visibility, nominal
type identity, initialization, ownership and async cleanup remain unchanged.
Selected project graphs still reject cycles; finite cycles among requested older
releases do not independently reject an otherwise acyclic selected graph.

Commit the generated `zore.lock`. Tagged graphs use version 2. Its `[projects]`
records show selected versions/tags and exact commits/trees. `[releases]` records
also retain the identities, manifest digests and native recipe fingerprints of
all requested release manifests, including unselected minima. Records are sorted
and numbered canonically; do not hand-edit them. Existing path/revision-only
workflows retain version-1 lock output and validation.

A matching lock is authoritative: repeated install can fetch exact commits on a
new machine without querying tags. Check/build/run/test only use verified cache
entries and never fetch or rewrite locks, even without `--offline`. The cache
retains immutable release bindings and the existing Git snapshots; snapshots are
verified against Git objects, including bytes, paths and supported file modes.
No source-tree checksum scheme, catalog or publisher is introduced.

## Change a minimum deliberately

Edit the application's requirements, then:

```sh
zore lock ./app
# Review the selected-version and commit changes in zore.lock.
```

Explicit lock recomputes the request graph, so requirements can raise versions
or removing a higher requirement can lower them. Historical release bindings
remain in the lock to preserve known identities; they do not themselves add
requirements or force old selections to stay in the build list. Online lock
checks requested tags against those identities and rejects retagging. Publish a
new version instead of moving a known version to new contents.

Matching locked installs still work when tags move or disappear, provided the
exact commits can be fetched or are cached. Fresh lookup trusts the repository's
tag; locks cannot prove its earlier history. Deleted, uncached commits may become
unavailable after server garbage collection.

`zore lock --offline` recomputes using known bindings and verified cached source.
Offline resolution needs snapshots for every requested release manifest, including
unselected minima, and for the selected project graph. A missing or corrupt
snapshot/binding produces an actionable error and is never silently repaired.
An offline first install is possible when all required bindings and snapshots
are already cached; a fresh cache needs an online install. Once present, an
unchanged locked graph works without access to any source repository.

Version-2 locks retain historical identities even after tagged requirements are
removed. Ordinary full-revision behavior, credentials, cache configuration,
manifest-drift checks and native linking requirements follow the existing
[Git dependency guide](git-dependencies.md). Dependency resolution executes no
library code or build scripts and requires no native linker. Read-only update
reporting and complete author workflow validation remain tracked by #146.
