# Pinned Git dependencies

Declare the library's repository and an exact commit in `zore.toml`:

```toml
name = "webapp"

[dependencies]
auth = { git = "https://example.org/team/auth.git", rev = "0123456789abcdef0123456789abcdef01234567" }
```

Replace the illustrative URL and hash with a repository you can access and its
full 40-digit commit ID. The dependency key must match the selected project's
`name`. Imports work like local libraries: `import "auth"` or
`import "auth/tokens"`. There are no new source keywords or version constraints.

For a repository containing several projects, add `subdir = "libraries/auth"`.
The selected directory must hold its own `zore.toml`; ancestor discovery is not
accepted. Subdirectory paths use `/` and cannot contain dot/parent components,
backslashes or drive prefixes. Omit `subdir` to select the repository root.

## Application workflow

```sh
zore install ./app
zore check --locked --offline ./app/main.ore
zore build --locked --offline ./app/main.ore
zore run --locked --offline ./app/main.ore
zore test --locked --offline ./app
```

`install` downloads missing sources and creates the entry project's `zore.lock`.
Commit that lockfile with your application. A repeated install validates an existing
lock; it does not overwrite drift. To change dependencies, edit the manifest's
exact pin, review the change, then run `zore lock ./app`. Lock resolves exactly
those pins and replaces the lockfile; it never chooses a newer commit or rewrites
manifests. `install --locked` can populate a clean CI machine's cache while retaining
a committed lockfile. Git must be installed for fetching and cache validation.

Check/build/run/test always use available cached sources, even without flags.
They never download a dependency or write a lockfile. Missing cache entries point
to `zore install`; missing or changed locks point to install/lock. `--locked` requires
a matching lockfile and never writes it. `--offline` forbids network; install/lock
can use it when every source is cached. Flags can appear before or after the
target; use `--` before a target beginning with `-`. `fmt` needs neither manifests
nor dependencies and does not accept dependency flags.

Checking available sources requires no clang or native linker. Build/run/test
retain the ordinary [native toolchain requirements](platform-support.md).

## Graph identity and integrity

Every project's own manifest declares its direct dependencies. Transitive and
unused declarations are validated too. Repeated URL/commit/project-directory
selections share package types and initialization. Conflicting project names or
source selections are errors, rather than an implicit version choice. Ordinary
visibility, ownership, generics, errors, async and cleanup remain unchanged.

A fetched project may use a relative path dependency to a sibling project inside
its repository checkout. It cannot use an absolute path or leave that checkout.
Nested Git dependencies need explicit pins. Library lockfiles are ignored when a
library is consumed: the entry application's lock governs the whole graph.
Explicitly testing a library uses that library as the entry project instead.

The generated version-1 TOML lock records all project names, source locations,
exact manifest SHA-256 digests, and Git commit/root-tree IDs. Local paths are
canonical locations relative to the entry project where possible; moving a
project tree together preserves relative locations. Moving an external local
library may require running `zore lock`. Paths on a different Windows volume
remain absolute.
Manifest bytes, including comments and line endings, are significant; use the
same checkout line-ending policy in development and CI. Do not hand-edit locks.

Cache validation checks Git objects and compares every source file against the
pinned blobs. Changed bytes, executable modes on Unix, missing/extra paths,
symlinks and corrupt object data are errors. Builds never silently repair or
trust a corrupt entry. Git sources are immutable selections; local dependencies
remain mutable source references even with a lock. A lock does not freeze local
source file contents.

The compiler adds the Rust `sha2` crate for cache keys and manifest digests only.
This adds no dependency to generated Zore programs or their runtime.

## Credentials and supported layouts

Use explicit `https://`, `ssh://` or `file://` URLs. HTTPS credentials and SSH
passwords in URLs, query strings and fragments are rejected. SSH user names such
as `ssh://git@example.org/team/auth.git` work. Configure normal Git credential
helpers or an SSH agent; keep secrets out of manifests. Git subprocess stderr is
suppressed to avoid exposing helper/server credentials. Fetch failures ask you to
check repository access, credentials and pinned-commit availability.

Branches, tags, abbreviated hashes, SHA-256 Git repositories, submodules and
symlinks inside fetched trees are unsupported. Repository file names must be
UTF-8 and portable to Windows; reserved/invalid names and case collisions are
rejected. Unix executable file modes are preserved. Source files are materialized
from Git blobs without checkout hooks, source filters or library build scripts.

## Cache management and interrupted commands

`ZORE_CACHE_DIR` overrides the dependency cache root. Defaults are:

| Host | Dependency cache |
| --- | --- |
| Linux/other Unix | `$XDG_CACHE_HOME/zore/dependencies` or `~/.cache/zore/dependencies` |
| macOS | `~/Library/Caches/zore/dependencies` |
| Windows | `%LOCALAPPDATA%\zore\dependencies` |

The runtime build cache is separate. Cache keys hash the credential-free URL and
revision; application and library sources stay untouched. A fetch stages a
complete repository and validated snapshot, then publishes it atomically.
Concurrent fetches from different projects reuse the winner. Incomplete
`.zore-stage-*` directories are ignored and can be deleted when no command runs.

Install/lock serialize lockfile writes with `.zore-lock.guard` at the entry root.
A concurrent resolver reports an error you can retry. After a crash, remove that
guard only after confirming no resolver is active. Lockfile writes use an atomic
replacement, so readers see either the previous complete lock or the new one.

To recover a corrupt entry, remove the exact cache directory named by the error
and run `zore install --locked ./app`. You can also remove the entire dependency
cache while no command uses it, then run install again. Offline builds cannot
recover missing data; keep the pinned commits available on the server or preserve a
validated cache in CI. Cache data is reproducible from pins, not precious source.

For library development, start with [local dependencies](local-dependencies.md),
then pin a committed revision in consuming applications. Native-backed packages
can use [curated adapters](native-libraries.md). The
[versioned distribution proposal](design/package-distribution.md) describes tagged
releases, minimum-version selection and update discovery. Version requirements
and `outdated` are not implemented. Ordinary Git remains the publishing tool;
full commit pins remain the supported Zore dependency workflow.
