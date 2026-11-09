#[test]
fn html_has_a_plain_string_api_available_without_native_linking() {
    super::accepts(&[(
        "main.ore",
        r#"package main
import "std/encoding/html"
async func render(text string) string { return html.EscapeString(text) }
func main() {
    let source = "<&>"
    let escape = html.EscapeString
    println(escape(source))
    println(source)
}
"#,
    )]);
    for (body, expected) in [
        (
            "println(html.EscapeString(Array<byte>{60}[:]))",
            "mismatched types",
        ),
        ("println(html.entity(60))", "not exported"),
        ("println(html.EscapeString())", "takes 1 argument"),
    ] {
        let source =
            format!("package main\nimport \"std/encoding/html\"\nfunc main() {{ {body} }}\n");
        super::rejects(&[("main.ore", &source)], expected);
    }
}
