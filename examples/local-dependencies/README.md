# Local dependency example

This folder contains four independent projects:

| Project | Direct dependencies | Purpose |
| --- | --- | --- |
| `app` (`webapp`) | `greeting`, `audit` | Runs the application and tests shared type identity |
| `greeting` | `shared` | Constructs people and formats greetings through its `text` subpackage |
| `audit` | `shared` | Accepts the same shared person type |
| `shared` | None | Defines the common `Person` type |

From the repository root:

```sh
zore check examples/local-dependencies/app/main.ore
zore run examples/local-dependencies/app/main.ore
zore test examples/local-dependencies/app
zore test examples/local-dependencies/greeting
```

Output:

```text
Hello, Ada!
person:Ada
```

`app` uses `Person` values returned by `greeting` without importing `shared`.
It would need its own direct `shared` dependency to import that project by name.
Each path is resolved relative to the manifest declaring it. Tests in libraries
are selected explicitly, rather than run implicitly when the application tests.

See [the dependency guide](../../docs/local-dependencies.md) for the manifest
subset, conflict rules and local-source limitations.
