//! Resolution and type-checking tests (spec §3.18–3.19, §5, §6.5–6.6, §7–8,
//! §37.1). Accepted programs are paired with rejections; features outside the
//! checker's subset must be reported as unsupported, never accepted.

use zore::check::{Checked, check_file};
use zore::hir::{self, Const, ExprKind, StmtKind};
use zore::source::SourceMap;
use zore::types::TypeStore;

struct Case {
    sources: SourceMap,
    checked: Checked,
}

impl Case {
    fn new(text: &str) -> Self {
        let mut sources = SourceMap::new();
        let id = sources.add("test.ore", text.into()).unwrap();
        let checked = check_file(sources.file(id).unwrap());
        Self { sources, checked }
    }

    fn errors(&self) -> Vec<(&str, &str)> {
        self.checked
            .diagnostics
            .iter()
            .map(|d| (d.message(), self.sources.slice(d.span()).unwrap()))
            .collect()
    }

    fn package(&self) -> &hir::Package {
        match &self.checked.package {
            Some(package) => package,
            None => panic!("{:#?}", self.errors()),
        }
    }

    fn function(&self, name: &str) -> &hir::Function {
        let package = self.package();
        package
            .functions
            .iter()
            .find(|f| f.name == name)
            .unwrap_or_else(|| panic!("no function {name}"))
    }
}

/// Wrap declarations in a `main` package with a trivial entry point.
fn program(decls: &str) -> String {
    format!("package main\n\n{decls}\n\nfunc main() {{}}\n")
}

/// Wrap statements as the body of `main`.
fn body(stmts: &str) -> String {
    format!("package main\n\nfunc main() {{\n{stmts}\n}}\n")
}

fn accepts(text: &str) -> Case {
    let case = Case::new(text);
    assert!(
        case.checked.diagnostics.is_empty() && case.checked.package.is_some(),
        "{text}\n{:#?}",
        case.errors()
    );
    case
}

fn rejects(text: &str, message: &str) -> Case {
    let case = Case::new(text);
    assert!(case.checked.package.is_none(), "{text} produced HIR");
    let errors = case.errors();
    assert!(
        errors.iter().any(|(m, _)| m.contains(message)),
        "{text}\nexpected {message:?}, got {errors:#?}"
    );
    case
}

/// The single `let` initializer in `main`, folded to a constant.
fn folded(stmts: &str) -> (Const, String) {
    let case = accepts(&body(stmts));
    let package = case.package();
    let main = case.function("main");
    let StmtKind::Let { value, .. } = &main.body.stmts.last().unwrap().kind else {
        panic!("{:#?}", main.body);
    };
    let ExprKind::Const(c) = &value.kind else {
        panic!("not folded: {value:#?}");
    };
    (c.clone(), package.types.display(value.ty()).to_string())
}

#[test]
fn semantic_target_checks_and_produces_hir() {
    let mut sources = SourceMap::new();
    let id = sources.load("examples/semantic-target/main.ore").unwrap();
    let checked = check_file(sources.file(id).unwrap());
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    let package = checked.package.unwrap();
    assert_eq!(package.name, "main");
    let user = &package.structs[0];
    assert_eq!(user.name, "User");
    assert_eq!(user.fields[0].ty, TypeStore::STRING);
    assert!(package.is_copy(package.types.struct_type(zore::types::StructId(0))));

    let main = package.function(package.entry.unwrap());
    assert_eq!(main.name, "main");
    let [let_user, call] = &main.body.stmts[..] else {
        panic!("{:#?}", main.body)
    };
    let StmtKind::Let { targets, value } = &let_user.kind else {
        panic!()
    };
    let local = &main.locals[targets[0].unwrap().0 as usize];
    assert_eq!(
        (local.name.as_str(), local.kind),
        ("user", hir::LocalKind::Let)
    );
    let ExprKind::StructLit { fields, .. } = &value.kind else {
        panic!()
    };
    assert!(matches!(&fields[0].1.kind, ExprKind::Const(Const::String(s)) if s == "John"));
    let StmtKind::Expr(expr) = &call.kind else {
        panic!()
    };
    let ExprKind::Call { function, args } = &expr.kind else {
        panic!()
    };
    assert_eq!(package.function(*function).name, "greet");
    assert!(matches!(args[0].kind, ExprKind::Local(_)));

    let greet = package
        .functions
        .iter()
        .find(|f| f.name == "greet")
        .unwrap();
    let StmtKind::Expr(print) = &greet.body.stmts[0].kind else {
        panic!()
    };
    let ExprKind::Println(arg) = &print.kind else {
        panic!()
    };
    assert!(matches!(arg.kind, ExprKind::Field { .. }));
    assert_eq!(arg.ty(), TypeStore::STRING);
}

