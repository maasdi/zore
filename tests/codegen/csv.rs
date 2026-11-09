use super::*;

const SHOW: &str = r##"
func visible(field string) string {
    return strings.ReplaceAll(strings.ReplaceAll(field, "\r", "\\r"), "\n", "\\n")
}

func show(reader mut csv.Reader) {
    for {
        let record, err = reader.Read()
        if err == io.EOF {
            println("EOF")
            return
        }
        if err != nil {
            println("error: " + errors.Message(err))
            return
        }
        var parts = Array<string>{}
        for field in record {
            parts.push("[" + visible(field) + "]")
        }
        println(strconv.Itoa(reader.Line()) + ": " + strings.Join(parts[:], ""))
    }
}

func parse(text string) {
    var reader = csv.NewReader(bytes.NewBufferString(text))
    show(reader)
}

func parseWith(text string, options csv.ReadOptions) {
    var reader, err = csv.NewReaderWith(bytes.NewBufferString(text), options)
    if err != nil {
        println("options: " + errors.Message(err))
        return
    }
    show(reader)
}
"##;

const IMPORTS: &str = "package main\nimport \"std/bytes\"\nimport \"std/encoding/csv\"\nimport \"std/errors\"\nimport \"std/io\"\nimport \"std/strconv\"\nimport \"std/strings\"\n";

fn reads(main: &str, expected: &str) {
    reads_with("", main, expected);
}

fn reads_with(types: &str, main: &str, expected: &str) {
    prints(
        &format!("{IMPORTS}{SHOW}{types}\nfunc main() {{\n{main}\n}}\n"),
        expected,
    );
}

#[test]
fn records_follow_quoting_line_ending_and_empty_field_rules() {
    reads(
        r##"
    parse("a,b,c\nd,e,f\n")
    parse("﻿name,age\r\nAda,36\r\n\r\n\nBob,7")
    parse("\"a,b\",\"say \"\"hi\"\"\",\"two\nlines\",\"cr\r\nlf\"\nnext,x,y,z\n")
    parse("a,\n,b\n,\n\"\"\n")
    parse("one\r\nlone\rcarriage\n x , y \n")
    parse("\"\"\"\",\"\"\n")
    parse("")
    parse("\n\r\n\n")
    parse("é,漢字,🙂\n")
"##,
        "1: [a][b][c]\n2: [d][e][f]\nEOF\n\
         1: [name][age]\n2: [Ada][36]\n5: [Bob][7]\nEOF\n\
         1: [a,b][say \"hi\"][two\\nlines][cr\\r\\nlf]\n4: [next][x][y][z]\nEOF\n\
         1: [a][]\n2: [][b]\n3: [][]\nerror: csv: line 4: record has 1 fields, want 2\n\
         1: [one]\n2: [lone\\rcarriage]\nerror: csv: line 3: record has 2 fields, want 1\n\
         1: [\"][]\nEOF\n\
         EOF\n\
         EOF\n\
         1: [é][漢字][🙂]\nEOF\n",
    );
}

#[test]
fn options_change_delimiter_comments_and_field_counts() {
    reads(
        r##"
    var semicolon = csv.DefaultReadOptions()
    semicolon.Delimiter = 59
    semicolon.Comment = 35
    parseWith("# heading\na;b,c\n#skip;me\n\"x;y\";z\n", semicolon)
    var anyCount = csv.DefaultReadOptions()
    anyCount.FieldsPerRecord = -1
    parseWith("a\nb,c\nd,e,f\n", anyCount)
    var three = csv.DefaultReadOptions()
    three.FieldsPerRecord = 3
    parseWith("a,b,c\nd,e\n", three)
    parseWith("a\n", three)
    parse("x\n\na,b\n")
    var tab = csv.DefaultReadOptions()
    tab.Delimiter = 9
    parseWith("a\tb c\t\"d\te\"\n", tab)
"##,
        "2: [a][b,c]\n4: [x;y][z]\nEOF\n\
         1: [a]\n2: [b][c]\n3: [d][e][f]\nEOF\n\
         1: [a][b][c]\nerror: csv: line 2: record has 2 fields, want 3\n\
         error: csv: line 1: record has 1 fields, want 3\n\
         1: [x]\nerror: csv: line 3: record has 2 fields, want 1\n\
         1: [a][b c][d\te]\nEOF\n",
    );
}

