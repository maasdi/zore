use super::*;
use std::time::Duration;

fn execute(imports: &str, program: &str, controlled: bool) -> Output {
    let source = format!(
        "package main\nimport \"std/regexp\"\n{imports}\nfunc check(ok bool, label string) {{ if !ok {{ panic(label) }} }}\n{program}"
    );
    run_source(&source, controlled)
}

fn run_source(source: &str, controlled: bool) -> Output {
    let directory = TempDir::new().unwrap();
    let executable = directory.executable("regexp");
    if controlled {
        controlled_runtime::build_with_workers(source, &executable, 1);
    } else {
        build_file(source, &executable).unwrap_or_else(|error| panic!("build: {error:?}"));
    }
    bounded_process::run(
        Command::new(executable).env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(30),
    )
    .unwrap()
}

fn succeeds(imports: &str, program: &str, controlled: bool) {
    let output = execute(imports, program, controlled);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "ok\n");
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

#[test]
fn regex_find_reports_ordered_captures_and_unicode_byte_offsets() {
    succeeds(
        "",
        r#"func main() {
    let re, err = regexp.Compile("(é)([α-ω]+)(a)?")
    check(err == nil, "compile")
    let found, present, findErr = re.Find("xéαβ!")
    check(findErr == nil && present, "find")
    check(found.Start == 1 && found.End == 7 && found.Groups.len() == 3, "whole span")
    check(found.Groups[0].Start == 1 && found.Groups[0].End == 3, "first capture")
    check(found.Groups[1].Start == 3 && found.Groups[1].End == 7, "second capture")
    check(!found.Groups[2].Matched() && found.Groups[2].Start == -1 && found.Groups[2].End == -1, "absent group")
    let absent, missing, absentErr = re.Find("no match")
    check(absentErr == nil && !missing && absent.Start == -1 && absent.End == -1 && absent.Groups.len() == 0, "absence")
    let lazy, lazyErr = regexp.Compile("(a+?)(a)")
    check(lazyErr == nil, "lazy compile")
    let short, exists, shortErr = lazy.Find("aaa")
    check(shortErr == nil && exists && short.End == 2 && short.Groups[0].End == 1, "lazy capture")
    let earlier, earlierErr = regexp.Compile("a|ab")
    check(earlierErr == nil, "ordered compile")
    let first, firstFound, firstErr = earlier.Find("ab")
    check(firstErr == nil && firstFound && first.End == 1, "alternative order")
    let ascii, asciiErr = regexp.Compile("\\w+")
    check(asciiErr == nil, "ascii compile")
    let matches, matchErr = ascii.Match("αβ")
    check(matchErr == nil && !matches, "ascii shorthand")
    println("ok")
}
"#,
        false,
    );
}

#[test]
fn regex_empty_iteration_and_replacement_use_explicit_bounds_and_escaping() {
    succeeds(
        "",
        r#"func main() {
    let empty, compileErr = regexp.Compile("")
    check(compileErr == nil, "empty compile")
    let boundaries, findErr = empty.FindAll("é🦀", 10)
    check(findErr == nil && boundaries.len() == 3, "rune boundaries")
    check(boundaries[0].Start == 0 && boundaries[1].Start == 2 && boundaries[2].Start == 6, "byte boundaries")
    let prefix, prefixErr = empty.FindAll("é🦀", 1)
    check(prefixErr == nil && prefix.len() == 1, "bounded results")
    let stars, starsErr = regexp.Compile("a*")
    check(starsErr == nil, "nullable compile")
    let only, onlyErr = stars.FindAll("aa", 10)
    check(onlyErr == nil && only.len() == 1 && only[0].End == 2, "adjacent empty omitted")
    let re, err = regexp.Compile("([a-z]+)=([0-9]+)")
    check(err == nil, "compile")
    let expanded, replaceErr = re.ReplaceAll("n=12 x=7", "${2}:$1:$$:$0")
    check(replaceErr == nil && expanded == "12:n:$:n=12 7:x:$:x=7", "expanded")
    let literal, literalErr = re.ReplaceAllLiteral("n=12", "$1\\")
    check(literalErr == nil && literal == "$1\\", "literal")
    for template in Array<string>{"$", "$x", "${}", "${1", "$3"} {
        let result, invalid = re.ReplaceAll("no matches", template)
        check(invalid != nil && result == "", "invalid reference even without match")
    }
    let dashed, dashedErr = empty.ReplaceAll("é", "-")
    check(dashedErr == nil && dashed == "-é-", "empty replacement iteration")
    println("ok")
}
"#,
        false,
    );
}

#[test]
fn regex_syntax_and_closed_handles_fail_without_partial_results() {
    succeeds(
        "",
        r#"func main() {
    for pattern in Array<string>{"(", "[z-a]", "a**", "(?=a)", "(?i)a", "\\1", "\\p{L}"} {
        let failed, invalid = regexp.Compile(pattern)
        check(invalid != nil, "invalid pattern")
        let value, closed = failed.Match("a")
        check(closed == error("regexp.Match: closed pattern") && !value, "closed failure handle")
        let shared, shareErr = failed.Share()
        check(shareErr == error("regexp.Share: closed pattern"), "cannot share closed handle")
        drop(shared)
    }
    let re, err = regexp.Compile("a")
    check(err == nil, "compile")
    for limit in Array<int>{-1, 0, regexp.MaxMatches + 1} {
        let none, invalid = re.FindAll("a", limit)
        check(invalid == error("regexp.FindAll: invalid match limit") && none.len() == 0, "invalid limit")
    }
    println("ok")
}
"#,
        false,
    );
}

