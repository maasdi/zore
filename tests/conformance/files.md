# File handles, temporary storage, renames, and folder walks

These cases cover spec §37.15. Native cases are in `tests/codegen/files.rs`;
checking cases are in `tests/packages/packages.rs`. Cases marked "Linux and
macOS" run only there.

## Opening files

| Scenario | Expected result |
| --- | --- |
| Flags `64` and `-1` | `os.OpenFile: unknown flag`; nothing is created |
| Flags `3` | `os.OpenFile: more than one access mode` |
| `OpenReadOnly` with `OpenCreate`, `OpenAppend`, `OpenTruncate`, or `OpenExclusive \| OpenCreate` | `os.OpenFile: append, create, exclusive, and truncate need write access` |
| `OpenExclusive` without `OpenCreate` | `os.OpenFile: exclusive needs create` |
| `OpenAppend \| OpenTruncate` | `os.OpenFile: append and truncate together` |
| A rejected or failed open | A closed `File`: `Name()` is `""` and `Sync()` gives `os.Sync: not an open file` |
| `OpenWriteOnly` on a missing path | An error beginning `os.OpenFile: ` |
| `OpenCreate \| OpenExclusive` on a new path, then again | Created and named after the path; the second fails and leaves the contents alone |
| Reading a write-only file, writing a read-only one | Errors beginning `os.Read: ` and `os.Write: ` |
| `OpenWriteOnly` without truncate | Writes over the start and keeps the rest |
| `OpenReadWrite \| OpenAppend`, `Seek(0, 0)`, write, read | The write lands at the end; the read starts at the sought position |
| `OpenTruncate` | The file is empty |
| `OpenCreate` with `0o600` (Linux and macOS) | `Mode()` is `0o600` |
| `Open` and `Create` of missing paths | Errors still begin `os.Open: ` and `os.Create: ` |
| `Stdin().Name()`, `Stdout().Name()` | `""` |

## Position and storage

| Scenario | Expected result |
| --- | --- |
| Ten bytes written, `Seek(0, SeekCurrent)`, `Seek(0, SeekEnd)`, `Seek(-4, SeekEnd)` then read | `10`, `10`, `6`, then `6789` |
| `Seek(-8, SeekCurrent)` | `2` |
| `Seek(-1, SeekStart)`, `Seek(-11, SeekEnd)` | Errors beginning `os.Seek: `; the position stays `2` |
| Whence `3` and `-1` | `os.Seek: invalid whence` |
| `Seek(12, SeekStart)` on ten bytes, read, write `!` | `0` and `EOF`; the file is 13 bytes with zeros at 10 and 11 |
| `os.File` passed as `mut io.Seeker` | Its `Seek` is called |
| `Sync` on a written file, `Stdout`, `Stderr`, `Stdin` | `nil` |
| `Stdin().Seek(0, SeekStart)` | `os.Seek: not a seekable file` |
| `Seek` and `Sync` in an `async func` task | Same results |

## Temporary files and folders

| Scenario | Expected result |
| --- | --- |
| `CreateTemp(dir, "report-*.csv")` | A new file in `dir` named `report-`, twelve characters from `0-9a-v`, `.csv`; opened for reading and writing |
| Patterns `plain`, `a*b*c`, and `""` | Characters added at the end; the last `*` replaced; a twelve-character name |
| Dropping the file | The file and its contents remain |
| `CreateTemp("", ...)` | The file is in `TempDir()`; the caller removes it |
| Patterns `a/b*` and `x/*` | `os.CreateTemp: pattern contains a path separator`, `os.MkdirTemp: pattern contains a path separator` |
| A missing `dir` | Errors beginning `os.CreateTemp: ` and `os.MkdirTemp: `; `MkdirTemp` returns `""` |
| `MkdirTemp(dir, "work-*")` | An empty new folder; `RemoveAll` removes it |
| Permission bits (Linux and macOS) | `0o600` for files and `0o700` for folders |
| 40 tasks each creating five files and one folder in one folder | 240 distinct entries; each file holds only what its creator wrote |

## Renames

| Scenario | Expected result |
| --- | --- |
| Temporary file in the same folder, `Sync`, `Close`, `Rename` onto an existing file | The target holds the new contents; no temporary file remains |
| A missing source | An error beginning `os.Rename: `; the target is unchanged |
| A folder onto a non-empty folder, a file onto a folder, a folder onto a file, a folder into itself | Errors beginning `os.Rename: ` |
| A folder to a new name | Its contents move with it |
| A folder onto an empty folder (Linux and macOS) | Replaced |
| Replacing a file that is open for reading (Linux and macOS) | The open file still reads the old contents; the path has the new ones |

## Folder walks

| Scenario | Expected result |
| --- | --- |
| A tree with folders `a`, `b/c`, `skip/deep` and files | Root first, then depth-first in name order |
| `SkipDir` on `skip`, and on the file `top` | Nothing below `skip`; no effect for the file |
| `Path()` before the first `Next`, and after the last | `""`; then `""`, `false`, `nil`, and `Next` stays `false` |
| A file as root | Only the file |
| A missing root | One entry: the root with an error beginning `os.Lstat: ` and `IsDir()` false |
| Folder `b` removed while `a` is visited | `b` is visited with `IsDir()` true and an error beginning `os.ReadDir: `; the walk continues |
| Links to a folder, a loop of links, and a broken link (Linux and macOS) | Visited with `IsSymlink()` true and never entered |
| A link as root (Linux and macOS) | Visited and not entered |
| Eight tasks counting files with an `async func` | Each counts three |

## Metadata

| Scenario | Expected result |
| --- | --- |
| `ReadDir` entries for a link and a folder (Linux and macOS) | `IsSymlink()` true and `IsDir()` false for the link; the reverse for the folder |
| `Lstat` of a link, `Stat` of the same link | The link's own facts with `IsSymlink()`; the folder, with `IsSymlink()` false |
| `Lstat` and `Stat` of a broken link | Facts with `IsSymlink()`; an error beginning `os.Stat: ` |
| `Lstat` of a missing path | An error beginning `os.Lstat: ` |

## Checking

| Program | Result |
| --- | --- |
| `os.OpenFile("a", os.OpenReadOnly)` | Rejected: three arguments |
| `let walk = filepath.Walk(".")` then `walk.Next()` | Rejected: `Next` needs a mutable walker |
| `let position = file.Seek(0, io.SeekStart)` | Rejected: two results |
| `os.CreateTemp("", "x")` as a statement | Rejected: the results must be used |
| `filepath.Walker{root: "."}` | Rejected: fields are not exported |