#[test]
fn malformed_input_names_the_line_and_column() {
    reads(
        r##"
    parse("ok,fine\nab\"c,d\n")
    parse("\"abc\"x,d\n")
    parse("a,b\n\"x\ny,\"z\"\"\n")
    parse("a,\"never closed\nmore\n")
    parse("a,b\n\"q\"\r,d\n")
    let invalid = Array<byte>{97, 44, 255, 10}
    var reader = csv.NewReader(bytes.NewBuffer(invalid[:]))
    show(reader)
    parse("a,b\nc\"\n")
"##,
        "1: [ok][fine]\nerror: csv: line 2, column 3: quote in unquoted field\n\
         error: csv: line 1, column 6: text after closing quote\n\
         1: [a][b]\nerror: csv: line 3, column 4: text after closing quote\n\
         error: csv: line 1, column 3: quoted field not closed\n\
         1: [a][b]\nerror: csv: line 2, column 4: text after closing quote\n\
         error: csv: line 1: field 2 is not valid UTF-8\n\
         1: [a][b]\nerror: csv: line 2, column 2: quote in unquoted field\n",
    );
}

#[test]
fn errors_are_sticky_and_limits_stop_reading_early() {
    reads(
        r##"
    var reader = csv.NewReader(bytes.NewBufferString("a\"b\nc\n"))
    let _, first = reader.Read()
    let _, second = reader.Read()
    println(first == second && first == error("csv: line 1, column 2: quote in unquoted field"))
    var small = csv.DefaultReadOptions()
    small.MaxRecordBytes = 8
    small.FieldsPerRecord = -1
    parseWith("12345678\n1234,678\n", small)
    parseWith("1234,6789\n", small)
    parseWith("\"1234567\"\n", small)
    parseWith("12345678,\n", small)
    var narrow = csv.DefaultReadOptions()
    narrow.MaxFields = 2
    parseWith("a,b\na,b,c\n", narrow)
    var everything = csv.NewReader(bytes.NewBufferString("a,b\nc,d\ne,f\n"))
    let rows, err = everything.ReadAll(3)
    println(err == nil && rows.len() == 3 && rows[2][1] == "f")
    var capped = csv.NewReader(bytes.NewBufferString("a\nb\nc\n"))
    let none, over = capped.ReadAll(2)
    println(over == error("csv.Reader.ReadAll: more than 2 records"))
    println(none.len() == 0)
    let _, again = capped.Read()
    println(again == error("csv.Reader.ReadAll: more than 2 records"))
    var zero = csv.NewReader(bytes.NewBufferString("a\n"))
    let _, bad = zero.ReadAll(0)
    println(bad == error("csv.Reader.ReadAll: maxRecords must be positive"))
    let _, fine = zero.ReadAll(1)
    println(fine == nil)
"##,
        "true\n\
         1: [12345678]\n2: [1234][678]\nEOF\n\
         error: csv: line 1: record longer than 8 bytes\n\
         error: csv: line 1: record longer than 8 bytes\n\
         error: csv: line 1: record longer than 8 bytes\n\
         1: [a][b]\nerror: csv: line 2: more than 2 fields\n\
         true\ntrue\ntrue\ntrue\ntrue\ntrue\n",
    );
}

#[test]
fn invalid_options_are_reported_and_kept() {
    reads(
        r##"
    let cases = Array<string>{"0", "34", "10", "13", "200"}
    for value in cases {
        var options = csv.DefaultReadOptions()
        let parsed, _ = strconv.Atoi(value)
        options.Delimiter = byte(parsed)
        parseWith("a\n", options)
    }
    var comment = csv.DefaultReadOptions()
    comment.Comment = 44
    parseWith("a\n", comment)
    comment.Comment = 34
    parseWith("a\n", comment)
    var fields = csv.DefaultReadOptions()
    fields.FieldsPerRecord = -2
    parseWith("a\n", fields)
    var bytesLimit = csv.DefaultReadOptions()
    bytesLimit.MaxRecordBytes = 0
    parseWith("a\n", bytesLimit)
    var fieldLimit = csv.DefaultReadOptions()
    fieldLimit.MaxFields = 0
    parseWith("a\n", fieldLimit)
    var broken, err = csv.NewReaderWith(bytes.NewBufferString("a\n"), fields)
    let _, later = broken.Read()
    println(err == later)
    var writeOptions = csv.DefaultWriteOptions()
    writeOptions.Delimiter = 34
    var writer, writeErr = csv.NewWriterWith(bytes.NewBufferString(""), writeOptions)
    println(errors.Message(writeErr))
    println(writer.Write(Array<string>{"a"}[:]) == error("csv.NewWriterWith: invalid Delimiter"))
    println(writer.Flush() == error("csv.NewWriterWith: invalid Delimiter"))
"##,
        "options: csv.NewReaderWith: invalid Delimiter\n\
         options: csv.NewReaderWith: invalid Delimiter\n\
         options: csv.NewReaderWith: invalid Delimiter\n\
         options: csv.NewReaderWith: invalid Delimiter\n\
         options: csv.NewReaderWith: invalid Delimiter\n\
         options: csv.NewReaderWith: invalid Comment\n\
         options: csv.NewReaderWith: invalid Comment\n\
         options: csv.NewReaderWith: FieldsPerRecord must be -1 or more\n\
         options: csv.NewReaderWith: MaxRecordBytes must be positive\n\
         options: csv.NewReaderWith: MaxFields must be positive\n\
         true\n\
         csv.NewWriterWith: invalid Delimiter\n\
         true\ntrue\n",
    );
}

