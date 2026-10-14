use zore::format::format_file;
use zore::source::SourceMap;

fn formatted(text: &str) -> String {
    let mut sources = SourceMap::new();
    let id = sources.add("test.ore", text.into()).unwrap();
    let result = format_file(sources.file(id).unwrap())
        .unwrap_or_else(|errors| panic!("{text}\n{errors:#?}"));
    let second = sources.add("again.ore", result.clone()).unwrap();
    assert_eq!(
        format_file(sources.file(second).unwrap()).unwrap(),
        result,
        "formatting must be idempotent"
    );
    result
}

#[test]
fn blocks_spacing_and_mixed_binding_roles_are_readable() {
    assert_eq!(
        formatted(
            "package main\nfunc main(){\n(let x,var err)=load()\nif err!=nil{return}\n(let next,err)=load()\nprintln(x+next)\n}\n"
        ),
        "package main\n\nfunc main() {\n    (let x, var err) = load()\n    if err != nil {\n        return\n    }\n    (let next, err) = load()\n    println(x + next)\n}\n"
    );
}

#[test]
fn literals_comments_and_semicolon_sensitive_boundaries_survive() {
    let output = formatted(
        "// heading\npackage main\n\nfunc main() {\nlet raw = `one\n  two // raw` // trailing\nlet quoted = \"é\\n\"\nlet n = 1 /* first\nsecond */\nlet negative = n - -1\nprintln(raw + quoted)\nprintln(negative)\n}\n// end\n",
    );
    for expected in [
        "// heading",
        "`one\n  two // raw`",
        "\"é\\n\"",
        "/* first\nsecond */",
        "n - -1",
        "// trailing",
        "// end",
    ] {
        assert!(output.contains(expected), "{output}");
    }
    let output = formatted("package main\nfunc main(){\nfor var i=0;i<3;i+=1{println(i)}\n}\n");
    assert!(
        output.contains("for var i = 0; i < 3; i += 1 {"),
        "{output}"
    );
}

#[test]
fn generic_types_slices_and_multiline_calls_keep_their_structure() {
    let output = formatted(
        "package main\ntype Box<T any> struct{\nValues Array<T>\n}\nfunc use(values []int) {\nlet box=Box<int>{Values:Array<int>{1,2}}\nprintln(values[1:2][0])\nconsume(\nbox,\nvalues,\n)\n}\n",
    );
    assert!(output.contains("type Box<T any> struct {"), "{output}");
    assert!(
        output.contains("Box<int>{Values: Array<int>{1, 2}}"),
        "{output}"
    );
    assert!(output.contains("values[1:2][0]"), "{output}");
    assert!(
        output.contains("consume(\n        box,\n        values,\n    )"),
        "{output}"
    );
}

#[test]
fn fixed_array_type_separators_remain_inline() {
    let output =
        formatted("package main\nfunc main(){\nlet values=[int;2]{1,2}\nprintln(values[0])\n}\n");
    assert!(output.contains("[int; 2]{1, 2}"), "{output}");
    let output = formatted(
        "package main\ntype Numbers []int\ntype Holder struct { values []int }\nfunc use(values []int, edit mut []int) [int;2] {\nlet view []int=values\nreturn [int;2]{view[0],edit[0]}\n}\n",
    );
    for expected in [
        "type Numbers []int",
        "values []int",
        "edit mut []int",
        ") [int; 2]",
        "let view []int = values",
        "view[0]",
    ] {
        assert!(output.contains(expected), "{output}");
    }
}

#[test]
fn malformed_source_is_rejected_before_formatting() {
    for text in [
        "package main\nfunc main( {",
        "package main\nlet s = \"bad",
        "package main\n/* no end",
    ] {
        let mut sources = SourceMap::new();
        let id = sources.add("invalid.ore", text.into()).unwrap();
        assert!(format_file(sources.file(id).unwrap()).is_err());
    }
}

