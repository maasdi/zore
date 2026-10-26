use super::{prints, run, stderr};

#[test]
fn standard_string_methods_preserve_unicode_empty_and_package_function_behavior() {
    prints(
        r#"package main
import "std/strings"
func check(ok bool) { if !ok { panic("string method result") } }
func main() {
    let text = "héllo ß"
    check(text.Contains("é") && text.Contains("") && !text.Contains("z"))
    check(text.ContainsRune('é') && !text.ContainsRune('界'))
    check(text.HasPrefix("hé") && text.HasSuffix("ß"))
    check(text.Index("l") == 3 && text.Index("z") == -1)
    check("banana".LastIndex("a") == 5)
    check(text.IndexByte(108) == 3 && text.IndexRune('é') == 1)
    check("hé".Count("") == 3 && "banana".Count("an") == 2)
    check("Straße".EqualFold("STRAßE"))
    check(text.ToUpper() == "HÉLLO SS" && "HÉLLO".ToLower() == "héllo")
    check("\u2003héllo\u00a0".TrimSpace() == "héllo")
    check("éxé".Trim("é") == "x")
    check("éxé".TrimLeft("é") == "xé" && "éxé".TrimRight("é") == "éx")
    check("prefix-value".TrimPrefix("prefix-") == "value")
    check("value-suffix".TrimSuffix("-suffix") == "value")
    let key, value, found = "name=Ada".Cut("=")
    check(key == "name" && value == "Ada" && found)
    let missing, empty, absent = "name".Cut("=")
    check(missing == "name" && empty == "" && !absent)
    check("hé".Repeat(2) == "héhé" && "x".Repeat(0) == "")
    check("banana".Replace("an", "AN", 1) == "bANana")
    check("banana".ReplaceAll("an", "AN") == "bANANa")
    check("hé".ReplaceAll("", "-") == "-h-é-")
    let runes = "hé".Split("")
    check(runes.len() == 2 && runes[1] == "é")
    let parts = "a:b:c".SplitN(":", 2)
    check(parts.len() == 2 && parts[1] == "b:c")
    check("a:b".SplitN(":", 0).len() == 0)
    let words = "\u2003one\ttwo\n".Fields()
    check(words.len() == 2 && words[1] == "two")
    var bytes = "hé".Bytes()
    check(bytes.len() == 3 && bytes[0] == 104 && bytes[1] == 195 && bytes[2] == 169)
    bytes[0] = 120
    let converted, err = strings.FromBytes(bytes[:])
    check(err == nil && converted == "xé")
    check(text.ToUpper() == strings.ToUpper(text))
    check(text.TrimSpace() == strings.TrimSpace(text))
    check("".TrimSpace().ToUpper() == "" && "".Fields().len() == 0)
    check("".Split("").len() == 0 && "".Split(",").len() == 1)
    println("ok")
    return
}
"#,
        "ok\n",
    );
}

#[test]
fn string_methods_evaluate_receivers_once_before_arguments_and_release_owned_results() {
    prints(
        r#"package main
func step(log mut Array<int>, n int, text string) string { log.push(n); return text }
func position(log mut Array<int>) int { log.push(4); return 0 }
func main() {
    var log = Array<int>{}
    let parts = step(log, 1, "a,b").Split(step(log, 2, ","))
    println(parts[1])
    println(step(log, 3, " x ").TrimSpace().ToUpper())
    let names = Array<string>{"é"}
    println(names[position(log)].ToUpper())
    for item in log { println(item) }
    for var i = 0; i < 20; i += 1 {
        let bytes = ("owned-" + string(rune(97 + i))).Bytes()
        let fields = ("owned " + string(rune(97 + i))).Fields()
        if bytes.len() != 7 || fields.len() != 2 { panic("owned result") }
    }
    scope {
        let unused = go ("discarded" + " result").Bytes()
        let split = go "x,y".Split(",")
        let values = split.wait()
        println(values[0])
    }
}
"#,
        "b\nX\nÉ\n1\n2\n3\n4\nx\n",
    );
}

#[test]
fn string_method_values_interfaces_and_async_receivers_follow_existing_ownership_rules() {
    prints(
        r#"package main
type TextOps interface { ToUpper() string; Contains(part string) bool; Bytes() Array<byte> }
type Holder struct { Text string }
func describe(value TextOps) string {
    let bytes = value.Bytes()
    if !value.Contains("é") || bytes.len() != 6 { panic("interface method") }
    return value.ToUpper()
}
func launch(text string) Task<string> { let upper = text.ToUpper; return go upper() }
async func create() string { return "  " + "héllo  " }
async func transform() string { let child = go create(); return (await child).TrimSpace().ToUpper() }
func TrimSpace(text string) string { return "wrong provider" }
func main() {
    let strings = "héllo".ToUpper().ToLower()
    println(describe(strings))
    let boxed TextOps = strings
    println(describe(boxed))
    let values = Array<TextOps>{strings}
    println(values[0].ToUpper())
    let holder = Holder{Text: strings}
    let upper = holder.Text.ToUpper
    let contains = holder.Text.Contains
    println(upper())
    println(contains("é"))
    let job = launch(strings)
    println(job.wait())
    let transformed = go transform()
    println(transformed.wait())
    println("  canonical  ".TrimSpace())
}
"#,
        "HÉLLO\nHÉLLO\nHÉLLO\nHÉLLO\ntrue\nHÉLLO\nHÉLLO\ncanonical\n",
    );
}

#[test]
fn string_method_panics_preserve_the_package_diagnostic_and_cleanup() {
    let output = run(r#"package main
func main() {
    let kept = "owned bytes".Bytes()
    println(("owned" + " receiver").Repeat(-1))
    println(kept.len())
}
"#);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("strings.Repeat: negative count"));
    assert!(!stderr(&output).contains("leak:"));
}