const SOURCES: &str = r##"
type Trickle struct { data Array<byte>; at int }
func (t mut Trickle) Read(buf mut []byte) (int, error) {
    if t.at >= t.data.len() { return 0, io.EOF }
    buf[0] = t.data[t.at]
    t.at += 1
    return 1, nil
}

type Failing struct { data Array<byte>; done bool; withData bool }
func (f mut Failing) Read(buf mut []byte) (int, error) {
    if f.done { return 0, error("disk gone") }
    f.done = true
    for var i = 0; i < f.data.len(); i += 1 { buf[i] = f.data[i] }
    if f.withData { return f.data.len(), error("disk gone") }
    return f.data.len(), nil
}

type Liar struct { count int }
func (l mut Liar) Read(buf mut []byte) (int, error) {
    return l.count, nil
}

type Stalled struct { calls int }
func (s mut Stalled) Read(buf mut []byte) (int, error) {
    s.calls += 1
    return 0, nil
}
"##;

#[test]
fn short_and_failing_reads_keep_complete_records_only() {
    reads_with(
        SOURCES,
        r##"
    var slow = csv.NewReader(Trickle{data: strings.Bytes("﻿a,\"b\r\nc\"\r\nd,e\r\n"), at: 0})
    show(slow)
    var failsAfterRecord = csv.NewReader(Failing{data: strings.Bytes("a,b\nc,"), done: false, withData: true})
    show(failsAfterRecord)
    var failsLater = csv.NewReader(Failing{data: strings.Bytes("a,b\n"), done: false, withData: false})
    show(failsLater)
    var failsInQuotes = csv.NewReader(Failing{data: strings.Bytes("\"open"), done: false, withData: false})
    show(failsInQuotes)
    var negative = csv.NewReader(Liar{count: -1})
    show(negative)
    var tooMany = csv.NewReader(Liar{count: 100000})
    show(tooMany)
    var stalled = csv.NewReader(Stalled{calls: 0})
    show(stalled)
"##,
        "1: [a][b\\r\\nc]\n3: [d][e]\nEOF\n\
         1: [a][b]\nerror: disk gone\n\
         1: [a][b]\nerror: disk gone\n\
         error: disk gone\n\
         error: csv.Reader: invalid read count\n\
         error: csv.Reader: invalid read count\n\
         error: csv.Reader: no read progress\n",
    );
}

#[test]
fn returned_records_are_new_arrays_the_caller_owns() {
    reads(
        r##"
    var reader = csv.NewReader(bytes.NewBufferString("a,b\nc,d\n"))
    var first, _ = reader.Read()
    first[0] = "changed"
    first.push("extra")
    let second, _ = reader.Read()
    println(first.len() == 3 && second.len() == 2 && second[0] == "c")
    var kept = Array<Array<string>>{}
    kept.push(first)
    kept.push(second)
    println(kept[0][0] + kept[1][1])
"##,
        "true\nchangedd\n",
    );
}

const SINKS: &str = r##"
type Short struct { taken int; limit int }
func (s mut Short) Write(data []byte) (int, error) {
    if s.taken >= s.limit { return 0, nil }
    s.taken += 1
    return 1, nil
}

type Broken struct { calls int }
func (b mut Broken) Write(data []byte) (int, error) {
    b.calls += 1
    return 1, error("pipe closed")
}

type Liar struct {}
func (l mut Liar) Write(data []byte) (int, error) {
    return data.len() + 1, nil
}
"##;

