use super::*;

#[test]
fn native_cli_build_run_and_test_accept_paths_with_spaces_and_unicode() {
    let directory = TempDir::new().unwrap();
    let project = directory.path().join("project café with spaces");
    let cache = directory.path().join("cache with spaces");
    std::fs::create_dir_all(&project).unwrap();
    let source = project.join("hello world.ore");
    std::fs::write(
        &source,
        "package main\nfunc main() { println(\"héllo\") }\n",
    )
    .unwrap();
    for action in ["build", "run"] {
        let output = zore()
            .arg(action)
            .arg(&source)
            .current_dir(&project)
            .env("ZORE_CACHE_DIR", &cache)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
        assert_eq!(
            stdout(&output),
            if action == "run" { "héllo\n" } else { "" }
        );
    }
    let executable = project.join(format!("hello world{}", std::env::consts::EXE_SUFFIX));
    assert!(executable.is_file());
    assert_eq!(
        Command::new(&executable).output().unwrap().stdout,
        "héllo\n".as_bytes()
    );
    std::fs::write(
        project.join("hello_test.ore"),
        "package main\nfunc TestNative() error { return nil }\n",
    )
    .unwrap();
    let output = zore()
        .arg("test")
        .arg(&project)
        .env("ZORE_CACHE_DIR", &cache)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).ends_with("Tests: 1 passed, 0 failed\n"));
}

#[test]
fn native_processes_preserve_arguments_bytes_working_directory_and_exit_status() {
    let directory = TempDir::new().unwrap();
    let folder = directory.path().join("process café with spaces");
    std::fs::create_dir_all(&folder).unwrap();
    let helper = folder.join(format!("helper program{}", std::env::consts::EXE_SUFFIX));
    build_file(r#"package main
import "std/os"
func main() {
    let args = os.Args()
    if args.len() != 5 || args[2] != "a b" || args[3] != "\"quoted\"" || args[4] != "C:\\tmp\\end\\" { panic("arguments changed") }
    let cwd, err = os.Getwd()
    if err != nil || cwd != os.Getenv("ZORE_PROCESS_DIR") { panic("working directory changed") }
    if args[1] == "bytes" {
        let _, _ = os.Stdout().Write(Array<byte>{0, 255, 128}[:])
    } else {
        let _, _ = os.Stdout().WriteString("out")
        let _, _ = os.Stderr().WriteString("err")
        os.Exit(7)
    }
}
"#, &helper).unwrap();
    let source = format!(
        r#"package main
import "std/os/exec"
import "std/strings"
func main() {{
    let args = Array<string>{{"bytes", "a b", "\"quoted\"", "C:\\tmp\\end\\"}}
    var bytesCmd = exec.Command({helper:?}, args[:])
    bytesCmd.Dir = {folder:?}
    let bytes, err = bytesCmd.Output()
    if err != nil {{ panic("child failed") }}
    for octet in bytes {{ println(octet) }}
    var combined = exec.Command({helper:?}, Array<string>{{"combined", "a b", "\"quoted\"", "C:\\tmp\\end\\"}}[:])
    combined.Dir = {folder:?}
    let captured, failure = combined.CombinedOutput()
    let text, _ = strings.FromBytes(captured[:])
    println(text == "outerr")
    println(failure == error("exit status 7"))
    println(combined.Run() == error("exit status 7"))
}}
"#,
        helper = helper.to_str().unwrap(),
        folder = folder.to_str().unwrap()
    );
    let executable = directory.executable("parent");
    build_file(&source, &executable).unwrap();
    let output = Command::new(executable)
        .env(
            "ZORE_PROCESS_DIR",
            if cfg!(unix) {
                std::fs::canonicalize(&folder).unwrap()
            } else {
                folder
            },
        )
        .env("ZORE_CHECK_LEAKS", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "0\n255\n128\ntrue\ntrue\ntrue\n");
}

#[test]
fn native_files_preserve_bytes_and_release_handles_before_removal() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("bytes café with spaces.bin");
    let source = format!(
        r#"package main
import "std/os"
func write(path string) error {{
    let file = os.Create(path)?
    let _, err = file.Write(Array<byte>{{0, 255, 10, 128}}[:])
    return err
}}
func main() {{
    let path = {path:?}
    if write(path) != nil {{ panic("write") }}
    let data, err = os.ReadFile(path)
    if err != nil {{ panic("read") }}
    for octet in data {{ println(octet) }}
    let info, statErr = os.Stat(path)
    if statErr != nil {{ panic("stat") }}
    println(info.Name() == "bytes café with spaces.bin")
    println(os.Remove(path) == nil)
}}
"#,
        path = path.to_str().unwrap()
    );
    prints(&source, "0\n255\n10\n128\ntrue\ntrue\n");
    assert!(!path.exists());
}

