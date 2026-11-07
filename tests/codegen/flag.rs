use super::*;

const PRELUDE: &str = r#"
func check(ok bool, label string) {
    if !ok { panic(label) }
}

func greeter() flag.Set {
    var set = flag.New("greet", "Print a greeting.")
    set.String("name", "n", "world", "who to greet")
    set.Int("count", "c", 1, "how many times")
    set.Bool("shout", "s", "use capital letters")
    set.Strings("tag", "", "a tag to add")
    set.Arg("style", "plain or fancy")
    set.Rest("file", "extra files")
    return set
}

func words(text string) Array<string> {
    if text == "" { return Array<string>{} }
    return strings.Split(text, " ")
}

func joined(values Array<string>) string {
    return strings.Join(values[:], ",")
}
"#;

fn program(imports: &str, body: &str) -> String {
    format!(
        "package main\nimport \"std/flag\"\nimport \"std/strings\"\n{imports}\n{PRELUDE}\nfunc main() {{\n{body}\n}}\n"
    )
}

#[test]
fn flag_reads_long_short_attached_and_mixed_options() {
    let body = r#"
    let set = greeter()
    let plain, err = set.Parse(words("fancy")[:])
    check(err == nil, "defaults parse")
    check(plain.String("name") == "world" && plain.Int("count") == 1 && !plain.Bool("shout"), "defaults")
    check(!plain.Has("name") && !plain.Has("count") && !plain.Has("shout") && !plain.Has("tag"), "nothing given")
    check(plain.Strings("tag").len() == 0 && plain.Rest().len() == 0 && plain.Arg("style") == "fancy", "empty lists")

    let long, longErr = set.Parse(words("--name bob --count=3 --shout --tag a --tag=b plain x y")[:])
    check(longErr == nil, "long parse")
    check(long.String("name") == "bob" && long.Int("count") == 3 && long.Bool("shout"), "long values")
    check(joined(long.Strings("tag")) == "a,b" && joined(long.Rest()) == "x,y", "long lists")
    check(long.Has("name") && long.Has("count") && long.Has("shout") && long.Has("tag"), "long given")

    let short, shortErr = set.Parse(words("-n al -c=2 -s plain")[:])
    check(shortErr == nil && short.String("name") == "al" && short.Int("count") == 2 && short.Bool("shout"), "short forms")

    let mixed, mixedErr = set.Parse(words("x -n al y --count 1 z")[:])
    check(mixedErr == nil && mixed.Arg("style") == "x" && joined(mixed.Rest()) == "y,z", "options between arguments")
    check(mixed.Has("count") && mixed.Int("count") == 1, "explicit value equal to the default")

    let shaped, shapedErr = set.Parse(words("--name=-s --count -5 plain - -- --help -n --tag=x")[:])
    check(shapedErr == nil && shaped.String("name") == "-s" && !shaped.Bool("shout"), "option-shaped value after =")
    check(shaped.Int("count") == -5, "negative value")
    check(joined(shaped.Rest()) == "-,--help,-n,--tag=x" && !shaped.Has("tag"), "words after --")

    var empty = Array<string>{}
    empty.push("")
    empty.push("--name=")
    empty.push("")
    let blank, blankErr = set.Parse(empty[:])
    check(blankErr == nil && blank.Arg("style") == "" && blank.Has("name") && blank.String("name") == "", "empty words")
    check(blank.Rest().len() == 1, "empty rest word")

    let dash, dashErr = set.Parse(words("--name - --tag - -c -0 plain")[:])
    check(dashErr == nil && dash.String("name") == "-" && joined(dash.Strings("tag")) == "-" && dash.Has("count"), "lone dash value")

    let equals, equalsErr = set.Parse(words("--name=a=b -c=+7 plain")[:])
    check(equalsErr == nil && equals.String("name") == "a=b" && equals.Int("count") == 7, "value after the first =")

    var bare = flag.New("bare", "")
    bare.Bool("verbose", "v", "")
    let none, noneErr = bare.Parse(Array<string>{}[:])
    check(noneErr == nil && !none.Bool("verbose"), "no arguments at all")
    println("ok")
"#;
    prints(&program("", body), "ok\n");
}