#[test]
fn writer_quotes_only_where_needed_and_reports_sink_failures() {
    let source = format!(
        r##"package main
import "std/encoding/csv"
import "std/errors"
import "std/os"
{SINKS}
func main() {{
    var plain = csv.NewWriter(os.Stdout())
    println(plain.Write(Array<string>{{"a", "b,c", "say \"x\"", " lead", "\ttab", "two\nlines", "cr\r", "", "trail "}}[:]) == nil)
    println(plain.Write(Array<string>{{""}}[:]) == nil)
    println(plain.Write(Array<string>{{"", ""}}[:]) == nil)
    println(errors.Message(plain.Write(Array<string>{{}}[:])))
    println(plain.Buffered())
    println(plain.Flush() == nil)
    println(plain.Buffered())
    var options = csv.DefaultWriteOptions()
    options.Delimiter = 59
    options.CRLF = true
    var semicolon, err = csv.NewWriterWith(os.Stdout(), options)
    println(err == nil)
    let rows = Array<Array<string>>{{Array<string>{{"a;b", "c,d"}}, Array<string>{{"é"}}}}
    println(semicolon.WriteAll(rows[:]) == nil)
    var short = csv.NewWriter(Short{{taken: 0, limit: 3}})
    _ = short.Write(Array<string>{{"abcd"}}[:])
    println(errors.Message(short.Flush()))
    println(short.Buffered())
    println(errors.Message(short.Write(Array<string>{{"x"}}[:])))
    var broken = csv.NewWriter(Broken{{calls: 0}})
    _ = broken.Write(Array<string>{{"abc"}}[:])
    println(errors.Message(broken.Flush()))
    println(broken.Buffered())
    println(errors.Message(broken.Flush()))
    var liar = csv.NewWriter(Liar{{}})
    _ = liar.Write(Array<string>{{"abc"}}[:])
    println(errors.Message(liar.Flush()))
}}
"##
    );
    prints(
        &source,
        "true\ntrue\ntrue\ncsv.Writer.Write: empty record\n66\n\
         a,\"b,c\",\"say \"\"x\"\"\",\" lead\",\"\ttab\",\"two\nlines\",\"cr\r\",,trail \n\"\"\n,\n\
         true\n0\ntrue\n\"a;b\";c,d\r\né\r\ntrue\n\
         csv.Writer: no write progress\n2\ncsv.Writer: no write progress\n\
         pipe closed\n3\npipe closed\n\
         csv.Writer: invalid write count\n",
    );
}

#[test]
fn python_written_csv_reads_and_writes_back_unchanged() {
    let fixture = include_bytes!("fixtures/interop.csv");
    let output = run_with_input(
        r##"package main
import "std/encoding/csv"
import "std/os"

func main() {
    var options = csv.DefaultReadOptions()
    options.FieldsPerRecord = -1
    var reader, _ = csv.NewReaderWith(os.Stdin(), options)
    let records, err = reader.ReadAll(1000)
    if err != nil {
        panic("read failed")
    }
    var writerOptions = csv.DefaultWriteOptions()
    writerOptions.CRLF = true
    var writer, _ = csv.NewWriterWith(os.Stdout(), writerOptions)
    if writer.WriteAll(records[:]) != nil {
        panic("write failed")
    }
}
"##,
        fixture,
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(output.stdout == fixture, "round trip changed the bytes");
}

#[test]
fn guide_examples_compile_and_run() {
    let guide = include_str!("../../docs/csv.md");
    let blocks: Vec<_> = guide
        .split("```ore\n")
        .skip(1)
        .map(|block| block.split("```").next().unwrap())
        .collect();
    assert_eq!(blocks.len(), 1);
    let output = run_with_input(blocks[0], b"name,score\nAda,7\n\"Lovelace, A\",5\n");
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "name,score\nAda,7\n\"Lovelace, A\",5\ntotal,12\n"
    );
}

const FAILING_AFTER: &str = r##"
type FailsAfter struct { data Array<byte>; done bool }
func (f mut FailsAfter) Read(buf mut []byte) (int, error) {
    if f.done { return 0, error("disk gone") }
    f.done = true
    for var i = 0; i < f.data.len(); i += 1 { buf[i] = f.data[i] }
    return f.data.len(), nil
}

