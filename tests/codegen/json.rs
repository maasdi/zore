use super::*;

#[test]
fn json_tokens_validate_rfc_syntax_ranges_and_sticky_errors() {
    prints(
        r#"package main
import "std/encoding/json"
import "std/strings"
import "std/errors"
import "std/math"
func require(ok bool) { if !ok { panic("JSON assertion") } }
func valid(text string) bool { let bytes = strings.Bytes(text); return json.Valid(bytes[:]) }
func main() {
    let good = Array<string>{"null", "true", "false", "-0", "0.1e+20", "1e9999", "[]", "{}", "[1, {\"a\": [false, null]}]", "\"\\u0000\\uD834\\uDD1Eé\"", "{\"x\":1,\"x\":2}", " \t\r\n123\r\n"}
    for text in good { require(valid(text)) }
    let bad = Array<string>{"", " ", "+1", "01", "-01", ".1", "1.", "1e", "1e+", "NaN", "Infinity", "nullx", "truefalse", "[1,]", "{\"a\":1,}", "[1 2]", "{\"a\" 1}", "{1:2}", "[}", "[", "{", "\"\\uD800\"", "\"\\uDC00\"", "\"\\uD800\\u0041\"", "\"\\x20\"", "\"\\uZZZZ\"", "\"a\nb\"", "null null", "/*x*/null", "\uFEFFnull"}
    for text in bad { require(!valid(text)) }
    let invalid = Array<byte>{34, 192, 128, 34}
    require(!json.Valid(invalid[:]))
    let integers = strings.Bytes("[-9223372036854775808,9223372036854775807,9223372036854775808]")
    var d = json.NewDecoder(integers[:])
    require(d.BeginArray() == nil)
    let minimum, minErr = d.Int()
    _ = minErr
    let maximum, maxErr = d.Int()
    _ = maxErr
    require(minErr == nil && maxErr == nil && minimum == -9223372036854775808 && maximum == 9223372036854775807)
    let _, rangeErr = d.Int()
    _ = rangeErr
    require(rangeErr != nil && !d.More() && d.Finish() == rangeErr)
    let decimals = strings.Bytes("1.0")
    var integer = json.NewDecoder(decimals[:])
    let _, fractionErr = integer.Int()
    _ = fractionErr
    require(fractionErr != nil)
    let huge = strings.Bytes("1e9999")
    var floating = json.NewDecoder(huge[:])
    let _, floatErr = floating.Float()
    _ = floatErr
    require(floatErr != nil)
    let position = strings.Bytes("{\n \"a\" 1}")
    var located = json.NewDecoder(position[:])
    require(located.BeginObject() == nil)
    let _, syntaxErr = located.Key()
    _ = syntaxErr
    require(strings.HasPrefix(errors.Message(syntaxErr), "json: line 2, column 6:"))
    var encoder = json.NewEncoder()
    encoder.SetIndent("  ")
    encoder.BeginObject()
    encoder.Field("text")
    encoder.String("<>&\u2028\u2029\n\"\\")
    encoder.Field("empty")
    encoder.BeginArray()
    encoder.EndArray()
    encoder.EndObject()
    let output, outErr = encoder.Finish()
    _ = outErr
    require(outErr == nil)
    let rendered, textErr = strings.FromBytes(output[:])
    _ = textErr
    require(textErr == nil)
    println(rendered)
    var poisoned = json.NewEncoder()
    poisoned.Float(math.Inf(1))
    poisoned.Int(1)
    let bytes, firstErr = poisoned.Finish()
    _ = firstErr
    require(firstErr != nil && bytes.len() == 0)
    println("ok")
}
"#,
        "{\n  \"text\": \"\\u003c\\u003e\\u0026\\u2028\\u2029\\u000a\\\"\\\\\",\n  \"empty\": []\n}\nok\n",
    );
}

