use super::*;

const CHECK: &str = r#"
func check(ok bool, label string) {
    if !ok { panic(label) }
}

func fails(err error, message string) bool {
    return err != nil && errors.Message(err) == message
}

func failsWith(err error, prefix string) bool {
    return err != nil && strings.HasPrefix(errors.Message(err), prefix)
}

func text(path string) string {
    let data, err = os.ReadFile(path)
    if err != nil { return "<" + errors.Message(err) + ">" }
    let value, _ = strings.FromBytes(data[:])
    return value
}
"#;

fn prints_in(source: &str, expected: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    let source = source.replace("DIR", &dir.path().to_string_lossy().replace('\\', "/"));
    prints(&source, expected);
    dir
}

#[test]
fn open_file_flags_choose_access_creation_and_truncation() {
    let source = format!(
        r#"package main
import "std/errors"
import "std/os"
import "std/strings"
{CHECK}
func rejected(flags int, message string) {{
    let file, err = os.OpenFile("DIR/never", flags, 0o644)
    check(fails(err, message), message)
    check(file.Name() == "", "failed open is closed")
    check(fails(file.Sync(), "os.Sync: not an open file"), "failed open has no handle")
}}

func main() {{
    let path = "DIR/data.txt"
    rejected(64, "os.OpenFile: unknown flag")
    rejected(-1, "os.OpenFile: unknown flag")
    rejected(3, "os.OpenFile: more than one access mode")
    let needWrite = "os.OpenFile: append, create, exclusive, and truncate need write access"
    rejected(os.OpenReadOnly | os.OpenCreate, needWrite)
    rejected(os.OpenReadOnly | os.OpenAppend, needWrite)
    rejected(os.OpenReadOnly | os.OpenTruncate, needWrite)
    rejected(os.OpenReadOnly | os.OpenExclusive | os.OpenCreate, needWrite)
    rejected(os.OpenWriteOnly | os.OpenExclusive, "os.OpenFile: exclusive needs create")
    rejected(os.OpenWriteOnly | os.OpenAppend | os.OpenTruncate, "os.OpenFile: append and truncate together")
    let _, statErr = os.Stat("DIR/never")
    check(statErr != nil, "rejected flags touch nothing")

    let _, missing = os.OpenFile(path, os.OpenWriteOnly, 0o644)
    check(failsWith(missing, "os.OpenFile: "), "missing without create")

    let created, createErr = os.OpenFile(path, os.OpenWriteOnly | os.OpenCreate | os.OpenExclusive, 0o640)
    check(createErr == nil, "exclusive create")
    check(created.Name() == path, "name")
    let _, _ = created.WriteString("hello world")
    var buf = Array<byte>{{0, 0}}
    let _, readErr = created.Read(buf[:])
    check(failsWith(readErr, "os.Read: "), "write-only cannot read")
    _ = created.Close()

    let _, again = os.OpenFile(path, os.OpenReadWrite | os.OpenCreate | os.OpenExclusive, 0o640)
    check(failsWith(again, "os.OpenFile: "), "exclusive refuses an existing file")
    check(text(path) == "hello world", "refused open keeps contents")

    let reader, _ = os.OpenFile(path, os.OpenReadOnly, 0)
    let _, writeErr = reader.WriteString("x")
    check(failsWith(writeErr, "os.Write: "), "read-only cannot write")
    drop(reader)

    let plain, _ = os.OpenFile(path, os.OpenWriteOnly, 0)
    let _, _ = plain.WriteString("HELLO")
    drop(plain)
    check(text(path) == "HELLO world", "no truncate overwrites in place")

    let appender, _ = os.OpenFile(path, os.OpenReadWrite | os.OpenAppend, 0)
    let start, _ = appender.Seek(0, 0)
    check(start == 0, "append can seek")
    var first = Array<byte>{{0}}
    let _, _ = appender.Read(first[:])
    check(first[0] == 72, "append reads from the sought position")
    let _, _ = appender.WriteString("!")
    let after, _ = appender.Seek(0, 1)
    drop(appender)
    check(text(path) == "HELLO world!", "append writes at the end after a seek")
    check(after == 12, "a write leaves the position at the end")

    let truncated, _ = os.OpenFile(path, os.OpenWriteOnly | os.OpenTruncate, 0)
    drop(truncated)
    check(text(path) == "", "truncate empties")

    let made, _ = os.Create("DIR/made.txt")
    check(made.Name() == "DIR/made.txt", "Create keeps the name")
    drop(made)
    check(os.Stdin().Name() == "" && os.Stdout().Name() == "", "standard streams have no name")
    let _, openErr = os.Open("DIR/absent")
    check(failsWith(openErr, "os.Open: "), "Open keeps its prefix")
    let _, createMissing = os.Create("DIR/absent/x")
    check(failsWith(createMissing, "os.Create: "), "Create keeps its prefix")
    println("ok")
}}
"#
    );
    prints_in(&source, "ok\n");
}