#[test]
fn hello_example_checks() {
    let mut sources = SourceMap::new();
    let id = sources.load("examples/hello/main.ore").unwrap();
    let checked = check_file(sources.file(id).unwrap());
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
}

#[test]
fn entry_point_contract() {
    accepts("package main\nfunc main() {}\n");
    accepts("package tools\nfunc helper() {}\n");
    accepts("package tools\nfunc main(x int) int { return x }\n");
    let case = rejects(
        "package main\nfunc helper() {}\n",
        "package `main` has no `main` function",
    );
    assert_eq!(case.errors()[0].1, "main");
    rejects("package main\nfunc main(x int) {}\n", "takes no parameters");
    rejects(
        "package main\nfunc main() int { return 0 }\n",
        "returns no results",
    );
    rejects("package main\nasync func main() {}\n", "cannot be `async`");
    rejects(
        "package main\nfunc main() {}\nfunc main() {}\n",
        "duplicate declaration `main`",
    );
}

#[test]
fn names_resolve_with_scopes_and_forward_references() {
    accepts(&program(
        "func first() int { return second() }
        func second() int { return limit }
        const limit = 3
        type Pair struct { Left Point; Right Point }
        type Point struct { X int; Y int }",
    ));
    accepts(&body(
        "let count = 1
        {
            let count = count + 1
            println(count)
        }
        println(count)
        let Println = 1
        println(Println)",
    ));
    // Parameters may be shadowed in a nested block, not the body block.
    accepts(&program("func f(a int) { { let a = 2; println(a) } }"));
}

#[test]
fn name_errors_are_reported() {
    rejects(&body("println(missing)"), "cannot find `missing`");
    rejects(&body("let a = a"), "cannot find `a`");
    rejects(
        &body("let count = 0\nvar count = 1"),
        "duplicate declaration `count`",
    );
    rejects(&body("let a, a = pair()"), "duplicate declaration `a`");
    rejects(
        &program("func f(a int) { let a = 2 }"),
        "duplicate declaration `a`",
    );
    rejects(
        &program("func f(a int, a int) {}"),
        "duplicate declaration `a`",
    );
    rejects(
        &program("type User struct {}\nfunc User() {}"),
        "duplicate declaration `User`",
    );
    rejects(
        &program("type User struct { Name string; Name string }"),
        "duplicate field `Name`",
    );
    rejects(&body("let println = 42"), "shadows a predeclared name");
    rejects(&body("var string = 1"), "shadows a predeclared name");
    rejects(&body("const int = 1"), "shadows a predeclared name");
    rejects(
        &program("func f(drop int) {}"),
        "shadows a predeclared name",
    );
    rejects(
        &program("type error struct {}"),
        "shadows a predeclared name",
    );
    rejects(&program("func f(x Missing) {}"), "cannot find `Missing`");
    rejects(
        &program("func helper() {}\nfunc f(x helper) {}"),
        "`helper` is not a type",
    );
    rejects(
        &body("let u = count{}\nlet count = 1"),
        "cannot find `count`",
    );
    rejects(
        &program("type Node struct { Next Node }"),
        "contains itself by value",
    );
    rejects(
        &program("type A struct { B B }\ntype B struct { A A }"),
        "contains itself by value",
    );
}

#[test]
fn bindings_types_and_defaults() {
    let case = accepts(&body(
        "let a = 1
        var b int64 = 2
        let c uint8 = 255
        let d = \"text\"
        let e = 'x'
        let f = true
        let _ = 5
        let g byte = c",
    ));
    let main = case.function("main");
    let types: Vec<String> = main
        .locals
        .iter()
        .map(|l| case.package().types.display(l.ty).to_string())
        .collect();
    assert_eq!(
        types,
        ["int64", "int64", "uint8", "string", "rune", "bool", "uint8"]
    );
    rejects(
        &body("let small uint8 = 256"),
        "`256` does not fit in `uint8`",
    );
    rejects(
        &body("let small int8 = -129"),
        "`-129` does not fit in `int8`",
    );
    rejects(
        &body("let big = 9223372036854775808"),
        "does not fit in `int64`",
    );
    rejects(
        &body("let x int32 = 1\nlet y int64 = x"),
        "expected `int64`, found `int32`",
    );
    rejects(
        &body("let s string = 1"),
        "expected `string`, found an integer constant",
    );
    rejects(
        &body("let r rune = 65"),
        "expected `rune`, found an integer constant",
    );
    rejects(
        &body("let x = 340282366920938463463374607431768211456"),
        "does not fit in `int64`",
    );
}