#[test]
fn flag_rejects_malformed_input_with_exact_messages_and_an_empty_result() {
    let body = r#"
    let set = greeter()
    for case in Array<string>{
        "--nmae bob plain|flag: unknown option --nmae",
        "--pasword=hunter2 plain|flag: unknown option --pasword",
        "-x plain|flag: unknown option -x",
        "-p=hunter2 plain|flag: unknown option -p",
        "-sn plain|flag: unknown option -sn",
        "-n5 plain|flag: unknown option -n5",
        "--shout=yes plain|flag: option --shout takes no value",
        "-s= plain|flag: option --shout takes no value",
        "plain --name|flag: option --name needs a value",
        "plain -c|flag: option --count needs a value",
        "--name -s plain|flag: option --name needs a value",
        "--tag --name plain|flag: option --tag needs a value",
        "--name -- plain|flag: option --name needs a value",
        "-c -x plain|flag: option --count needs a value",
        "-c --5 plain|flag: option --count needs a value",
        "--count x plain|flag: option --count: invalid integer \"x\"",
        "-c=1.5 plain|flag: option --count: invalid integer \"1.5\"",
        "--count=99999999999999999999 plain|flag: option --count: invalid integer \"99999999999999999999\"",
        "--name a -n b plain|flag: option --name given more than once",
        "-s --shout plain|flag: option --shout given more than once",
        "-c 1 -c 1 plain|flag: option --count given more than once",
        "|flag: missing argument STYLE",
        "--name bob|flag: missing argument STYLE",
        "--bogus|flag: unknown option --bogus",
        "--bogus --help|flag: help requested",
        "--help --bogus|flag: help requested",
        "plain -h|flag: help requested",
        "--name --help|flag: help requested",
        "--count many -h|flag: help requested",
        "a b c -h=x --help|flag: help requested",
        "--bogus -- --help|flag: unknown option --bogus",
        "--name=--help|flag: missing argument STYLE",
        "-h=x|flag: option --help takes no value",
        "--help= plain|flag: option --help takes no value",
    } {
        let input, expected, _ = strings.Cut(case, "|")
        let result, err = set.Parse(words(input)[:])
        check(err != nil && errors.Message(err) == expected, case)
        check(!strings.Contains(errors.Message(err), "hunter2"), "secret repeated")
        check(!result.Has("name") && result.String("name") == "world" && result.Int("count") == 1, "empty result")
        check(result.Strings("tag").len() == 0 && result.Rest().len() == 0 && result.Arg("style") == "", "empty lists")
    }
    let _, help = set.Parse(words("--help")[:])
    check(help == flag.Help && errors.Is(help, flag.Help), "Help compares equal")

    var fixed = flag.New("copy", "")
    fixed.Arg("from", "")
    fixed.Arg("to-path", "")
    let _, missing = fixed.Parse(words("a")[:])
    check(errors.Message(missing) == "flag: missing argument TO_PATH", "second missing")
    let _, extra = fixed.Parse(words("a b c d")[:])
    check(errors.Message(extra) == "flag: unexpected argument \"c\"", "first extra")
    let _, late = fixed.Parse(words("a b c --bogus")[:])
    check(errors.Message(late) == "flag: unknown option --bogus", "options before counts")
    println("ok")
"#;
    prints(&program("import \"std/errors\"", body), "ok\n");
}