#[cfg(windows)]
#[test]
fn native_cli_preserves_full_windows_exit_statuses() {
    let directory = TempDir::new().unwrap();
    let source = directory.path().join("main.ore");
    for code in [4660, -1073741819] {
        std::fs::write(
            &source,
            format!("package main\nimport \"std/os\"\nfunc main() {{ os.Exit({code}) }}\n"),
        )
        .unwrap();
        let output = zore().arg("run").arg(&source).output().unwrap();
        assert_eq!(output.status.code(), Some(code), "{output:?}");
    }
}

fn windows_path_source(main: &str) -> String {
    let implementation = include_str!("../../std/path/filepath/windows.ore")
        .replacen("package filepath", "package main", 1)
        .replace(
            "func absolute(p string) (string, error)",
            "func absolute(p string) (string, error) { return p, nil }",
        );
    format!("{implementation}\nfunc main() {{\n{main}\n}}\n")
}

#[test]
fn windows_path_rules_preserve_volumes_and_handle_both_separators() {
    prints(
        &windows_path_source(
            r#"
    for p in Array<string>{"", "C:", "C:foo/../bar", "C:/a/../../b", "\\\\host\\share\\a\\..\\..\\x", "\\foo\\..\\bar"} { println(Clean(p)) }
    println(Join(Array<string>{"C:", "notes.txt"}[:]))
    println(Clean("a/../C:notes.txt"))
    println(Clean("\\\\host\\share\\..\\"))
    println(Join(Array<string>{"C:/work", "../notes.txt"}[:]))
    let dir, file = Split("C:notes.txt")
    println(dir + "|" + file)
    println(Base("C:/work/a.txt"))
    println(Dir("C:/work/a.txt"))
    println(Ext("\\\\host\\share.name\\dir\\a.txt"))
    for p in Array<string>{"C:notes.txt", "\\notes.txt", "C:/notes.txt", "\\\\host\\share", "\\\\host"} { println(IsAbs(p)) }
    println(Separator == '\\')
    println(ListSeparator == ';')
"#,
        ),
        ".\nC:.\nC:bar\nC:\\b\n\\\\host\\share\\x\n\\bar\nC:notes.txt\n.\\C:notes.txt\n\\\\host\\share\nC:\\notes.txt\nC:|notes.txt\na.txt\nC:\\work\n.txt\nfalse\nfalse\ntrue\ntrue\nfalse\ntrue\ntrue\n",
    );
}

#[test]
fn windows_path_namespaces_receive_an_explicit_diagnostic() {
    for prefix in [r#"\\\\?\\C:\\file"#, r#"\\\\.\\device"#] {
        let output = run(&windows_path_source(&format!(
            "println(Clean(\"{prefix}\"))"
        )));
        assert_eq!(output.status.code(), Some(2));
        assert!(
            stderr(&output).contains("filepath: Windows device and verbatim paths are unsupported")
        );
    }
}

#[cfg(windows)]
#[test]
fn windows_absolute_paths_have_honest_results() {
    prints(
        r#"package main
import "std/path/filepath"
import "std/os"
func main() {
    let cwd, cwdErr = os.Getwd()
    if cwdErr != nil { panic("cwd") }
    let resolved, resolveErr = filepath.Abs("notes/../file.txt")
    println(resolveErr == nil)
    println(resolved == filepath.Join(Array<string>{cwd, "file.txt"}[:]))
    println(filepath.IsAbs(resolved))
    let _, namespaceErr = filepath.Abs("\\\\?\\C:\\file")
    println(namespaceErr == error("filepath: Windows device and verbatim paths are unsupported"))
}
"#,
        "true\ntrue\ntrue\ntrue\n",
    );
}