#[test]
fn constants_fold_exactly() {
    assert_eq!(
        folded("let x uint8 = 200 + 55"),
        (Const::Int(255), "uint8".into())
    );
    assert_eq!(
        folded("let x = 1 << 62"),
        (Const::Int(1 << 62), "int64".into())
    );
    assert_eq!(folded("let x = -7 * 3"), (Const::Int(-21), "int64".into()));
    assert_eq!(
        folded("let x = uint8(128) << 1"),
        (Const::Int(0), "uint8".into())
    );
    assert_eq!(
        folded("let x = int8(64) << 1"),
        (Const::Int(-128), "int8".into())
    );
    assert_eq!(
        folded("let x = int8(-1) << 1"),
        (Const::Int(-2), "int8".into())
    );
    assert_eq!(
        folded("let x = int8(-2) >> 1"),
        (Const::Int(-1), "int8".into())
    );
    assert_eq!(
        folded("let x = uint8(128) >> 7"),
        (Const::Int(1), "uint8".into())
    );
    assert_eq!(
        folded("let x = uint8(1) << uint64(7)"),
        (Const::Int(128), "uint8".into())
    );
    assert_eq!(
        folded("let x = int64(-7) / 3"),
        (Const::Int(-2), "int64".into())
    );
    assert_eq!(
        folded("let x = int64(-7) % 3"),
        (Const::Int(-1), "int64".into())
    );
    assert_eq!(
        folded("let x = ^uint8(5)"),
        (Const::Int(250), "uint8".into())
    );
    assert_eq!(
        folded("let x = uint8(12) & 10 | 1"),
        (Const::Int(9), "uint8".into())
    );
    assert_eq!(
        folded("let x = \"buf-\" + \"v2\""),
        (Const::String("buf-v2".into()), "string".into())
    );
    assert_eq!(
        folded("let x = \"a\" < \"b\""),
        (Const::Bool(true), "bool".into())
    );
    assert_eq!(
        folded("let x = 'a' < 'b' && !false"),
        (Const::Bool(true), "bool".into())
    );
    assert_eq!(folded("let x = 3 > 2"), (Const::Bool(true), "bool".into()));
    // Exact untyped values beyond 64 bits, reduced before typing.
    assert_eq!(
        folded("const big = 1 << 100\nlet x = big >> 98"),
        (Const::Int(4), "int64".into())
    );
    assert_eq!(folded("let x = -5 >> 1"), (Const::Int(-3), "int64".into()));
    assert_eq!(
        folded("const pageSize = 4096\nconst bufferSize = pageSize * 4\nlet x = bufferSize"),
        (Const::Int(16384), "int64".into())
    );
    assert_eq!(
        folded("const maxRetries uint8 = 5\nconst twice = maxRetries * 2\nlet x = twice"),
        (Const::Int(10), "uint8".into())
    );
}

#[test]
fn constant_errors_are_compile_time() {
    rejects(
        &body("const a uint8 = 200\nlet b = a + a"),
        "constant expression overflows `uint8`",
    );
    rejects(&body("let x = int8(-128) / -1"), "overflows `int8`");
    rejects(&body("let x = -uint8(1)"), "overflows `uint8`");
    rejects(&body("let x = int64(1) / 0"), "division by zero");
    rejects(&body("let x = int64(1) % 0"), "division by zero");
    rejects(
        &body("let x = uint8(1) << 8"),
        "shift count `8` is too large",
    );
    rejects(
        &body("let x = uint8(1) >> 8"),
        "shift count `8` is too large",
    );
    rejects(&body("let x = 1 << -1"), "shift count `-1` is negative");
    rejects(
        &body("const wide = 128 << 1\nlet small uint8 = wide"),
        "`256` does not fit in `uint8`",
    );
    rejects(
        &body("const n = 1 << 64\nlet value uint64 = n"),
        "does not fit in `uint64`",
    );
    rejects(&body("let x = uint8(300)"), "`300` does not fit in `uint8`");
    rejects(
        &body("const big uint64 = 18446744073709551615\nlet x = int64(big)"),
        "does not fit in `int64`",
    );
    // A runtime value converts with a runtime check instead (§6.6).
    accepts(&body(
        "let big uint64 = 18446744073709551615\nlet x = int64(big)",
    ));
    rejects(
        &program("const a = b\nconst b = a"),
        "defined in terms of itself",
    );
    rejects(
        &program("const a = b + 1\nconst b = c\nconst c = a"),
        "defined in terms of itself",
    );
    rejects(
        &program("func f() int { return 1 }\nconst bad = f()"),
        "not a constant expression",
    );
    rejects(
        &body("var v = 1\nconst bad = v"),
        "not a constant expression",
    );
    rejects(
        &program("type U struct {}\nconst bad = U{}"),
        "not a constant expression",
    );
    let case = rejects(
        &program("const a = b\nconst b = a"),
        "defined in terms of itself",
    );
    assert_eq!(case.errors().len(), 1, "{:#?}", case.errors());
}

