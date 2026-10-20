# Publish a library and find updates

Zore consumes ordinary Git releases. Each library can have its own repository,
maintainers and Git access policy. A release needs a name, canonical version and
licensing in its committed `zore.toml`:

```toml
name = "greeting"
version = "1.2.3"
license = "MIT OR Apache-2.0"
```

Keep license/notice files, source, tests and documentation in the repository.
[Release metadata](release-metadata.md) describes optional fields and native
source requirements. The project name and version determine the exact tag
`greeting/v1.2.3`. In a monorepo the consumer also names the exact `subdir`
containing this project's manifest. Release path dependencies must stay inside
the same repository snapshot.

## Author a release

From the library's Git working directory, with `origin` already configured:

```sh
zore install .
zore check ./greeting.ore
zore test .
zore check --release --tag greeting/v1.2.3 .
git diff
git add .
git diff --cached
git commit -m "Release greeting 1.2.3"
git tag -a greeting/v1.2.3 -m "Release greeting 1.2.3"
git push origin refs/tags/greeting/v1.2.3
```

If an existing lock no longer matches an intentionally edited manifest, use
`zore lock .` and review its diff before checking/testing. Retain the repository's
CI and review process. Tests are explicit actions. `check --release` is an optional
local metadata preflight: it neither runs tests nor checks the Git ref, uploads
sources, creates a release or writes dependency state. Lightweight tags work too.
Pushing a tag transfers the reachable source commit; the release workflow does
not depend on a separate branch push succeeding at the same time. No catalog
branch, extra retention ref, atomic multi-ref push, Zore account or `zore publish`
command is needed.

Use normal Git repository permissions, HTTPS credential helpers or SSH agents.
Do not put access tokens or passwords in `git` URLs in manifests. Git handles
authentication; Zore does not maintain another account or credential store.

If a push fails, inspect its Git error and the remote ref:

```sh
git ls-remote origin refs/tags/greeting/v1.2.3
git rev-parse refs/tags/greeting/v1.2.3
```

If the server accepted the same tag but the client lost its response, repeating
the same push is safe. A different existing tag is a conflict: inspect its
contents and coordinate with maintainers. Even annotated tags pointing to the
same commit can have different tag objects. Do not force-update a published
version. Correct the source, raise the manifest version and publish a new tag.
Release notes can explain fixes or warnings.

Git hosts may permit force-updates and deletion; Zore does not claim
server-enforced immutable history. Locks and shared bindings reject a changed
identity they already know, while first resolution trusts the repository's
current tag. Deleting a tag does not revoke a cached or locked source. An
uncached commit may become unavailable once all retaining refs are removed and
the server garbage-collects it. Keep published tags available and retain source
mirrors/caches where long-term availability matters.

## Consume and inspect updates

An application declares the exact repository URL and a compatible minimum:

```toml
name = "webapp"
[dependencies]
greeting = { git = "https://example.org/team/greeting.git", version = "1.2.3" }
```

Replace the illustrative URL with the author's real repository. Then:

```sh
zore install ./app
zore check --locked --offline ./app/main.ore
zore test --locked --offline ./app
zore outdated ./app
```

Commit `zore.lock` together with the manifest. `outdated` requires a matching
lock and the complete verified source cache that locked checking uses. It
reports every selected tagged dependency, including transitive projects. Local
and full-revision sources have no inferred release line and are omitted.

Example report:

```text
Advertised tags only; source metadata is unverified until explicit lock.
PROJECT   LOCKED   NEWEST COMPATIBLE   NEWER INCOMPATIBLE LINES
greeting  1.2.3    1.4.0               2.3.0, 3.0.0
```

Actual columns are separated by tabs. Versions are ordered numerically, so
1.10.0 follows 1.9.0. Major 1 and higher share a line by major; before 1.0, the
minor must also match. Each newer incompatible line shows its newest advertised
version. Malformed/noncanonical release names, prereleases and build suffixes
are ignored. A `-` means no matching advertised version, including when tags
have disappeared; a reported compatible version may then be older than the lock.

The command lists ref names without fetching their source. A reported tag might
have invalid metadata, unsupported platform claims or even point to a non-commit
object. Discovery does not prove API compatibility or select that source.
Existing commit, name/version, licensing, compatibility and native recipe
validation still applies when explicitly resolving an update.

`zore outdated --offline ./app` makes no remote lookup and labels the report
**stale and incomplete cached releases**. It combines historical lock bindings
with immutable release bindings already in the shared cache. An online report
does not add advertised versions to that cache, so an offline report can know
fewer versions. Missing binding-index directories supply no extra versions;
corrupt published records fail without repair. Install sources before using
either mode with an empty cache.

Neither mode edits manifests, locks, guards or caches, executes source, or needs
a native toolchain. Both exit 0 when reporting succeeds, even if updates exist;
validation/access failures exit 1 and usage errors exit 2. `--locked` is rejected
because a matching lock is always required. Ordinary install/check/build/run/test
do not discover or silently choose a newer advertised version.

## Apply an update deliberately

Change a direct dependency minimum in the application manifest. To raise a
transitive `shared` dependency, add a direct requirement using its existing exact
source identity:

```toml
[dependencies]
greeting = { git = "https://example.org/team/greeting.git", version = "1.2.3" }
shared = { git = "https://example.org/team/shared.git", version = "1.4.0" }
```

Then:

```sh
zore lock ./app
git diff -- app/zore.toml app/zore.lock
zore check --locked --offline ./app/main.ore
zore test --locked --offline ./app
```

The highest requested minimum wins within one line, retaining requirements from
all requested releases. The direct requirement cannot lower a transitive
minimum, cross its compatibility line or override a hard revision pin. An
incompatible upgrade needs coordinated changes to the libraries that still
require the older line. Review versions, commits, source identities and newly
introduced requirements in the lock diff before committing it. Removing a higher
requirement can lower the selection; historical bindings do not force it to stay.
See [tagged dependencies](versioned-dependencies.md) for the full selection and
offline contract.

Machine-readable yanks/deprecations, global search, catalogs, automated multi-ref
publishing and stronger retention/transparency services remain separate work.
Repository readmes, examples and release pages provide initial discovery and
warnings.