#[test]
fn json_tree_preserves_numbers_order_clones_and_depth_limits() {
    prints(
        r#"package main
import "std/encoding/json"
import "std/strings"
func require(ok bool) { if !ok { panic("JSON tree assertion") } }
func main() {
    let input = strings.Bytes("{\"a\":1,\"b\":[-0,1e9999,\"\\uD83D\\uDE00\"],\"a\":2}")
    let tree, err = json.Parse(input[:])
    _ = err
    require(err == nil && tree.Len() == 2 && tree.Has("a") && !tree.Has("c"))
    let number, numberErr = tree.Get("a").Int()
    _ = numberErr
    require(numberErr == nil && number == 2)
    require(tree.Get("missing").Get("x").IsNull())
    var child = tree.Get("b")
    child.Push(json.BoolValue(true))
    require(child.Len() == 4 && tree.Get("b").Len() == 3)
    let _, overflow = tree.Get("b").Index(1).Float()
    _ = overflow
    require(overflow != nil)
    let keys = tree.Keys()
    require(keys[0] == "a" && keys[1] == "b")
    var encoder = json.NewEncoder()
    encoder.Value(tree)
    let output, outErr = encoder.Finish()
    _ = outErr
    require(outErr == nil)
    let text, textErr = strings.FromBytes(output[:])
    _ = textErr
    require(textErr == nil)
    println(text)
    var deep = ""
    for var i = 0; i < 600; i += 1 { deep += "[" }
    deep += "0"
    for var i = 0; i < 600; i += 1 { deep += "]" }
    let bytes = strings.Bytes(deep)
    require(!json.Valid(bytes[:]))
    var decoder = json.NewDecoder(bytes[:])
    decoder.SetMaxDepth(600)
    let parsed, parseErr = decoder.Value()
    _ = parseErr
    require(parseErr == nil && decoder.Finish() == nil)
    var reencode = json.NewEncoder()
    reencode.Value(parsed)
    let encoded, encodeErr = reencode.Finish()
    _ = encodeErr
    let roundtrip, roundErr = strings.FromBytes(encoded[:])
    _ = roundErr
    require(encodeErr == nil && roundErr == nil && roundtrip == deep)
    var skip = json.NewDecoder(bytes[:])
    skip.SetMaxDepth(600)
    require(skip.Skip() == nil && skip.Finish() == nil)
    println("ok")
}
"#,
        "{\"a\":2,\"b\":[-0,1e9999,\"😀\"]}\nok\n",
    );
}