#[test]
fn runtime_shift_counts_and_contextual_shift_typing() {
    accepts(&body(
        "var n = 3
        let a = 1 << n
        let b uint8 = 1 << n
        var bits uint8 = 128
        bits <<= 1
        bits >>= n",
    ));
    rejects(
        &body("var bits uint8 = 1\nbits <<= 8"),
        "out of range for `uint8`",
    );
    rejects(&body("let x = 1 << true"), "shift count must be an integer");
    rejects(&body("let x = \"a\" << 1"), "cannot be applied to `string`");
}

#[test]
fn operators_require_matching_supported_types() {
    accepts(&body(
        "var a = 1
        var b = 2
        let sum = a + b * 3 - a / b % 2
        let bits = a & b | a ^ b
        let cmp = a < b && b >= a || a != b
        let s = \"x\" + \"y\"
        let r = 'a' <= 'b'
        let t = true == false",
    ));
    rejects(
        &body("let x = int32(1) + int64(2)"),
        "expected `int32`, found `int64`",
    );
    rejects(
        &body("let x = int32(1) < uint8(2)"),
        "expected `int32`, found `uint8`",
    );
    rejects(
        &body("let x = true < false"),
        "`<` cannot be applied to `bool`",
    );
    rejects(
        &body("let x = \"a\" - \"b\""),
        "`-` cannot be applied to `string`",
    );
    rejects(
        &body("let x = 'a' + 'b'"),
        "`+` cannot be applied to `rune`",
    );
    rejects(&body("let x = !1"), "`!` requires a `bool` operand");
    rejects(
        &body("let x = -true"),
        "unary `-` cannot be applied to `bool`",
    );
    rejects(&body("let x = 1 && true"), "mismatched types");
    rejects(
        &program("type P struct { X int }\nfunc f(a P, b P) bool { return a == b }"),
        "`==` cannot be applied to `P`",
    );
}

