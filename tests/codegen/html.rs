use super::*;
use std::time::Duration;

fn execute(source: &str, controlled: bool) -> Output {
    let directory = TempDir::new().unwrap();
    let executable = directory.executable("html");
    if controlled {
        controlled_runtime::build_with_workers(source, &executable, 1);
    } else {
        build_file(source, &executable).unwrap_or_else(|e| panic!("{e:?}"));
    }
    bounded_process::run(
        Command::new(executable).env("ZORE_CHECK_LEAKS", "1"),
        Duration::from_secs(30),
    )
    .unwrap()
}

#[test]
fn html_escaping_preserves_utf8_and_replaces_exactly_five_characters() {
    let source = r#"package main
import "std/html"
import "std/strings"
func check(ok bool, label string) { if !ok { panic(label) } }
func main() {
    check(html.EscapeString("") == "", "empty")
    check(html.EscapeString("&<>\"'") == "&amp;&lt;&gt;&quot;&#39;", "five characters")
    check(html.EscapeString("&amp;&lt;&#39;&#x27;&quot;") == "&amp;amp;&amp;lt;&amp;#39;&amp;#x27;&amp;quot;", "existing entities are data")
    check(html.EscapeString("漢字🙂é\u0000\r\n\t\u2028") == "漢字🙂é\u0000\r\n\t\u2028", "unchanged UTF-8 and controls")
    check(html.EscapeString("é<🙂&漢>é\"𝄞'") == "é&lt;🙂&amp;漢&gt;é&quot;𝄞&#39;", "ASCII boundaries do not split UTF-8")
    check(html.EscapeString("javascript:alert(1) ` = /") == "javascript:alert(1) ` = /", "does not validate active contexts")
    let escaped = html.EscapeString("&")
    check(html.EscapeString(escaped) == "&amp;amp;", "not idempotent")
    let dense = strings.Repeat("\"", 50000)
    check(html.EscapeString(dense) == strings.Repeat("&quot;", 50000), "sixfold expansion")
    let plain = strings.Repeat("🙂éa", 50000)
    check(html.EscapeString(plain) == plain, "long unchanged text")
    println("ok")
}
"#;
    let output = execute(source, false);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "ok\n");
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

#[test]
fn html_results_survive_sources_and_one_worker_suspension_without_leaks() {
    let source = r#"package main
import "std/html"
import "std/strings"
func check(ok bool, label string) { if !ok { panic(label) } }
func local(escape bool) string {
    var source = strings.NewBuilder()
    source.WriteString("漢字🙂")
    if escape { source.WriteString("<&\"'>") }
    return html.EscapeString(source.String())
}
async func render(text string, gate channel<bool>) string {
    let escaped = html.EscapeString(text)
    let _, _ = gate.receive()
    return escaped
}
func scenario() {
    waitIdle(0)
    let plain = local(false)
    let escaped = local(true)
    for var i = 0; i < 100; i += 1 { let temporary = html.EscapeString(local(true)); check(temporary.len() > 0, "temporary result") }
    check(plain == "漢字🙂", "unchanged result survives source")
    check(escaped == "漢字🙂&lt;&amp;&quot;&#39;&gt;", "escaped result survives source")
    let gate = channel<bool>()
    var input = "<&🙂>"
    let task = go render(input, gate)
    waitPending(1)
    input = "changed"
    gate.send(true)
    let result = task.wait()
    check(input == "changed" && result == "&lt;&amp;🙂&gt;", "string values across suspension")
    let escape = html.EscapeString
    check(escape("x'y") == "x&#39;y", "function value")
    waitIdle(0)
}
func main() { scenario(); println("ok") }
"#;
    let output = execute(source, true);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "ok\n");
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}

#[test]
fn html_guide_example_runs() {
    let guide = include_str!("../../docs/html.md");
    let source = guide
        .split("```ore\n")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap();
    let output = execute(source, false);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "<p title=\"Ada &amp; &lt;friends&gt;\">Hello Ada &amp; &lt;friends&gt;</p>\n"
    );
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
}