#[test]
fn regex_adversarial_inputs_and_expansion_limits_fail_explicitly() {
    succeeds(
        "import \"std/strings\"",
        r#"func main() {
    let tooLarge, sizeErr = regexp.Compile(strings.Repeat("a", regexp.MaxPatternBytes + 1))
    check(sizeErr == error("regexp.Compile: pattern byte limit exceeded"), "pattern size")
    drop(tooLarge)
    let expanded, stateErr = regexp.Compile("(?:a{1000}){5}")
    check(stateErr == error("regexp.Compile: compiled state limit exceeded"), "compiled states")
    drop(expanded)
    let groups, groupsErr = regexp.Compile(strings.Repeat("()", regexp.MaxCaptures + 1))
    check(groupsErr != nil, "capture bound")
    drop(groups)
    let re, err = regexp.Compile("a")
    check(err == nil, "compile")
    let value, inputErr = re.Match(strings.Repeat("a", regexp.MaxInputBytes + 1))
    check(inputErr == error("regexp.Match: input byte limit exceeded") && !value, "input bound")
    let output, outputErr = re.ReplaceAllLiteral("aa", strings.Repeat("x", regexp.MaxOutputBytes))
    check(outputErr == error("regexp.ReplaceAllLiteral: output byte limit exceeded") && output == "", "output bound")
    let many, countErr = re.ReplaceAll(strings.Repeat("a", regexp.MaxMatches + 1), "")
    check(countErr == error("regexp.ReplaceAll: match limit exceeded") && many == "", "replacement count")
    let costly, costlyErr = regexp.Compile("(?:a?){1000}b")
    check(costlyErr == nil, "adversarial compile")
    let matched, workErr = costly.Match(strings.Repeat("a", 8192))
    check(workErr == error("regexp.Match: work limit exceeded") && !matched, "bounded adversarial work")
    let linear, linearErr = regexp.Compile("^(a|aa)*b$")
    check(linearErr == nil, "nested compile")
    let found, linearMatchErr = linear.Match(strings.Repeat("a", 10000))
    check(linearMatchErr == nil && !found, "no exponential backtracking")
    let optional, optionalErr = regexp.Compile("(a)?b")
    check(optionalErr == nil, "optional capture")
    let discarded, expansionErr = optional.ReplaceAll(strings.Repeat("b", regexp.MaxMatches), strings.Repeat("$1", regexp.MaxInputBytes / 2))
    check(expansionErr == error("regexp.ReplaceAll: work limit exceeded") && discarded == "", "empty capture expansion budget")
    println("ok")
}
"#,
        false,
    );
}

#[test]
fn regex_shared_handles_and_scoped_borrows_work_on_one_scheduler_worker() {
    succeeds(
        "",
        r#"async func owned(re own regexp.Regexp, results channel<bool>) {
    let matched, err = re.Match("é42")
    results.send(err == nil && matched)
}
func borrowed(re regexp.Regexp, results channel<bool>) {
    let matched, err = re.Match("é42")
    results.send(err == nil && matched)
}
func fail() error {
    let local = regexp.Compile("a")?
    let shared = local.Share()?
    let _, err = shared.Match("a")
    if err != nil { return err }
    return error("intentional")
}
func main() {
    let re, err = regexp.Compile("^é[0-9]+$")
    check(err == nil, "compile")
    let results = channel<bool>(64)
    scope {
        for var i = 0; i < 8; i += 1 { go borrowed(re, results) }
    }
    for var i = 0; i < 64; i += 1 {
        let share, shareErr = re.Share()
        check(shareErr == nil, "share")
        go owned(share, results)
    }
    drop(re)
    for var i = 0; i < 72; i += 1 {
        let matched, _ = results.receive()
        check(matched, "shared immutable pattern")
    }
    waitIdle(0)
    check(fail() == error("intentional"), "error cleanup")
    println("ok")
}
"#,
        true,
    );
}

#[test]
fn regex_panic_releases_all_compiled_handles() {
    let output = execute(
        "",
        r#"func main() {
    let re, err = regexp.Compile("a+")
    check(err == nil, "compile")
    let share, shareErr = re.Share()
    check(shareErr == nil, "share")
    let matched, matchErr = share.Match("a")
    check(matchErr == nil && matched, "match")
    panic("regexp cleanup probe")
}

"#,
        false,
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("regexp cleanup probe"));
    assert!(!stderr(&output).contains("leak:"), "{}", stderr(&output));
}

#[test]
fn regex_guide_examples_compile_and_run_together() {
    let guide = include_str!("../../docs/regexp.md");
    let blocks: Vec<_> = guide
        .split("```ore\n")
        .skip(1)
        .map(|block| block.split("```").next().unwrap())
        .collect();
    assert_eq!(blocks.len(), 2);
    let source = blocks.join("\n").replace("let err = run()", "let err = run()\n    let parallelErr = concurrent()\n    if parallelErr != nil { panic(\"concurrent regex example failed\") }");
    let output = run_source(&source, false);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "name=é\nname\nname: é ($) count: 42 ($)\n2\ntrue\n"
    );
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}