#[test]
fn json_generated_struct_codecs_support_naming_optional_strict_and_custom_types() {
    prints(
        r#"package main
import "std/encoding/json"
import "std/strings"
type Celsius float64
func (c Celsius) EncodeJSON(e mut json.Encoder) { e.String("warm") }
func (c mut Celsius) DecodeJSON(d mut json.Decoder) error { let _ = d.String()?; c = Celsius(21.5); return nil }
type CaseNames struct { ABc int; AbC int }
type User struct { ID int; HTTPServer string; UserID int; Email json.Optional<string>; Tags Array<string>; Temp Celsius; secret string }
func require(ok bool) { if !ok { panic("JSON codec assertion") } }
func encode<T any>(value T) (Array<byte>, error) { return json.MarshalWith(value, json.LowerCamel) }
func main() {
    var user = User{ID: 1, HTTPServer: "local", UserID: 9, Email: json.Optional<string>{Value: "old", Set: false}, Tags: Array<string>{"admin"}, Temp: Celsius(0), secret: "private"}
    let output, err = encode(user)
    _ = err
    require(err == nil)
    let text, textErr = strings.FromBytes(output[:])
    _ = textErr
    require(textErr == nil)
    println(text)
    let bytes = strings.Bytes("{\"ID\":2,\"id\":3,\"HTTPserver\":\"remote\",\"temp\":\"hot\",\"email\":null,\"tags\":[\"member\"],\"new\":{\"x\":[1]}}")
    require(json.UnmarshalWith(bytes[:], user, json.LowerCamel) == nil)
    require(user.ID == 3 && user.HTTPServer == "remote" && user.Temp == Celsius(21.5) && !user.Email.Set && user.Tags.len() == 1 && user.secret == "private")
    require(json.UnmarshalStrict(bytes[:], user, json.LowerCamel) != nil)
    user.Email = json.Some("now")
    let empty = strings.Bytes("{}")
    require(json.UnmarshalWith(empty[:], user, json.LowerCamel) == nil && !user.Email.Set && user.ID == 3)
    let null = strings.Bytes("null")
    require(json.Unmarshal(null[:], user) != nil)
    let snake, snakeErr = json.MarshalWith(user, json.SnakeCase)
    _ = snakeErr
    let snakeText, snakeTextErr = strings.FromBytes(snake[:])
    _ = snakeTextErr
    require(snakeErr == nil && snakeTextErr == nil && strings.Contains(snakeText, "\"http_server\"") && strings.Contains(snakeText, "\"user_id\""))
    var cases = CaseNames{ABc: 0, AbC: 0}
    require(json.UnmarshalWith(strings.Bytes("{\"abC\":2,\"ABC\":3}")[:], cases, json.LowerCamel) == nil && cases.ABc == 3 && cases.AbC == 2)
    let _, namingErr = json.MarshalWith(user, json.Naming(99))
    _ = namingErr
    require(namingErr != nil)
    println("ok")
}
"#,
        "{\"id\":1,\"httpServer\":\"local\",\"userID\":9,\"tags\":[\"admin\"],\"temp\":\"warm\"}\nok\n",
    );
}

#[test]
fn json_generated_collections_recursive_types_and_numeric_widths() {
    prints(
        r#"package main
import "std/encoding/json"
import "std/strings"
type Node struct { Name string; Children Array<Node> }
type Numbers struct { I8 int8; I16 int16; I32 int32; I64 int64; U8 uint8; U16 uint16; U32 uint32; U64 uint64; F32 float32; F64 float64 }
func require(ok bool) { if !ok { panic("JSON collection assertion") } }
func main() {
    var nodes = Array<Node>{}
    let input = strings.Bytes("[{\"Name\":\"root\",\"Children\":[{\"Name\":\"leaf\",\"Children\":[]}]}]")
    require(json.Unmarshal(input[:], nodes) == nil && nodes[0].Children[0].Name == "leaf")
    let output, err = json.Marshal(nodes[:])
    _ = err
    let text, textErr = strings.FromBytes(output[:])
    _ = textErr
    require(err == nil && textErr == nil)
    println(text)
    var mapping = map[int]Node{}
    let data = strings.Bytes("{\"10\":{\"Name\":\"ten\"},\"2\":{\"Name\":\"two\"},\"-1\":{\"Name\":\"negative\"}}")
    require(json.Unmarshal(data[:], mapping) == nil)
    let sorted, sortErr = json.Marshal(mapping)
    _ = sortErr
    let sortedText, sortedErr = strings.FromBytes(sorted[:])
    _ = sortedErr
    require(sortErr == nil && sortedErr == nil)
    println(sortedText)
    var fixed = [int; 2]{0, 0}
    require(json.Unmarshal(strings.Bytes("[1,2]")[:], fixed) == nil && fixed[1] == 2)
    require(json.Unmarshal(strings.Bytes("[1]")[:], fixed) != nil)
    require(json.Unmarshal(strings.Bytes("[1,2,3]")[:], fixed) != nil)
    var numbers = Numbers{I8: 0, I16: 0, I32: 0, I64: 0, U8: 0, U16: 0, U32: 0, U64: 0, F32: 0, F64: 0}
    let limits = strings.Bytes("{\"I8\":-128,\"I16\":-32768,\"I32\":-2147483648,\"I64\":-9223372036854775808,\"U8\":255,\"U16\":65535,\"U32\":4294967295,\"U64\":18446744073709551615,\"F32\":0.1,\"F64\":-0.0}")
    require(json.Unmarshal(limits[:], numbers) == nil && numbers.U64 == 18446744073709551615)
    let encoded, encodedErr = json.Marshal(numbers)
    _ = encodedErr
    let numericText, numericErr = strings.FromBytes(encoded[:])
    _ = numericErr
    require(encodedErr == nil && numericErr == nil)
    println(numericText)
    let invalid = Array<string>{"{\"I8\":128}", "{\"I16\":-32769}", "{\"I32\":2147483648}", "{\"I64\":9223372036854775808}", "{\"U8\":256}", "{\"U16\":-1}", "{\"U32\":4294967296}", "{\"U64\":18446744073709551616}", "{\"I8\":1e0}", "{\"F32\":1e100}", "{\"F64\":1e9999}"}
    for bad in invalid { require(json.Unmarshal(strings.Bytes(bad)[:], numbers) != nil) }
    require(json.Unmarshal(strings.Bytes("{\"01\":{}}")[:], mapping) != nil)
    println("ok")
}
"#,
        "[{\"Name\":\"root\",\"Children\":[{\"Name\":\"leaf\",\"Children\":[]}]}]\n{\"-1\":{\"Name\":\"negative\",\"Children\":[]},\"10\":{\"Name\":\"ten\",\"Children\":[]},\"2\":{\"Name\":\"two\",\"Children\":[]}}\n{\"I8\":-128,\"I16\":-32768,\"I32\":-2147483648,\"I64\":-9223372036854775808,\"U8\":255,\"U16\":65535,\"U32\":4294967295,\"U64\":18446744073709551615,\"F32\":0.1,\"F64\":-0.0}\nok\n",
    );
}

