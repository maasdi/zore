# Maintained packages

The Zore project maintains two external packages with native providers: `tls`
for verified TLS and HTTPS, and `sqlite` for persistence. They are released
from this repository's [`packages/`](../packages) folder with ordinary Git tags,
so an application adds one dependency line and never copies a folder by hand.
Secure random bytes and HMAC need no package at all: they are in the standard
library.

## Add a release

```toml
name = "myapp"

[dependencies]
tls = { git = "https://github.com/maasdi/zore.git", subdir = "packages/tls", version = "0.1.0-alpha.2" }
sqlite = { git = "https://github.com/maasdi/zore.git", subdir = "packages/sqlite", version = "0.1.0-alpha.2" }
```

Keep only the packages you use. Then:

```sh
zore install .
git add zore.toml zore.lock
zore build --locked --offline ./main.ore
```

`install` looks up the tag, verifies the selected commit and records it in
`zore.lock`. Commit the lock with your application. After that, `check`,
`build`, `run`, `test` and `install --locked` work offline from the populated
cache and keep the same bytes until you change the manifest and run
`zore lock .`. `zore outdated .` lists newer releases without changing
anything. `version = "0.1.0-alpha.2"` means at least that release within the
0.1 line, so it can later move to `0.1.0-alpha.3` or `0.1.0`; the lock holds
the exact choice. Each package release has the same version as the compiler
release it ships with, and the first one ships with compiler `v0.1.0-alpha.2`.
Use a compiler at least as new as the package version. See [tagged dependencies](versioned-dependencies.md)
for the selection rules and [release workflow](release-workflow.md) for the
cache and update details.

`zore check` needs no C compiler. Building needs the normal clang and Rust
toolchain on Linux GNU, macOS (x86_64, aarch64) or Windows x64 MSVC. No system
OpenSSL, SQLite or other native library is installed or downloaded.

## Secure random bytes and HMAC

```ore
package main

import "std/crypto/hmac"
import "std/crypto/rand"
import "std/crypto/subtle"
import "std/errors"
import "std/strings"

func verified(message string) (bool, error) {
    let key = rand.Bytes(32)?
    let data = strings.Bytes(message)
    let tag = hmac.SHA256(key[:], data[:])
    let expected = hmac.SHA256(key[:], data[:])
    return subtle.ConstantTimeEqual(tag[:], expected[:]), nil
}

func main() {
    let ok, err = verified("hello")
    if err != nil { panic(errors.Message(err)) }
    println(ok)
}
```

Compare MACs with `subtle.ConstantTimeEqual`, never with an early-exit
comparison such as `bytes.Equal`. Random generation fails with an error rather than falling back to weak entropy. The
older `crypto` package in `examples/crypto/crypto` is kept for existing code
and is not released.

## Verified HTTPS

```ore
package main

import "std/context"
import "std/net/http"
import "std/os"
import "tls/https"

func fetch(url string) error {
    let client = https.NewDefaultClient(http.DefaultClientConfig())?
    let response = client.Get(context.Background(), url)?
    println(response.StatusCode)
    return nil
}

func fetchWithRoots(rootsFile string, url string) error {
    let roots = os.ReadFile(rootsFile)?
    let client = https.NewClient(roots[:], http.DefaultClientConfig())?
    let response = client.Get(context.Background(), url)?
    println(response.StatusCode)
    return nil
}
```

`NewDefaultClient` trusts the platform roots. `NewClient` trusts exactly the
given PEM roots and rejects an empty bundle. Both always verify the chain,
validity time and host name; there is no option to skip verification. The
[TLS package readme](../packages/tls/README.md) describes root discovery,
protocol limits, deadlines and the lower-level `tls` connection API.

## SQLite persistence

```ore
package main

import "sqlite"

func save(body string) error {
    let db = sqlite.Open("notes.db")?
    var create = db.Prepare("create table if not exists notes(body text not null)")?
    create.Step()?
    create.Close()?
    db.Transaction(func(tx mut sqlite.Transaction) error {
        var insert = tx.Prepare("insert into notes(body) values (?)")?
        insert.BindText(1, body)?
        insert.Step()?
        return nil
    })?
    return nil
}
```

Bind values as parameters instead of building SQL text. `Database`,
`Statement` and `Transaction` own native resources and close on drop; the
transaction commits when the callback returns nil and rolls back on an error
or panic. The [SQLite guide](libraries/sqlite.md) covers the full API,
blocking behavior and provider provenance.

## Ownership, updates and security

[`packages/README.md`](../packages/README.md) records each package's identity,
release policy, supported-host CI and how provider security fixes are shipped.
Report suspected vulnerabilities privately as described in
[SECURITY.md](../SECURITY.md).