#[test]
fn declaration_comments_remain_attached_and_empty_generic_literals_are_compact() {
    let output = formatted(
        "package main\n// Runs the example.\nfunc main(){\nlet empty=Box<int>{}\nprintln(empty)\n}\n// Reads the counter.\nfunc (counter Counter) Read() (int,error){return counter.N,nil}\n",
    );
    assert!(
        output.contains("\n\n// Runs the example.\nfunc main()"),
        "{output}"
    );
    assert!(output.contains("Box<int>{}"), "{output}");
    assert!(
        output.contains("\n\n// Reads the counter.\nfunc (counter Counter) Read() (int, error)"),
        "{output}"
    );
}

#[test]
fn all_repository_source_files_format_without_token_changes() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let mut paths = Vec::new();
    fn collect(path: &std::path::Path, paths: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect(&path, paths);
            } else if path.extension().is_some_and(|ext| ext == "ore") {
                paths.push(path);
            }
        }
    }
    for directory in ["examples", "compiler-zore", "std", "benchmarks"] {
        collect(&root.join(directory), &mut paths);
    }
    assert!(paths.len() > 70);
    for path in paths {
        let text = std::fs::read_to_string(&path).unwrap();
        let result = std::panic::catch_unwind(|| formatted(&text));
        assert!(result.is_ok(), "{} failed", path.display());
        let mut sources = SourceMap::new();
        let id = sources.add(&path, text).unwrap();
        let file = sources.file(id).unwrap();
        let mut uneven = String::new();
        let mut previous_end = 0;
        for (index, token) in zore::lexer::lex(file).tokens.iter().enumerate() {
            if file.text()[previous_end..token.span.start() as usize].contains('\n')
                && !uneven.ends_with('\n')
            {
                uneven.push('\n');
            }
            match token.kind {
                zore::lexer::TokenKind::Semicolon(
                    zore::lexer::Separator::Newline | zore::lexer::Separator::Eof,
                ) => uneven.push('\n'),
                zore::lexer::TokenKind::Eof => {}
                _ => {
                    uneven.push_str(if index % 2 == 0 { "\t" } else { "   " });
                    uneven.push_str(
                        &file.text()[token.span.start() as usize..token.span.end() as usize],
                    );
                }
            }
            previous_end = token.span.end() as usize;
        }
        let result = std::panic::catch_unwind(|| formatted(&uneven));
        assert!(
            result.is_ok(),
            "{} with uneven whitespace failed",
            path.display()
        );
    }
}

#[test]
fn async_literal_formatting_preserves_nested_body_modes() {
    assert_eq!(
        formatted(
            "package main\nfunc main(){let f=async func(n int)int{let inner=func(){};return await load(n)};let t=go f(1);println(t.wait())}\n"
        ),
        "package main\n\nfunc main() {\n    let f = async func(n int) int {\n        let inner = func() {};\n        return await load(n)\n    };\n    let t = go f(1);\n    println(t.wait())\n}\n"
    );
}

#[test]
fn conditional_entry_binding_headers_format_without_changing_modes() {
    assert_eq!(
        formatted(
            "package main\nfunc main(){\nif user=users.entry(id){println(user.Name)}else if mut user=users.entry(other){user.Visits+=1}else{return}\n}\n"
        ),
        "package main\n\nfunc main() {\n    if user = users.entry(id) {\n        println(user.Name)\n    } else if mut user = users.entry(other) {\n        user.Visits += 1\n    } else {\n        return\n    }\n}\n"
    );
}

#[test]
fn lexical_task_scopes_format_as_blocks_and_keep_their_keyword() {
    assert_eq!(
        formatted("package main\nfunc main(){scope{\nlet t=go work()\nt.wait()\n}}\n"),
        "package main\n\nfunc main() {\n    scope {\n        let t = go work()\n        t.wait()\n    }\n}\n"
    );
}