#[test]
fn flag_usage_text_is_built_in_declaration_order() {
    let body = r#"
    let set = greeter()
    fmt.Print(set.Usage())
    var bare = flag.New("tool", "")
    bare.Bool("dry-run", "", "")
    bare.String("out", "o", "", "where to write")
    bare.Int("level", "l", 0, "")
    bare.Strings("xs", "", "")
    fmt.Print(bare.Usage())
    var only = flag.New("only", "Nothing to set.")
    only.Rest("item", "")
    fmt.Print(only.Usage())
"#;
    prints(
        &program("import \"std/fmt\"", body),
        "Usage: greet [options] STYLE [FILE...]

Print a greeting.

Arguments:
  STYLE    plain or fancy
  FILE...  extra files

Options:
  -n, --name NAME    who to greet (default \"world\")
  -c, --count COUNT  how many times (default 1)
  -s, --shout        use capital letters
      --tag TAG      a tag to add (repeatable)
  -h, --help         show this help
Usage: tool [options]

Options:
      --dry-run
  -o, --out OUT      where to write
  -l, --level LEVEL
      --xs XS        (repeatable)
  -h, --help         show this help
Usage: only [options] [ITEM...]

Nothing to set.

Arguments:
  ITEM...

Options:
  -h, --help  show this help
",
    );
}

#[test]
fn flag_declaration_mistakes_panic() {
    for (declaration, message) in [
        (
            "set.Bool(\"help\", \"\", \"\")",
            "flag: reserved option --help",
        ),
        (
            "set.Bool(\"hide\", \"h\", \"\")",
            "flag: reserved option -h",
        ),
        (
            "set.Bool(\"x\", \"\", \"\")",
            "flag: invalid option name \"x\"",
        ),
        (
            "set.Bool(\"9lives\", \"\", \"\")",
            "flag: invalid option name \"9lives\"",
        ),
        (
            "set.Bool(\"dry run\", \"\", \"\")",
            "flag: invalid option name \"dry run\"",
        ),
        (
            "set.Bool(\"--name\", \"\", \"\")",
            "flag: invalid option name \"--name\"",
        ),
        (
            "set.Bool(\"quiet\", \"qq\", \"\")",
            "flag: invalid short name \"qq\"",
        ),
        (
            "set.Bool(\"quiet\", \"-\", \"\")",
            "flag: invalid short name \"-\"",
        ),
        (
            "set.Int(\"name\", \"\", 0, \"\")",
            "flag: duplicate option --name",
        ),
        (
            "set.Bool(\"nothing\", \"n\", \"\")",
            "flag: duplicate option -n",
        ),
        ("set.Arg(\"\", \"\")", "flag: invalid argument name \"\""),
        (
            "set.Arg(\"1st\", \"\")",
            "flag: invalid argument name \"1st\"",
        ),
        ("set.Arg(\"file\", \"\")", "flag: duplicate argument FILE"),
        ("set.Arg(\"more\", \"\")", "flag: argument MORE after Rest"),
        ("set.Rest(\"more\", \"\")", "flag: argument MORE after Rest"),
        (
            "set.Bool(\"file\", \"\", \"\")",
            "flag: option --file and argument FILE share a name",
        ),
        (
            "set.Bool(\"FILE\", \"\", \"\")",
            "flag: option --FILE and argument FILE share a name",
        ),
    ] {
        let source = format!(
            "package main\nimport \"std/flag\"\nfunc main() {{\n    var set = flag.New(\"t\", \"\")\n    set.String(\"name\", \"n\", \"\", \"\")\n    set.Rest(\"file\", \"\")\n    println(\"declared\")\n    {declaration}\n}}\n"
        );
        panics(&source, message, "declared\n");
    }
}

#[test]
fn flag_arguments_cannot_reuse_an_option_name() {
    let source = "package main\nimport \"std/flag\"\nfunc main() {\n    var set = flag.New(\"t\", \"\")\n    set.String(\"to-path\", \"\", \"\", \"\")\n    println(\"declared\")\n    set.Arg(\"to-path\", \"\")\n}\n";
    panics(
        source,
        "flag: option --to-path and argument TO_PATH share a name",
        "declared\n",
    );
}

#[test]
fn flag_result_lookups_panic_on_undeclared_names_and_kinds() {
    for (lookup, message) in [
        (
            "_ = result.String(\"count\")",
            "flag: no String option --count",
        ),
        ("_ = result.Int(\"name\")", "flag: no Int option --name"),
        (
            "_ = result.Bool(\"missing\")",
            "flag: no Bool option --missing",
        ),
        (
            "_ = result.Strings(\"name\")",
            "flag: no Strings option --name",
        ),
        ("_ = result.Has(\"missing\")", "flag: no option --missing"),
        ("_ = result.Arg(\"other\")", "flag: no argument OTHER"),
        ("_ = result.Rest()", "flag: no Rest argument"),
    ] {
        let source = format!(
            "package main\nimport \"std/flag\"\nfunc main() {{\n    var set = flag.New(\"t\", \"\")\n    set.String(\"name\", \"\", \"\", \"\")\n    set.Int(\"count\", \"\", 0, \"\")\n    set.Arg(\"file\", \"\")\n    var args = Array<string>{{}}\n    args.push(\"f\")\n    let result, err = set.Parse(args[:])\n    println(err == nil)\n    {lookup}\n}}\n"
        );
        panics(&source, message, "true\n");
    }
}

const REPORTING: &str = r#"package main
import "std/flag"
import "std/os"

func main() {
    var set = flag.New("greet", "Print a greeting.")
    set.String("name", "n", "world", "who to greet")
    set.Arg("style", "plain or fancy")
    let args = os.Args()
    let result, err = set.Parse(args[1:])
    if err != nil {
        var stdout = os.Stdout()
        var stderr = os.Stderr()
        os.Exit(set.Report(err, stdout, stderr))
    }
    println(result.Arg("style") + " hello " + result.String("name"))
}
"#;

#[test]
fn flag_report_prints_help_and_errors_and_returns_the_exit_status() {
    let dir = TempDir::new().unwrap();
    let executable = dir.executable("greet");
    build_file(REPORTING, &executable).unwrap_or_else(|e| panic!("build failed: {e:?}"));
    let invoke = |args: &[&str]| {
        Command::new(&executable)
            .args(args)
            .env("ZORE_CHECK_LEAKS", "1")
            .output()
            .unwrap()
    };
    let ok = invoke(&["fancy", "--name", "bo b"]);
    assert_eq!(ok.status.code(), Some(0), "{}", stderr(&ok));
    assert_eq!(stdout(&ok), "fancy hello bo b\n");
    assert!(ok.stderr.is_empty());
    let help = invoke(&["--help", "--bogus"]);
    assert_eq!(help.status.code(), Some(0), "{}", stderr(&help));
    assert!(help.stderr.is_empty(), "{}", stderr(&help));
    assert_eq!(
        stdout(&help),
        "Usage: greet [options] STYLE\n\nPrint a greeting.\n\nArguments:\n  STYLE  plain or fancy\n\nOptions:\n  -n, --name NAME  who to greet (default \"world\")\n  -h, --help       show this help\n"
    );
    let bad = invoke(&["--name=secret-value", "--nmae=secret-value"]);
    assert_eq!(bad.status.code(), Some(2), "{}", stderr(&bad));
    assert!(bad.stdout.is_empty());
    assert_eq!(
        stderr(&bad),
        "greet: unknown option --nmae\nUsage: greet [options] STYLE\nRun 'greet --help' for more information.\n"
    );
    let missing = invoke(&[]);
    assert_eq!(missing.status.code(), Some(2));
    assert!(stderr(&missing).starts_with("greet: missing argument STYLE\n"));
}

#[test]
fn flag_report_returns_one_when_its_writer_fails_and_zero_for_nil() {
    let source = r#"package main
import "std/errors"
import "std/flag"
import "std/fmt"

type Broken struct { calls int }
func (b mut Broken) Write(data []byte) (int, error) {
    b.calls += 1
    return 0, error("broken pipe")
}

type Kept struct { text string }
func (k mut Kept) Write(data []byte) (int, error) {
    for value in data { k.text = k.text + string(rune(value)) }
    return data.len(), nil
}

func main() {
    var set = flag.New("t", "")
    var broken = Broken{calls: 0}
    var kept = Kept{text: ""}
    println(set.Report(nil, broken, kept))
    println(broken.calls)
    println(set.Report(flag.Help, broken, kept))
    println(set.Report(error("flag: unknown option --x"), kept, broken))
    println(broken.calls)
    println(kept.text == "")
    println(set.Report(errors.Wrap(flag.Help, "tool"), kept, broken))
    println(broken.calls)
    println(set.Report(error("other failure"), broken, kept))
    fmt.Print(kept.text)
}
"#;
    prints(
        source,
        "0\n0\n1\n1\n2\ntrue\n0\n2\n2\nUsage: t [options]\n\nOptions:\n  -h, --help  show this help\nt: other failure\nUsage: t [options]\nRun 't --help' for more information.\n",
    );
}

#[test]
fn flag_parses_in_async_tasks_and_propagates_errors_without_leaks() {
    let source = r#"package main
import "std/errors"
import "std/flag"

func options() flag.Set {
    var set = flag.New("job", "")
    set.Int("workers", "w", 2, "")
    set.Strings("tag", "t", "")
    set.Rest("input", "")
    return set
}

func workers(args Array<string>) (int, error) {
    let set = options()
    let result = set.Parse(args[:])?
    return result.Int("workers") + result.Rest().len() + result.Strings("tag").len(), nil
}

async func run(args own Array<string>) int {
    let count, err = workers(args)
    if err != nil { return -1 }
    return count
}

func main() {
    var good = Array<string>{}
    good.push("-w")
    good.push("5")
    good.push("a")
    good.push("-t=x")
    let task = go run(good)
    println(task.wait())
    var bad = Array<string>{}
    bad.push("--workers=many")
    let _, err = workers(bad)
    println(errors.Message(err))
    let failed = go run(bad)
    println(failed.wait())
}
"#;
    prints(
        source,
        "7\nflag: option --workers: invalid integer \"many\"\n-1\n",
    );
}