#[test]
fn json_interface_codecs_optional_collections_and_failure_cleanup() {
    prints(
        r#"package main
import "std/encoding/json"
import "std/strings"
type Box<T any> struct { Item T }
func (b Box<T>) EncodeJSON(e mut json.Encoder) { e.String("box") }
func (b mut Box<T>) DecodeJSON(d mut json.Decoder) error { let _ = d.String()?; return nil }
type Private struct { Name string; hidden channel<int> }
type Nested struct { Item Private }
func write(value json.Marshaler) (Array<byte>, error) { return json.Marshal(value) }
func read(data []byte, value mut json.Unmarshaler) error { return json.Unmarshal(data, value) }
func require(ok bool) { if !ok { panic("JSON ownership assertion") } }
func main() {
    var box = Box<int>{Item: 4}
    let data, err = write(box)
    require(err == nil)
    require(read(data[:], box) == nil)
    var owned json.Unmarshaler = Box<int>{Item: 8}
    require(json.Unmarshal(data[:], owned) == nil)
    let boxes = Array<Box<int>>{Box<int>{Item: 7}}
    let encoded, encodedErr = json.Marshal(boxes)
    require(encodedErr == nil)
    let text, textErr = strings.FromBytes(encoded[:])
    require(textErr == nil)
    println(text)
    var decoded = Array<Private>{}
    require(json.Unmarshal(strings.Bytes("[{\"Name\":\"x\"}]")[:], decoded) == nil && decoded[0].Name == "x")
    var raw = Array<int>{4}
    var view mut []int = raw[:]
    let viewData, viewErr = json.Marshal(view)
    require(viewErr == nil && json.Valid(viewData[:]))
    var optional = Array<json.Optional<int>>{}
    require(json.Unmarshal(strings.Bytes("[1,null,2]")[:], optional) == nil && optional.len() == 3 && !optional[1].Set)
    let optionData, optionErr = json.Marshal(optional)
    require(optionErr == nil)
    let optionText, optionTextErr = strings.FromBytes(optionData[:])
    require(optionTextErr == nil)
    println(optionText)
    var mapping = map[string]Array<string>{"keep": Array<string>{"old"}}
    require(json.Unmarshal(strings.Bytes("{\"new\":[\"x\"],\"new\":[\"last\"]}")[:], mapping) == nil && mapping.len() == 2)
    let mapData, mapErr = json.Marshal(mapping)
    require(mapErr == nil)
    let mapText, mapTextErr = strings.FromBytes(mapData[:])
    require(mapTextErr == nil)
    println(mapText)
    require(json.Unmarshal(strings.Bytes("{\"bad\":[\"partial\",false]}")[:], mapping) != nil)
    var nested = Nested{Item: Private{Name: "old", hidden: channel<int>()}}
    require(json.UnmarshalStrict(strings.Bytes("{\"Item\":{\"wrong\":1}}")[:], nested, json.AsWritten) != nil)
    println("ok")
}
"#,
        "[\"box\"]\n[1,null,2]\n{\"keep\":[\"old\"],\"new\":[\"last\"]}\nok\n",
    );
}