#[cfg(unix)]
#[test]
fn open_file_and_temporary_storage_use_their_permission_bits() {
    let source = format!(
        r#"package main
import "std/errors"
import "std/os"
import "std/strings"
{CHECK}
func main() {{
    let created, _ = os.OpenFile("DIR/private", os.OpenWriteOnly | os.OpenCreate, 0o600)
    drop(created)
    let info, _ = os.Stat("DIR/private")
    check(info.Mode() == 0o600, "OpenFile perm")
    let temp, _ = os.CreateTemp("DIR", "t-*")
    let tempInfo, _ = os.Stat(temp.Name())
    check(tempInfo.Mode() == 0o600, "CreateTemp perm")
    let folder, _ = os.MkdirTemp("DIR", "d-*")
    let folderInfo, _ = os.Stat(folder)
    check(folderInfo.Mode() == 0o700 && folderInfo.IsDir(), "MkdirTemp perm")
    println("ok")
}}
"#
    );
    prints_in(&source, "ok\n");
}

#[test]
fn seek_moves_reads_and_writes_and_sync_reports_success() {
    let source = format!(
        r#"package main
import "std/errors"
import "std/io"
import "std/os"
import "std/strings"
{CHECK}
func rewind(s mut io.Seeker) int {{
    let position, _ = s.Seek(0, io.SeekStart)
    return position
}}

func main() {{
    var file, _ = os.Create("DIR/seek.bin")
    let _, _ = file.WriteString("0123456789")
    let here, _ = file.Seek(0, io.SeekCurrent)
    check(here == 10, "current position")
    let size, _ = file.Seek(0, io.SeekEnd)
    check(size == 10, "size from the end")
    let back, _ = file.Seek(-4, io.SeekEnd)
    check(back == 6, "back from the end")
    var four = Array<byte>{{0, 0, 0, 0}}
    let n, _ = file.Read(four[:])
    let got, _ = strings.FromBytes(four[:n])
    check(got == "6789", "read after seek")
    let forward, _ = file.Seek(-8, io.SeekCurrent)
    check(forward == 2, "relative seek")

    let kept, negative = file.Seek(-1, io.SeekStart)
    check(failsWith(negative, "os.Seek: ") && kept == 0, "negative start")
    let _, before = file.Seek(-11, io.SeekEnd)
    check(failsWith(before, "os.Seek: "), "before the start")
    let still, _ = file.Seek(0, io.SeekCurrent)
    check(still == 2, "failed seek keeps the position")
    let _, whence = file.Seek(0, 3)
    check(fails(whence, "os.Seek: invalid whence"), "whence 3")
    let _, low = file.Seek(0, -1)
    check(fails(low, "os.Seek: invalid whence"), "whence -1")

    let far, _ = file.Seek(12, io.SeekStart)
    check(far == 12, "past the end")
    var one = Array<byte>{{0}}
    let none, eof = file.Read(one[:])
    check(eof == io.EOF && none == 0, "EOF past the end")
    let _, _ = file.WriteString("!")
    check(rewind(file) == 0, "io.Seeker")
    check(file.Sync() == nil, "Sync")
    let data, _ = os.ReadFile("DIR/seek.bin")
    check(data.len() == 13 && data[10] == 0 && data[11] == 0 && data[12] == 33, "gap filled with zeros")
    _ = file.Close()

    let _, stdinSeek = os.Stdin().Seek(0, io.SeekStart)
    check(fails(stdinSeek, "os.Seek: not a seekable file"), "stdin")
    check(os.Stdout().Sync() == nil && os.Stderr().Sync() == nil && os.Stdin().Sync() == nil, "standard Sync")
    println("ok")
}}
"#
    );
    prints_in(&source, "ok\n");
}