#[test]
fn structs_fields_and_literals() {
    accepts(&program(
        "type User struct { Name string; age int }
        type Empty struct {}
        func make() User { return User{age: 1, Name: \"x\"} }
        func read(u User) string { return u.Name }
        func use() {
            var u = make()
            u.Name = \"y\"
            u.age += 1
            let e = Empty{}
            println(read(u))
            println(make().age)
        }",
    ));
    rejects(
        &program("type U struct { A int }\nfunc f() { let u = U{A: 1, B: 2} }"),
        "has no field `B`",
    );
    rejects(
        &program("type U struct { A int }\nfunc f() { let u = U{A: 1, A: 2} }"),
        "initialized more than once",
    );
    rejects(
        &program("type U struct { A int; B int }\nfunc f() { let u = U{A: 1} }"),
        "missing field `B`",
    );
    rejects(
        &program("type U struct { A int; B int; C int }\nfunc f() { let u = U{} }"),
        "missing fields `A`, `B`, `C`",
    );
    rejects(
        &program("type U struct { A int }\nfunc f() { let u = U{A: \"x\"} }"),
        "expected `int64`, found `string`",
    );
    rejects(
        &program("type U struct { A int }\nfunc f(u U) { println(u.B) }"),
        "no field `B` on type `U`",
    );
    rejects(
        &body("let x = 1\nprintln(x.field)"),
        "type `int64` has no fields",
    );
    rejects(&body("let x = count{}"), "cannot find `count`");
    rejects(&body("let v = 1\nlet x = v{}"), "`v` is not a struct type");
}

#[test]
fn calls_arguments_and_results() {
    accepts(&program(
        "func add(a int, b int) int { return a + b }
        func pair() (int, string) { return 1, \"x\" }
        func forward() (int, string) { return pair() }
        func consume(v own int) {}
        func use() {
            let n, s = pair()
            var left = 1
            var right = 2
            left, right = right, left
            var k = 0
            var t = \"\"
            k, t = forward()
            _, _ = pair()
            _ = add(n, 2)
            let total = add(left, right)
            consume(total)
            println(s)
            let wide = int64(int8(-3))
        }",
    ));
    rejects(
        &program("func add(a int, b int) int { return a + b }\nfunc f() { add(1) }"),
        "takes 2 arguments but 1 was given",
    );
    rejects(
        &program("func f(a int) {}\nfunc g() { f(\"x\") }"),
        "expected `int64`, found `string`",
    );
    rejects(
        &program(
            "func pair() (int, int) { return 1, 2 }\nfunc add(a int, b int) {}\nfunc g() { add(pair()) }",
        ),
        "takes 2 arguments but 1",
    );
    rejects(
        &program(
            "func pair() (int, int) { return 1, 2 }\nfunc one(a int) {}\nfunc g() { one(pair()) }",
        ),
        "returns 2 values",
    );
    rejects(
        &program("func greet() {}\nfunc g() { let x = greet() }"),
        "has no value",
    );
    rejects(
        &program("func greet() {}\nfunc g() { _ = greet() }"),
        "has no value",
    );
    rejects(
        &program("func pair() (int, int) { return 1, 2 }\nfunc g() { let a, b, c = pair() }"),
        "expected 3 values",
    );
    rejects(
        &program("func pair() (int, int) { return 1, 2 }\nfunc g() { var a = 0\na = pair() }"),
        "returns 2 values",
    );
    rejects(
        &program(
            "func pair() (int, string) { return 1, \"\" }\nfunc g() { var a = 0\nvar b = 0\na, b = pair() }",
        ),
        "expected `int64`, found `string`",
    );
    rejects(
        &body("var a = 1\na, a = 1, 2"),
        "assignment targets overlap",
    );
    rejects(
        &program("type P struct { X int }\nfunc g() { var p = P{X: 1}\np, p.X = P{X: 2}, 3 }"),
        "assignment targets overlap",
    );
    rejects(
        &body("var a = 1\nvar b = 2\na, b = 1"),
        "2 targets but the value provides 1",
    );
    rejects(&body("var a = 1\na = 1, 2"), "1 target but 2 values");
    rejects(&body("let x = 1\nx()"), "`x` is not a function");
    rejects(
        &program("type U struct {}\nfunc g() { U() }"),
        "constructed with `U{...}`",
    );
    rejects(
        &program("func greet() {}\nfunc g() { let f = greet }"),
        "function values are not supported",
    );
    rejects(&body("let x = int"), "`int` is a type, not a value");
    rejects(&body("let x = string(1)"), "only numeric conversions exist");
    rejects(
        &body("let x = int64(\"1\")"),
        "cannot convert `string` to `int64`",
    );
    rejects(&body("let x = int64(1, 2)"), "exactly one argument");
}

#[test]
fn println_contract() {
    accepts(&program(
        "type U struct { Name string }
        func f(u U) {
            println(\"text\")
            println(42)
            println(int8(-5))
            println(uint64(18446744073709551615))
            println(true)
            println('é')
            println(u.Name)
        }",
    ));
    rejects(&body("println()"), "exactly 1 argument but 0");
    rejects(&body("println(\"a\", \"b\")"), "exactly 1 argument but 2");
    rejects(
        &program("type U struct {}\nfunc f(u U) { println(u) }"),
        "cannot print values of type `U`",
    );
    rejects(&body("let p = println"), "`println` can only be called");
    rejects(&body("_ = println(\"x\")"), "has no value");
    rejects(&body("let x = println(\"x\")"), "has no value");
}

#[test]
fn assignment_requires_writable_places() {
    rejects(
        &body("let x = 1\nx = 2"),
        "cannot assign to immutable binding `x`",
    );
    rejects(
        &program("func f(a int) { a = 2 }"),
        "cannot assign to parameter `a`",
    );
    rejects(
        &program("type P struct { X int }\nfunc f(p P) { p.X = 1 }"),
        "cannot assign to parameter `p`",
    );
    rejects(
        &program("type P struct { X int }\nfunc f() { let p = P{X: 1}\np.X = 2 }"),
        "immutable binding `p`",
    );
    rejects(&body("const c = 1\nc = 2"), "which is a constant");
    rejects(
        &program("func g() {}\nfunc f() { g = 1 }"),
        "which is a function",
    );
    rejects(
        &program(
            "type P struct { X int }\nfunc make() P { return P{X: 1} }\nfunc f() { make().X = 1 }",
        ),
        "field of a temporary value",
    );
    rejects(
        &program("func f(a own int) { a = 1 }"),
        "assigning to `own` or `mut` parameters is not supported",
    );
    rejects(
        &body("var s = \"a\"\ns -= \"b\""),
        "`-=` cannot be applied to `string`",
    );
    rejects(
        &body("var b = true\nb += 1"),
        "`+=` cannot be applied to `bool`",
    );
    rejects(
        &body("var x uint8 = 1\nx += 300"),
        "`300` does not fit in `uint8`",
    );
    accepts(&body(
        "var s = \"a\"\ns += \"b\"\nvar n = 1\nn *= 2\nn %= 3\nn |= 4",
    ));
}

#[test]
fn control_flow_and_return_completeness() {
    accepts(&program(
        "func classify(n int) int {
            if n < 0 {
                return -1
            } else if n == 0 {
                return 0
            } else {
                return 1
            }
        }
        func forever() int {
            for {
                work()
            }
        }
        func work() {}
        func loops(limit int) {
            for var i = 0; i < limit; i += 1 {
                if i == 2 { continue }
                if i == 5 { break }
            }
            var j = 0
            for j < limit { j += 1 }
            for j = 0; j < 3; j += 1 {}
            return
        }
        func nested() int {
            {
                return 1
            }
        }",
    ));
    rejects(
        &program("func f(n int) int { if n > 0 { return 1 } }"),
        "can reach the end of its body",
    );
    rejects(
        &program("func f() int { for { break } }"),
        "can reach the end of its body",
    );
    rejects(
        &program("func f(n int) int { for n > 0 { return 1 } }"),
        "can reach the end of its body",
    );
    rejects(
        &program("func f() int { }"),
        "can reach the end of its body",
    );
    rejects(&body("break"), "`break` outside of a loop");
    rejects(&body("continue"), "`continue` outside of a loop");
    rejects(
        &body("if 1 { }"),
        "expected `bool`, found an integer constant",
    );
    rejects(&body("for \"x\" { }"), "expected `bool`, found `string`");
    rejects(
        &program("func f() int { return }"),
        "bare `return` is not allowed",
    );
    rejects(&program("func f() { return 1 }"), "does not return a value");
    rejects(
        &program("func f() (int, int) { return 1 }"),
        "expected 2 return values",
    );
    rejects(
        &program("func f() int { return 1, 2 }"),
        "expected 1 return value, found 2",
    );
    rejects(
        &program("func f() int { return \"x\" }"),
        "expected `int64`, found `string`",
    );
    rejects(
        &program(
            "func pair() (int, int) { return 1, 2 }\nfunc f() (int, int, int) { return pair() }",
        ),
        "expected 3 return values",
    );
    rejects(
        &program(
            "func pair() (int, int) { return 1, 2 }\nfunc f() (int, int) { return 1, pair() }",
        ),
        "returns 2 values",
    );
    rejects(
        &body("for var i = 0; i < 3; i += 1 {}\nprintln(i)"),
        "cannot find `i`",
    );
    rejects(
        &body("if true { let inner = 1 }\nprintln(inner)"),
        "cannot find `inner`",
    );
}

#[test]
fn unsupported_features_are_never_accepted() {
    for (text, message) in [
        (
            program("func f() error { return nil }"),
            "the `error` type is not supported",
        ),
        (body("let x = nil"), "`nil` is not supported"),
        (
            program("type U struct {}\nfunc (u U) m() {}"),
            "methods are not supported",
        ),
        (
            program("type U struct { A int }\nfunc f(u U) { u.m() }"),
            "method calls are not supported",
        ),
        (
            program("async func f() {}"),
            "`async` functions are not supported",
        ),
        (
            program("type U struct {}\nfunc f(u mut U) {}"),
            "`mut` parameters are not supported",
        ),
        (
            program("func g() int { return 1 }\nfunc f() { let x = g()? }"),
            "the `?` operator is not supported",
        ),
        (
            program("func g() int { return 1 }\nfunc f() { let x = await g() }"),
            "`await` is not supported",
        ),
        (
            "package main\nimport \"zore/fmt\"\nfunc main() {}\n".into(),
            "imports are not supported",
        ),
        (
            program("let top = 1"),
            "package-level `let` and `var` are not supported",
        ),
        (program("func f(xs Array) {}"), "`Array` is not supported"),
        (
            body("let x = clone(1)"),
            "`clone` and `drop` are not supported",
        ),
        (
            body("let r = rune(65)"),
            "rune conversions are not supported",
        ),
    ] {
        rejects(&text, message);
    }
    assert_eq!(
        folded("let x = int(7) / 2"),
        (Const::Int(3), "int64".into())
    );
}

#[test]
fn syntax_errors_stop_before_semantic_checks() {
    let case = Case::new("package main\nfunc main() {\n    let x = \n    println(missing)\n}\n");
    assert_eq!(case.errors().len(), 1, "{:#?}", case.errors());
    assert!(case.checked.package.is_none());
}

#[test]
fn diagnostics_are_reported_in_source_order() {
    let case = Case::new(&body(
        "let a uint8 = 300\nprintln(missing)\nlet b = true + 1",
    ));
    let spans: Vec<&str> = case.errors().into_iter().map(|(_, span)| span).collect();
    assert_eq!(spans, ["300", "missing", "1"]);
}

/// Untyped constants follow Go's model (§6.7).
#[test]
fn untyped_constant_kinds_follow_section_6_7() {
    let int = |v: i128| (Const::Int(v), "int64".to_string());
    let float = |v: f64| (Const::Float(v), "float64".to_string());
    assert_eq!(folded("let x = 2 + 3.0"), float(5.0));
    assert_eq!(folded("let x = 15 / 4"), int(3));
    assert_eq!(folded("let x = 15 / 4.0"), float(3.75));
    assert_eq!(folded("let x = -7 / 2"), int(-3));
    assert_eq!(folded("let x = -7 % 3"), int(-1));
    assert_eq!(folded("let x = ^1"), int(-2));
    assert_eq!(folded("let x = ^-1"), int(0));
    assert_eq!(folded("let x = 6 & 3"), int(2));
    assert_eq!(folded("let x = 6 | 3"), int(7));
    assert_eq!(folded("let x = 6 ^ 3"), int(5));
    assert_eq!(folded("let x = -4 | 1"), int(-3));
    assert_eq!(folded("let x = -4 & 7"), int(4));
    assert_eq!(folded("let x = 1 << 3.0"), int(8));
    assert_eq!(folded("let x = 1.0 << 3"), int(8));
    assert_eq!(
        folded("let x = uint8(1) << 1.0"),
        (Const::Int(2), "uint8".into())
    );
    assert_eq!(
        folded("const huge = 1 << 100\nlet x int8 = huge >> 98"),
        (Const::Int(4), "int8".into())
    );
    assert_eq!(folded("const x = 1 << 254\nlet y = x >> 253"), int(2));
    assert_eq!(
        folded("const half float64 = 3 / 2\nlet x = half"),
        float(1.0)
    );
    assert_eq!(
        folded("const exact float64 = 3 / 2.0\nlet x = exact"),
        float(1.5)
    );
    assert_eq!(
        folded("let x = 0.1 * 3 == 0.3"),
        (Const::Bool(true), "bool".into())
    );
    assert_eq!(
        folded("let x = 1 < 1.5"),
        (Const::Bool(true), "bool".into())
    );
    assert_eq!(
        folded("let x uint8 = 42.0"),
        (Const::Int(42), "uint8".into())
    );
    assert_eq!(
        folded("let x uint64 = 1e10"),
        (Const::Int(10_000_000_000), "uint64".into())
    );
    assert_eq!(
        folded("let x float32 = 0.1"),
        (Const::Float(f64::from(0.1f32)), "float32".into())
    );
    assert_eq!(
        folded("let x float32 = 2.718281828459045"),
        (
            Const::Float(f64::from("2.718281828459045".parse::<f32>().unwrap())),
            "float32".into()
        )
    );
    assert_eq!(folded("let x float64 = -1e-1000"), float(0.0));
    assert_eq!(folded("let x = 1e308 * 10 / 10"), float(1e308));
    assert_eq!(folded("let x = 1.5"), float(1.5));
    assert_eq!(folded("let x float64 = 1"), float(1.0));
    // Exact evaluation beyond 128 bits before typing.
    assert_eq!(
        folded("let x = 340282366920938463463374607431768211456 >> 120"),
        int(256)
    );
}

#[test]
fn untyped_constant_errors_follow_section_6_7() {
    rejects(&body("let x = 1 / 0"), "division by zero");
    rejects(&body("let x = 1.0 / 0.0"), "division by zero");
    rejects(&body("let x = 5 % 0"), "division by zero");
    rejects(&body("let x = 7.5 % 2"), "`%` requires integer operands");
    rejects(&body("let x = 7 % 2.0"), "`%` requires integer operands");
    rejects(&body("let x = 1.5 & 1"), "`&` requires integer operands");
    rejects(&body("let x = ^1.0"), "`^` requires integer operands");
    rejects(&body("let x = uint8(^1)"), "`-2` does not fit in `uint8`");
    rejects(&body("let x = 1 << 3.5"), "shift count must be an integer");
    rejects(
        &body("let x = 1.5 << 1"),
        "shifted constant `1.5` must be an integer",
    );
    rejects(&body("let x int = 1.1"), "is not an integer");
    rejects(&body("let x uint8 = 1024"), "does not fit in `uint8`");
    rejects(&body("let x float64 = 1e1000"), "overflows `float64`");
    rejects(&body("let x float32 = 1e39"), "overflows `float32`");
    rejects(&body("let x = 1e1000"), "overflows `float64`");
    rejects(
        &body("let r rune = 65"),
        "expected `rune`, found an integer constant",
    );
    rejects(
        &body("let s string = 1.5"),
        "expected `string`, found a floating-point constant",
    );
    rejects(&body("let x = 1 << 5000"), "4096-bit integer limit");
    rejects(
        &body("let x = 1e99999"),
        "floating-point constant is too large",
    );
    rejects(&body("let x = 1 && true"), "mismatched types");
    rejects(&body("let x = 1 || 2"), "`||` requires `bool` operands");
}

#[test]
fn float_types_and_conversions() {
    accepts(&program(
        "func area(w float64, h float64) float64 { return w * h / 2 }
        func use() {
            var x float32 = 1.5
            x += 2
            x *= x
            let y = float64(x) - 0.25
            let n = int64(y)
            let back = float32(n)
            let cmp = y < 3.0 && y != 0
            println(y)
            println(area(2, 3.5))
        }",
    ));
    assert_eq!(
        folded("let x = int64(2.0)"),
        (Const::Int(2), "int64".into())
    );
    assert_eq!(
        folded("let x = float32(0.1)"),
        (Const::Float(f64::from(0.1f32)), "float32".into())
    );
    assert_eq!(
        folded("let x = float64(float32(0.1))"),
        (Const::Float(f64::from(0.1f32)), "float64".into())
    );
    assert_eq!(
        folded("let x = float32(16777217)"),
        (Const::Float(16_777_216.0), "float32".into())
    );
    assert_eq!(
        folded("const c float64 = 2.5\nlet x = int8(c * 2)"),
        (Const::Int(5), "int8".into())
    );
    assert_eq!(
        folded("let x = -float64(0)"),
        (Const::Float(0.0), "float64".into())
    );
    assert_eq!(
        folded("const x float32 = 0.1\nlet y = x"),
        (Const::Float(f64::from(0.1f32)), "float32".into())
    );
    rejects(&body("let x = int64(2.5)"), "is not an integer");
    rejects(
        &body("const c float64 = 2.5\nlet x = int64(c)"),
        "is not an integer",
    );
    rejects(
        &body("const c float64 = 1e300\nlet x = float32(c)"),
        "overflows `float32`",
    );
    rejects(
        &body("const big float64 = 1e20\nlet x = int64(big)"),
        "does not fit in `int64`",
    );
    rejects(&body("let x = float64(1) / 0"), "division by zero");
    rejects(
        &body("let x = 1.7976931348623157e308 * float64(2)"),
        "overflows `float64`",
    );
    rejects(
        &body("var f = 1.5\nlet x = f % 2"),
        "`%` cannot be applied to `float64`",
    );
    rejects(
        &body("var f = 1.5\nlet x = ^f"),
        "unary `^` cannot be applied to `float64`",
    );
    rejects(
        &body("var f = 1.5\nlet x = f << 1"),
        "cannot be applied to `float64`",
    );
    rejects(
        &body("var f = 1.5\nlet x = 1 << f"),
        "shift count must be an integer, found `float64`",
    );
    rejects(
        &body("var f float32 = 1\nlet x float64 = f"),
        "expected `float64`, found `float32`",
    );
    rejects(
        &body("var f = 1.5\nf %= 2"),
        "`%=` cannot be applied to `float64`",
    );
    rejects(
        &body("let x = float64(\"1\")"),
        "cannot convert `string` to `float64`",
    );
    // Runtime conversions are checked at runtime, not rejected (§6.6).
    accepts(&body("var f = 2.5\nlet n = int64(f)"));
}
