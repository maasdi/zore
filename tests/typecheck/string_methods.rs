use super::{accepts, body, rejects};

#[test]
fn standard_string_methods_work_without_imports_and_preserve_result_types() {
    accepts(&body(
        r#"
        let title string = "  héllo  ".TrimSpace().ToUpper()
        let contains bool = title.Contains("É")
        let offset int = title.IndexRune('É')
        let words Array<string> = title.Split(" ")
        let bytes Array<byte> = title.Bytes()
        let left, right, found = title.Cut("É")
        println(left + right)
        println(found)
    "#,
    ));
    accepts(
        r#"package main
import "std/strings"
func main() {
    let text = "  Zore  "
    println(text.TrimSpace() == strings.TrimSpace(text))
    let parts = text.Fields()
    println(strings.Join(parts[:], ","))
}"#,
    );
}

#[test]
fn string_methods_keep_named_type_identity_and_existing_method_value_rules() {
    accepts(
        r#"package main
type Text string
func (text Text) TrimSpace() Text { return text }
type Holder struct { Text string }
func main() {
    let named = Text(" named ")
    let retained Text = named.TrimSpace()
    println(string(retained).TrimSpace())
    let holder = Holder{Text: "hello"}
    let upper func() string = holder.Text.ToUpper
    let contains func(string) bool = holder.Text.Contains
    println(upper())
    println(contains("h"))
}"#,
    );
    rejects(
        r#"package main
type Text string
func main() { let text = Text("hello"); println(text.ToUpper()) }
"#,
        "no method `ToUpper`",
    );
    rejects(
        &body("let upper = \"hello\".ToUpper; println(upper())"),
        "receiver of a method value must be a local",
    );
    rejects(
        &body(
            "let words = Array<string>{\"hello\"}; let upper = words[0].ToUpper; println(upper())",
        ),
        "receiver of a method value must be a local",
    );
}

#[test]
fn string_methods_reject_bad_arguments_nil_and_primitive_extensions() {
    for (source, expected) in [
        ("println(\"x\".TrimSpace(1))", "takes 0 arguments"),
        ("println(\"x\".Contains())", "takes 1 argument"),
        ("println(\"x\".Contains(1))", "expected `string`"),
        ("println(\"x\".Split(1))", "expected `string`"),
        ("println(\"x\".Repeat(\"2\"))", "expected `int64`"),
        ("println(\"x\".IndexRune(\"x\"))", "expected `rune`"),
        (
            "let n int = 1; println(n.TrimSpace())",
            "no method `TrimSpace`",
        ),
        ("let text string = nil", "nil"),
        ("println(\"x\" == nil)", "nil"),
        ("let data = \"x\".FromBytes()", "no method `FromBytes`"),
    ] {
        rejects(&body(source), expected);
    }
    for package in ["main", "strings"] {
        rejects(
            &format!(
                r#"package {package}
func (text string) TrimSpace() string {{ return text }}
func main() {{}}
"#
            ),
            "methods can be declared only on struct or named types",
        );
    }
}

#[test]
fn string_method_discovery_covers_nested_headers_closures_and_initializers() {
    for source in [
        "let title = \" x \".TrimSpace()\nfunc main() { println(title) }",
        "func main() { let f = func() string { return \" x \".TrimSpace() }; println(f()) }",
        "func main() { for item in \"x y\".Fields() { println(item) } }",
        "func main() { if false { println(1) } else if \"x\".Contains(\"x\") { println(2) } }",
        "func main() { scope { let child = go \"x\".ToUpper(); println(child.wait()) } }",
        "func main() { let ch = channel<string>(1); ch.send(\"x\"); select { case let text, ok = ch.receive() { if ok { println(text.ToUpper()) } } } }",
        "func main() { let values = map[string]string{\"x\": \" y \".TrimSpace()}; println(values.len()) }",
    ] {
        accepts(&format!("package main\n{source}"));
    }
}

#[test]
fn string_methods_satisfy_interfaces_and_use_ordinary_task_rules() {
    accepts(
        r#"package main
type TextOps interface { ToUpper() string }
func accept(value TextOps) {}
func main() { let text = "hello"; accept(text) }
"#,
    );
    accepts(
        r#"package main
type TextOps interface { ToUpper() string; Contains(part string) bool; Bytes() Array<byte> }
func describe(value TextOps) string { return value.ToUpper() }
func launch(text string) Task<string> { let upper = text.ToUpper; return go upper() }
async func text() string { return "héllo" }
async func transform() string { let child = go text(); return (await child).ToUpper() }
func main() {
    let text = "hello"
    println(describe(text))
    let upper = launch(text)
    println(upper.wait())
    let transformed = go transform()
    println(transformed.wait())
}"#,
    );
    accepts(
        r#"package main
type Mutating interface { mut ToUpper() string }
func update(value mut Mutating) {}
func main() { var text = "x"; update(text) }
"#,
    );
    rejects(
        r#"package main
type Wrong interface { ToUpper() int }
func use(value Wrong) {}
func main() { let text = "x"; use(text) }
"#,
        "has a different signature",
    );
    rejects(
        r#"package main
import "std/strings"
func main() { println("x".ToUpper()) }
"#,
        "imported and not used",
    );
    rejects(
        &body("var text = \"x\"; let upper = text.ToUpper; text = \"y\"; println(upper())"),
        "borrow",
    );
}