#[test]
fn temporary_files_and_folders_are_new_owned_and_never_reused() {
    let source = format!(
        r#"package main
import "std/errors"
import "std/os"
import "std/path/filepath"
import "std/strings"
{CHECK}
func randomPart(name string, prefix string, suffix string) bool {{
    if !strings.HasPrefix(name, prefix) || !strings.HasSuffix(name, suffix) {{ return false }}
    let middle = name[prefix.len():name.len() - suffix.len()]
    if middle.len() != 12 {{ return false }}
    for var i = 0; i < middle.len(); i += 1 {{
        let c = middle[i]
        if !(c >= 48 && c <= 57) && !(c >= 97 && c <= 118) {{ return false }}
    }}
    return true
}}

func main() {{
    check(os.TempDir().len() > 0, "TempDir")
    var file, err = os.CreateTemp("DIR", "report-*.csv")
    check(err == nil, "CreateTemp")
    check(filepath.Dir(file.Name()) == filepath.Clean("DIR"), "in dir")
    check(randomPart(filepath.Base(file.Name()), "report-", ".csv"), "pattern with star")
    let _, _ = file.WriteString("a,b")
    let seek, _ = file.Seek(0, 0)
    var two = Array<byte>{{0, 0, 0}}
    let n, _ = file.Read(two[:])
    check(seek == 0 && n == 3, "opened for reading and writing")
    let name = file.Name()
    drop(file)
    check(text(name) == "a,b", "drop keeps the file")

    let plain, _ = os.CreateTemp("DIR", "plain")
    check(randomPart(filepath.Base(plain.Name()), "plain", ""), "pattern without star")
    let last, _ = os.CreateTemp("DIR", "a*b*c")
    check(randomPart(filepath.Base(last.Name()), "a*b", "c"), "last star")
    let bare, _ = os.CreateTemp("DIR", "")
    check(randomPart(filepath.Base(bare.Name()), "", ""), "empty pattern")
    check(plain.Name() != last.Name(), "distinct names")

    let inTemp, tempErr = os.CreateTemp("", "zore-test-*")
    check(tempErr == nil, "default folder")
    check(filepath.Dir(inTemp.Name()) == filepath.Clean(os.TempDir()), "TempDir used")
    let tempName = inTemp.Name()
    _ = inTemp.Close()
    check(os.Remove(tempName) == nil, "caller removes")

    let _, sep = os.CreateTemp("DIR", "a/b*")
    check(fails(sep, "os.CreateTemp: pattern contains a path separator"), "separator")
    let _, dirSep = os.MkdirTemp("DIR", "x/*")
    check(fails(dirSep, "os.MkdirTemp: pattern contains a path separator"), "folder separator")
    let _, missing = os.CreateTemp("DIR/absent", "x-*")
    check(failsWith(missing, "os.CreateTemp: "), "missing folder")
    let missingDir, missingDirErr = os.MkdirTemp("DIR/absent", "x-*")
    check(failsWith(missingDirErr, "os.MkdirTemp: ") && missingDir == "", "missing folder for MkdirTemp")

    let folder, folderErr = os.MkdirTemp("DIR", "work-*")
    check(folderErr == nil && randomPart(filepath.Base(folder), "work-", ""), "MkdirTemp")
    let info, _ = os.Stat(folder)
    check(info.IsDir(), "a folder")
    let entries, _ = os.ReadDir(folder)
    check(entries.len() == 0, "empty")
    check(os.RemoveAll(folder) == nil, "removed by the caller")
    println("ok")
}}
"#
    );
    prints_in(&source, "ok\n");
}