#[test]
fn json_token_call_order_zero_values_and_peek_are_checked() {
    prints(
        r#"package main
import "std/encoding/json"
import "std/strings"
func require(ok bool) { if !ok { panic("JSON token state") } }
func main() {
    let queue = channel<json.Encoder>()
    queue.close()
    var zero, _ = queue.receive()
    zero.Bool(true)
    let output, outErr = zero.Finish()
    require(outErr == nil && json.Valid(output[:]))
    let emptyValue = channel<json.Value>()
    emptyValue.close()
    let value, _ = emptyValue.receive()
    require(value.IsNull())
    let emptyBytes = Array<byte>{}
    var decoders = Array<json.Decoder>{}
    var _, zeroDecoder = decoders.pop()
    require(zeroDecoder.Finish() != nil)
    var empty = json.NewDecoder(emptyBytes[:])
    let _, emptyErr = empty.Peek()
    require(emptyErr != nil && !empty.IsNull() && empty.Null() == emptyErr)
    let input = strings.Bytes("[null,1,{\"x\":true,\"x\":false}]")
    var decoder = json.NewDecoder(input[:])
    require(decoder.BeginArray() == nil && decoder.More() && decoder.IsNull())
    require(decoder.Null() == nil)
    let kind, kindErr = decoder.Peek()
    require(kindErr == nil && kind == json.KindNumber)
    let number, intErr = decoder.Int()
    require(intErr == nil && number == 1)
    require(decoder.BeginObject() == nil)
    let first, firstErr = decoder.Key()
    require(firstErr == nil && first == "x")
    let flag, flagErr = decoder.Bool()
    require(flagErr == nil && flag)
    let second, secondErr = decoder.Key()
    require(secondErr == nil && second == "x")
    let flag2, flag2Err = decoder.Bool()
    require(flag2Err == nil && !flag2)
    require(!decoder.More() && decoder.EndObject() == nil && !decoder.More() && decoder.EndArray() == nil && decoder.Finish() == nil)
    var ordering = json.NewDecoder(input[:])
    require(!ordering.More() && ordering.Finish() != nil)
    var depth = json.NewDecoder(input[:])
    depth.SetMaxDepth(0)
    require(depth.BeginArray() != nil)
    var encoder = json.NewEncoder()
    encoder.BeginObject()
    encoder.Field("missing")
    encoder.EndObject()
    encoder.Null()
    let poisoned, poisonErr = encoder.Finish()
    require(poisonErr != nil && poisoned.len() == 0)
    var late = json.NewEncoder()
    late.Int(1)
    late.SetIndent(" ")
    let _, lateErr = late.Finish()
    require(lateErr != nil)
    var invalidIndent = json.NewEncoder()
    invalidIndent.SetIndent("x")
    let _, indentErr = invalidIndent.Finish()
    require(indentErr != nil)
    println("ok")
}
"#,
        "ok\n",
    );
}