type EndsWithData struct { done bool }
func (e mut EndsWithData) Read(buf mut []byte) (int, error) {
    if e.done { return 0, io.EOF }
    e.done = true
    let text = strings.Bytes("a,b\nc,d")
    for var i = 0; i < text.len(); i += 1 { buf[i] = text[i] }
    return text.len(), io.EOF
}

func failing(data own Array<byte>) {
    var reader = csv.NewReader(FailsAfter{data: data, done: false})
    show(reader)
}
"##;

#[test]
fn a_source_error_wins_over_problems_in_the_record_it_cut_off() {
    reads_with(
        FAILING_AFTER,
        r##"
    failing(strings.Bytes("x,y\n\"a\"\r"))
    failing(Array<byte>{120, 44, 121, 10, 97, 44, 195})
    failing(strings.Bytes("x,y\na\""))
    failing(strings.Bytes("a\"b,c\nd\n"))
    var ended = csv.NewReader(EndsWithData{done: false})
    show(ended)
"##,
        "1: [x][y]\nerror: disk gone\n\
         1: [x][y]\nerror: disk gone\n\
         1: [x][y]\nerror: csv: line 2, column 2: quote in unquoted field\n\
         error: csv: line 1, column 2: quote in unquoted field\n\
         1: [a][b]\n2: [c][d]\nEOF\n",
    );
}

#[test]
fn line_reports_the_last_returned_record_and_errors_stay_sticky() {
    reads(
        r##"
    var reader = csv.NewReader(bytes.NewBufferString("a,b\nc,d\n\ne\"f\n"))
    println(reader.Line())
    let _, _ = reader.Read()
    let _, _ = reader.Read()
    let _, err = reader.Read()
    println(errors.Message(err))
    println(reader.Line())
    let bad = Array<byte>{97, 10, 255, 10}
    var utf = csv.NewReader(bytes.NewBuffer(bad[:]))
    let _, _ = utf.Read()
    let _, first = utf.Read()
    let _, second = utf.Read()
    println(first == second && first == error("csv: line 2: field 1 is not valid UTF-8"))
    var counted = csv.NewReader(bytes.NewBufferString("a,b\nc\nd,e\n"))
    let _, _ = counted.Read()
    let _, once = counted.Read()
    let _, twice = counted.Read()
    println(once == twice && once == error("csv: line 2: record has 1 fields, want 2"))
    var options = csv.DefaultReadOptions()
    options.FieldsPerRecord = -2
    var broken, made = csv.NewReaderWith(bytes.NewBufferString("a\n"), options)
    let _, all = broken.ReadAll(0)
    println(all == made && broken.Line() == 0)
"##,
        "0\ncsv: line 4, column 2: quote in unquoted field\n2\ntrue\ntrue\ntrue\n",
    );
}

#[test]
fn comments_byte_order_marks_and_exact_limits() {
    reads(
        r##"
    var comments = csv.DefaultReadOptions()
    comments.Comment = 35
    parseWith("﻿# title\r\na,b\r\n# end", comments)
    parseWith("#only", comments)
    parse("﻿a\"b\n")
    var small = csv.DefaultReadOptions()
    small.MaxRecordBytes = 6
    parseWith("\"abcd\"\n", small)
    parseWith("\"a\"\"b\"\n", small)
    parseWith("\"abcd\"\"\n", small)
"##,
        "2: [a][b]\nEOF\n\
         EOF\n\
         error: csv: line 1, column 5: quote in unquoted field\n\
         1: [abcd]\nEOF\n\
         1: [a\"b]\nEOF\n\
         error: csv: line 1: record longer than 6 bytes\n",
    );
}

#[test]
fn writer_flushes_on_its_own_and_round_trips_a_leading_byte_order_mark() {
    prints(
        r##"package main
import "std/encoding/csv"
import "std/os"
import "std/strings"

type Counter struct { writes int }
func (c mut Counter) Write(data []byte) (int, error) {
    c.writes += 1
    println(data.len())
    return data.len(), nil
}

func main() {
    var counter = csv.NewWriter(Counter{writes: 0})
    let field = strings.Repeat("x", 1000)
    var row = Array<string>{}
    for var i = 0; i < 16; i += 1 { row.push(field) }
    for var i = 0; i < 5; i += 1 { _ = counter.Write(row[:]) }
    println(counter.Buffered())
    var out = csv.NewWriter(os.Stdout())
    _ = out.Write(Array<string>{"﻿a", "b"}[:])
    _ = out.Flush()
}
"##,
        "80080\n0\n\"\u{feff}a\",b\n",
    );
}
