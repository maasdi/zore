# Local project dependencies

A project can depend on another local Zore project through `zore.toml`:

```toml
name = "webapp"
version = "0.1.0"

[dependencies]
greeting = { path = "../greeting" }
```

The target folder must contain its own `zore.toml` with `name = "greeting"`.
The dependency key must match that name. Relative paths are resolved from the
manifest declaring them, regardless of the shell's working directory. Absolute
paths work too. Windows drive-relative (`C:folder`) and root-relative paths
without a volume are rejected. Literal strings preserve Windows backslashes:

```toml
[dependencies]
greeting = { path = 'C:\work\greeting' }
```

Import the library's root package or one of its subpackages:

```ore
import "greeting"
import "greeting/text"
```

Each package must declare the final import-path segment as its package name.
Root packages therefore use the project's name; `main` packages cannot be
imported. Existing imports within a project and `std/...` imports keep working.
There are no dependency aliases or new source keywords.

## Dependency graphs

Each library declares its own dependencies. If `greeting` imports `shared`, it
lists `shared` in its own manifest. The application can use a value returned by
`greeting` whose type comes from `shared`, but must declare `shared` directly
before writing an import of it. Resolution checks the importing project's
permissions before looking in the package cache.

Repeated references to one canonical project root share types and initialization.
A diamond graph initializes each imported package once. Two different roots with
the same project name are an error, including a conflict with the application.
All declared dependencies are validated, even when unused. Project and package
cycles are errors with a chain; this increment performs no version selection.

A dependency declaration may intentionally leave the application's folder tree.
Imports within that dependency cannot leave its canonical project root or cross
another nested project's manifest. Symlinks cannot bypass these boundaries.
Different package import paths exposing one canonical folder are rejected.

## Manifest format

The compiler supports a defined TOML subset: required `name`, optional `version`,
and one optional `[dependencies]` table containing inline `{ path = "..." }`
entries. Fields and entries fit on one line. Bare keys, spaces/tabs, LF/CRLF,
comments and single-line basic/literal strings are supported. Basic strings
support TOML escapes and Unicode scalar escapes. Unknown or duplicate fields,
unsupported tables, malformed strings and extra inline-table fields are errors.
Multiline strings, dotted/quoted keys, arrays, and inline-table trailing commas
are outside the subset. Diagnostics name the manifest and line.

Local dependencies are mutable source references. A lock records their manifests
and locations, not immutable source contents. See [pinned Git dependencies](git-dependencies.md)
for install/lock, application lockfiles and offline caching. Native integration
is #138, and distribution/registry work is deferred in #139.

## Commands and tests

`zore check`, `build`, `run` and `test` use the same dependency graph. Checking
available Zore sources does not need clang or a native linker. Compilation still
requires the ordinary host toolchain described in [platform support](platform-support.md).

Imported library `_test.ore` files stay excluded. Run `zore test` on the library
itself to check and execute its own tests. Selecting an application does not
implicitly select external dependency tests.

The [local-dependency example](../examples/local-dependencies) has an application,
two libraries and a shared library. It demonstrates root/subpackage imports,
a diamond graph and tests in independently selected projects.
