# SQLite library

The maintained external `sqlite` package in `examples/sqlite-library/sqlite`
uses the curated `sqlite-v1` adapter. Applications import `sqlite` through an
ordinary local or tagged Git dependency. The separate persistence application
is in `examples/sqlite-library/app`. No installed SQLite library is used.

## API

```ore
func Open(path string) (Database, error)
func (db Database) Prepare(sql string) (Statement, error)
func (db Database) SetBusyTimeout(milliseconds int) error
func (db mut Database) Close()
func (db own Database) Begin() (Transaction, error)
func (tx Transaction) Prepare(sql string) (Statement, error)
func (tx own Transaction) Commit() (Database, error)
func (tx own Transaction) Rollback() (Database, error)
func (s mut Statement) BindNull(index int) error
func (s mut Statement) BindInt(index int, value int) error
func (s mut Statement) BindFloat(index int, value float64) error
func (s mut Statement) BindText(index int, value string) error
func (s mut Statement) BindBytes(index int, value []uint8) error
func (s mut Statement) Step() (bool, error)
func (s mut Statement) Reset() error
func (s Statement) ColumnCount() (int, error)
func (s Statement) ColumnType(column int) (int, error)
func (s Statement) Int(column int) (int, error)
func (s Statement) Float(column int) (float64, error)
func (s Statement) Text(column int) (string, error)
func (s Statement) Bytes(column int) (Array<uint8>, error)
func (s mut Statement) Close() error
```

Use `:memory:` for a private in-memory database, or a UTF-8 filesystem path.
Open uses read/write/create and serialized mode, without URI interpretation.
Paths and SQL reject NUL. Prepare accepts one statement only, including trailing
whitespace, with at most 1 MiB SQL. SQL comments after the final statement are
outside this conservative surface. Use parameters for values, never SQL assembly.
Parameter indexes are SQLite's one-based indexes; columns are zero-based.
Unbound parameters are NULL. Binding is permitted before Step; Reset clears all
bindings and returns the statement to its initial state. Step returns true for
a row, false on completion, and an error on failure; call Reset before reuse.
Binding after stepping and stepping after completion/failure require Reset.

ColumnType returns `sqlite.Null`, `sqlite.Integer`, `sqlite.Real`, `sqlite.Text`,
or `sqlite.Blob`. Typed accessors require a current row and the exact type;
no implicit SQLite coercion occurs. Text and Bytes allocate independent owned
results, including empty values; text includes NUL and validates UTF-8. BindText
copies UTF-8 bytes and BindBytes copies arbitrary bytes. Values are bounded by
16 MiB. Float binds reject NaN because SQLite otherwise silently converts it to
NULL; infinities remain real values. The package does not expose row pointers.

Database, Statement and Transaction are ordinary Move resources. Statement
ownership includes a connection lease, so a statement may outlive its database
wrapper. Closing the database prevents new operations using it but preserves
existing statements. Statements finalize on Close/drop; Close reports SQLite's
finalize error while still releasing the token. Close is idempotent.

Begin consumes a Database and requires no live statements. Its BEGIN IMMEDIATE
transaction may prepare statements; close all statements before consuming the
transaction with Commit/Rollback. Either returns the Database on success.
Completion failure destroys the transaction and rolls it back. Dropping an
uncompleted transaction rolls back, removes its connection token and invalidates
escaped statements; their subsequent operations fail safely. Nested transactions
and savepoints are excluded. SQL transaction-control statements are rejected by
the provider's authorizer, keeping the resource contract intact.

All calls serialize per connection. Plain functions block synchronously with
worker compensation; async functions calling these library wrappers suspend
through the existing helper pool. Wrapper resources and borrowed input storage
remain live throughout native work. User plain helpers retain ordinary blocking
semantics. Destructors run compensated synchronous cleanup. No cancellation,
interrupt, context or hard deadline API is offered; dropping a task detaches it.
Busy timeout is 0–60,000 milliseconds and defaults to zero. It bounds lock retries,
not execution or filesystem latency. SQLite failures include a numeric SQLite
status and message; invalid tokens, types and phases are ordinary errors.

## Reviewed source and build recipe

SQLite 3.54.1 is the maintained upstream release selected from
https://www.sqlite.org/download.html on 2026-10-23. The upstream public-domain
notice is preserved in both source files; SQLite's licensing statement is at
https://www.sqlite.org/copyright.html. Zore wrapper/bridge code is Apache-2.0.
No Cargo dependency, dynamic loading, package build script or system SQLite SDK
is required. The source archive is:

- URL: https://www.sqlite.org/2026/sqlite-amalgamation-3540100.zip
- Upstream SHA3-256: `92281c86e2957f9b9305890f9ac04204caa7b677cc868dc326d942689df28e30`
- SHA-256: `0a9c8e34352dad46006aa82836d5383bff9dc6bc9ce5a60553c05aa30e6ad1f9`
- Unchanged `sqlite3.c`, renamed `native/sqlite.c`: SHA-256 `0c657666d819f261cc4007526ad57e1669de9949df7a8ea847aa284a2bf0ee52`
- Unchanged `sqlite3.h`, renamed `native/sqlite.h`: SHA-256 `d6c84557261893439b341d2e6c35187ae3ea24fbfd843ad94616f72d89242b8c`

The registered static C11 recipe adds `SQLITE_THREADSAFE=1`,
`SQLITE_OMIT_LOAD_EXTENSION`, `SQLITE_DQS=0`, `SQLITE_DEFAULT_MEMSTATUS=0`,
`SQLITE_MAX_LENGTH=16777216`, `SQLITE_MAX_SQL_LENGTH=1048576`, and
`_DEFAULT_SOURCE` to the shared warning/optimization flags. No extension loading,
SQL callbacks into Zore or arbitrary library linking is available. SQLite may
use its ordinary OS VFS and host C runtime; Rust's host runtime supplies the
standard threading/system link closure. The recipe, flags, symbols and all four
package-owned native files contribute to the approved fingerprint and locks.

Supported hosts are Linux GNU/macOS x86_64/aarch64 and Windows x86_64 MSVC using
[the normal toolchains](../platform-support.md). Unsupported hosts fail explicitly;
cross-compilation is excluded. `check` validates source without executing a native
compiler/linker. Build/run/test compile validated snapshots and statically link
SQLite. Native objects are temporary; selected recipes separate runtime caches.

Compared with Adler-32 this provider adds only a registry entry, adapter flags,
and registered helper waits for its blocking wrapper calls. Connection leases,
statement finalization, row copying, transaction rollback and error translation
live in the package-owned reviewed bridge and ordinary Zore wrapper.

## Persistence example

```sh
zore install --offline examples/sqlite-library/app
zore check --locked --offline examples/sqlite-library/app/main.ore
zore run --locked --offline examples/sqlite-library/app/main.ore
zore install --offline examples/sqlite-library/sqlite
zore test --locked --offline examples/sqlite-library/sqlite
```

The app inserts a parameterized row in `notes.db` and queries its stored text.
Driver tests create a separate tagged Git provider repository, install its release,
remove the source remote, and run locked/offline check/build/test from the cache.
The same host-independent tests are selected by the Linux/macOS/Windows CI matrix;
local verification alone does not establish results for hosts it did not run on.