#[test]
fn concurrent_temporary_files_never_collide_in_plain_and_async_code() {
    let source = format!(
        r#"package main
import "std/errors"
import "std/os"
import "std/strings"
import "std/sync"
{CHECK}
func make(dir string, wg sync.WaitGroup, failures Mutex<int>) {{
    for var i = 0; i < 5; i += 1 {{
        let file, err = os.CreateTemp(dir, "c-*")
        if err != nil {{ failures.withLock(func(n mut int) {{ n += 1 }}) }}
        let _, _ = file.WriteString(file.Name())
    }}
    let folder, err = os.MkdirTemp(dir, "c-*")
    if err != nil || folder == "" {{ failures.withLock(func(n mut int) {{ n += 1 }}) }}
    wg.Done()
}}

async func makeAsync(dir string) (string, error) {{
    var file = os.CreateTemp(dir, "async-*")?
    let _ = file.WriteString("async")?
    check(file.Sync() == nil, "async Sync")
    let position = file.Seek(0, 0)?
    check(position == 0, "async Seek")
    return file.Name(), nil
}}

func main() {{
    let dir = "DIR/many"
    check(os.MkdirAll(dir, 0o755) == nil, "folder")
    let wg = sync.NewWaitGroup()
    let failures = mutex(0)
    for var t = 0; t < 40; t += 1 {{
        wg.Add(1)
        go make(dir, wg, failures)
    }}
    wg.Wait()
    failures.withLock(func(n mut int) {{ check(n == 0, "no failures") }})
    let entries, _ = os.ReadDir(dir)
    check(entries.len() == 240, "every file and folder is new")
    for entry in entries {{
        if !entry.IsDir() {{
            let path = dir + "/" + entry.Name()
            check(text(path) == path, "each file written once by its creator")
        }}
    }}
    let task = go makeAsync(dir)
    let name, err = task.wait()
    check(err == nil && text(name) == "async", "async")
    check(os.RemoveAll(dir) == nil, "cleanup")
    println("ok")
}}
"#
    );
    prints_in(&source, "ok\n");
}

#[test]
fn rename_replaces_files_and_refuses_unsafe_folder_moves() {
    let source = format!(
        r#"package main
import "std/errors"
import "std/os"
import "std/strings"
{CHECK}
func save(path string, data string) error {{
    var file = os.CreateTemp("DIR", "save-*")?
    let name = file.Name()
    let _, writeErr = file.WriteString(data)
    var failure = writeErr
    if failure == nil {{ failure = file.Sync() }}
    _ = file.Close()
    if failure != nil {{
        _ = os.Remove(name)
        return failure
    }}
    return os.Rename(name, path)
}}

func main() {{
    check(os.WriteFile("DIR/target", strings.Bytes("old")[:], 0o644) == nil, "old")
    check(save("DIR/target", "new") == nil, "save")
    check(text("DIR/target") == "new", "replaced")
    let entries, _ = os.ReadDir("DIR")
    check(entries.len() == 1, "no temporary file left")

    check(failsWith(os.Rename("DIR/absent", "DIR/other"), "os.Rename: "), "missing source")
    check(text("DIR/target") == "new", "failed rename keeps target")

    check(os.MkdirAll("DIR/full/inner", 0o755) == nil && os.MkdirAll("DIR/src/sub", 0o755) == nil, "folders")
    check(failsWith(os.Rename("DIR/src", "DIR/full"), "os.Rename: "), "folder onto non-empty folder")
    check(failsWith(os.Rename("DIR/target", "DIR/full"), "os.Rename: "), "file onto folder")
    check(failsWith(os.Rename("DIR/src", "DIR/target"), "os.Rename: "), "folder onto file")
    check(failsWith(os.Rename("DIR/src", "DIR/src/sub/inside"), "os.Rename: "), "folder into itself")
    check(os.Rename("DIR/src", "DIR/moved") == nil, "folder to a new name")
    let info, _ = os.Stat("DIR/moved/sub")
    check(info.IsDir(), "contents moved")
    println("ok")
}}
"#
    );
    prints_in(&source, "ok\n");
}

#[cfg(unix)]
#[test]
fn rename_onto_an_empty_folder_and_an_open_file_on_unix() {
    let source = format!(
        r#"package main
import "std/errors"
import "std/os"
import "std/strings"
{CHECK}
func main() {{
    check(os.MkdirAll("DIR/a/x", 0o755) == nil && os.MkdirAll("DIR/b", 0o755) == nil, "folders")
    check(os.Rename("DIR/a", "DIR/b") == nil, "folder onto an empty folder")
    let info, _ = os.Stat("DIR/b/x")
    check(info.IsDir(), "replaced")

    check(os.WriteFile("DIR/f", strings.Bytes("old")[:], 0o644) == nil, "old")
    check(os.WriteFile("DIR/g", strings.Bytes("new")[:], 0o644) == nil, "new")
    let reader, _ = os.Open("DIR/f")
    check(os.Rename("DIR/g", "DIR/f") == nil, "replace an open file")
    var buf = Array<byte>{{0, 0, 0}}
    let _, _ = reader.Read(buf[:])
    let held, _ = strings.FromBytes(buf[:])
    check(held == "old" && text("DIR/f") == "new", "open handle keeps the old file")
    println("ok")
}}
"#
    );
    prints_in(&source, "ok\n");
}

#[test]
fn walk_visits_depth_first_in_name_order_and_skips_on_request() {
    let source = format!(
        r#"package main
import "std/errors"
import "std/os"
import "std/path/filepath"
import "std/strings"
{CHECK}
func main() {{
    let root = "DIR/tree"
    check(os.MkdirAll(root + "/b/c", 0o755) == nil && os.MkdirAll(root + "/a", 0o755) == nil, "folders")
    check(os.MkdirAll(root + "/skip/deep", 0o755) == nil, "skipped folder")
    check(os.WriteFile(root + "/b/c/2", strings.Bytes("")[:], 0o644) == nil, "file")
    check(os.WriteFile(root + "/b/x", strings.Bytes("")[:], 0o644) == nil, "file")
    check(os.WriteFile(root + "/top", strings.Bytes("")[:], 0o644) == nil, "file")
    check(os.WriteFile(root + "/skip/deep/hidden", strings.Bytes("")[:], 0o644) == nil, "file")
    var walk = filepath.Walk(root)
    check(walk.Path() == "", "nothing before Next")
    for walk.Next() {{
        check(walk.Err() == nil, "no errors")
        let rel = walk.Path()[root.len():]
        if walk.IsDir() {{ println(rel + "/") }} else {{ println(rel) }}
        if filepath.Base(walk.Path()) == "skip" {{ walk.SkipDir() }}
        if filepath.Base(walk.Path()) == "top" {{ walk.SkipDir() }}
    }}
    check(walk.Path() == "" && !walk.IsDir() && walk.Err() == nil, "cleared at the end")
    check(!walk.Next(), "stays finished")

    var single = filepath.Walk(root + "/top")
    check(single.Next() && single.Path() == root + "/top" && !single.IsDir(), "file root")
    check(!single.Next(), "file root has nothing below")

    var missing = filepath.Walk(root + "/absent")
    check(missing.Next() && missing.Path() == root + "/absent", "missing root visited")
    check(failsWith(missing.Err(), "os.Lstat: ") && !missing.IsDir(), "missing root error")
    check(!missing.Next(), "missing root ends")

    var changing = filepath.Walk(root)
    var errorsSeen = 0
    for changing.Next() {{
        if changing.Path() == root + "/a" {{ check(os.RemoveAll(root + "/b") == nil, "remove b") }}
        if changing.Err() != nil {{
            errorsSeen += 1
            check(changing.Path() == root + "/b" && changing.IsDir(), "error on the removed folder")
            check(failsWith(changing.Err(), "os.ReadDir: "), "read error")
        }}
    }}
    check(errorsSeen == 1, "one error, walk continued")
    println("ok")
}}
"#
    );
    prints_in(
        &source,
        "/\n/a/\n/b/\n/b/c/\n/b/c/2\n/b/x\n/skip/\n/top\nok\n",
    );
}

#[cfg(unix)]
#[test]
fn walk_and_lstat_report_links_without_following_them() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("tree");
    std::fs::create_dir_all(root.join("real/inside")).unwrap();
    std::fs::write(root.join("real/file"), "x").unwrap();
    std::os::unix::fs::symlink("../real", root.join("real/inside/loop")).unwrap();
    std::os::unix::fs::symlink("real", root.join("link")).unwrap();
    std::os::unix::fs::symlink("nowhere", root.join("broken")).unwrap();
    let source = format!(
        r#"package main
import "std/errors"
import "std/os"
import "std/path/filepath"
import "std/strings"
{CHECK}
func main() {{
    let root = "DIR/tree"
    var walk = filepath.Walk(root)
    for walk.Next() {{
        check(walk.Err() == nil, "no errors")
        var mark = ""
        if walk.IsDir() {{ mark = "/" }}
        if walk.IsSymlink() {{ mark = "@" }}
        println(walk.Path()[root.len():] + mark)
    }}
    var fromLink = filepath.Walk(root + "/link")
    check(fromLink.Next() && fromLink.IsSymlink() && !fromLink.IsDir(), "link root")
    check(!fromLink.Next(), "link root not entered")

    let entries, _ = os.ReadDir(root)
    for entry in entries {{
        if entry.Name() == "link" {{ check(entry.IsSymlink() && !entry.IsDir(), "entry link") }}
        if entry.Name() == "real" {{ check(!entry.IsSymlink() && entry.IsDir(), "entry folder") }}
    }}
    let linkInfo, linkErr = os.Lstat(root + "/link")
    check(linkErr == nil && linkInfo.IsSymlink() && !linkInfo.IsDir() && linkInfo.Name() == "link", "Lstat link")
    let followed, _ = os.Stat(root + "/link")
    check(followed.IsDir() && !followed.IsSymlink(), "Stat follows")
    let broken, brokenErr = os.Lstat(root + "/broken")
    check(brokenErr == nil && broken.IsSymlink(), "Lstat broken link")
    let _, statBroken = os.Stat(root + "/broken")
    check(failsWith(statBroken, "os.Stat: "), "Stat broken link")
    let _, absent = os.Lstat(root + "/absent")
    check(failsWith(absent, "os.Lstat: "), "Lstat missing")
    println("ok")
}}
"#
    )
    .replace("DIR", &dir.path().to_string_lossy());
    prints(
        &source,
        "/\n/broken@\n/link@\n/real/\n/real/file\n/real/inside/\n/real/inside/loop@\nok\n",
    );
}

#[test]
fn walk_works_inside_async_functions() {
    let source = format!(
        r#"package main
import "std/errors"
import "std/os"
import "std/path/filepath"
import "std/strings"
{CHECK}
async func count(root string) (int, error) {{
    var files = 0
    var walk = filepath.Walk(root)
    for walk.Next() {{
        if walk.Err() != nil {{ return files, walk.Err() }}
        if !walk.IsDir() {{ files += 1 }}
    }}
    return files, nil
}}

func main() {{
    check(os.MkdirAll("DIR/w/x/y", 0o755) == nil, "folders")
    for name in Array<string>{{"DIR/w/1", "DIR/w/x/2", "DIR/w/x/y/3"}} {{
        check(os.WriteFile(name, strings.Bytes(name)[:], 0o644) == nil, name)
    }}
    var tasks = Array<Task<int, error>>{{}}
    for var i = 0; i < 8; i += 1 {{ tasks.push(go count("DIR/w")) }}
    for tasks.len() > 0 {{
        let _, task = tasks.pop()
        let n, err = task.wait()
        check(err == nil && n == 3, "three files")
    }}
    let failing = go count("DIR/none")
    let _, missing = failing.wait()
    check(failsWith(missing, "os.Lstat: "), "missing root")
    println("ok")
}}
"#
    );
    prints_in(&source, "ok\n");
}
