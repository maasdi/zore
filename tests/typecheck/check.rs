//! Resolution and type-checking tests. Accepted programs are paired with
//! rejections; features outside the checker's subset must be reported as
//! unsupported, never accepted.

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

fn accepts_move(text: &str) -> Case {
    let case = Case::new(text);
    assert!(
        case.checked.diagnostics.is_empty() && case.checked.package.is_some(),
        "{text}\n{:#?}",
        case.errors()
    );
    case
}

fn rejects_move(text: &str, message: &str) -> Case {
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
    let id = sources
        .load(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../examples/semantic-target/main.ore"
        ))
        .unwrap();
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
    let id = sources
        .load(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../examples/hello/main.ore"
        ))
        .unwrap();
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
    // A runtime value converts with a runtime check instead.
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
fn methods_declare_and_call() {
    let case = accepts(&program(
        "type User struct { Name string; age int }
        type Other struct { Name string }
        func (u User) greet() { println(u.Name) }
        func (u own User) years(extra int) int { return u.age + extra }
        func (o Other) greet() { println(o.Name) }
        func make() User { return User{Name: \"x\", age: 1} }
        func use() {
            let u = make()
            u.greet()
            println(u.years(2))
            println(make().years(3))
            Other{Name: \"y\"}.greet()
        }",
    ));
    let greet = case.function("User.greet");
    assert_eq!(greet.params.len(), 1);
    assert_eq!(case.function("User.years").params.len(), 2);
    assert_eq!(case.function("Other.greet").params.len(), 1);
    rejects(
        &program("type U struct { A int }\nfunc f(u U) { u.m() }"),
        "type `U` has no method `m`",
    );
    rejects(
        &program("type U struct { A int }\nfunc f(u U) { u.A() }"),
        "`A` is a field, not a method",
    );
    rejects(
        &program("type U struct {}\nfunc (u U) m() {}\nfunc f(u U) { u.m(1) }"),
        "`m` takes 0 arguments but 1 was given",
    );
    rejects(
        &program("type U struct {}\nfunc (u U) m(a int) {}\nfunc f(u U) { u.m(\"x\") }"),
        "expected `int64`, found `string`",
    );
    rejects(
        &program("type U struct {}\nfunc (u U) m() {}\nfunc (u U) m() {}"),
        "duplicate method `m`",
    );
    rejects(
        &program("type U struct { m int }\nfunc (u U) m() {}"),
        "duplicate member `m`",
    );
    rejects(
        &program("func (x int) m() {}"),
        "methods can be declared only on struct types",
    );
    rejects(&program("func (x Missing) m() {}"), "cannot find `Missing`");
    rejects(
        &program("type U struct { A int }\nfunc (u U) m() { u.A = 1 }"),
        "cannot assign to parameter `u`",
    );
    rejects(&body("let x = 1\nx.m()"), "type `int64` has no method `m`");
}

#[test]
fn mut_parameters_and_receivers_require_mutable_places() {
    accepts(&program(
        "type Inner struct { N int }
        type Outer struct { Inner Inner; Label string }
        func bump(i mut Inner, by int) { i.N += by }
        func set(n mut int, value int) { n = value }
        func relabel(o mut Outer) { o.Label = \"x\"; bump(o.Inner, 1) }
        func (i mut Inner) reset() { i.N = 0 }
        func (o mut Outer) clear() { o.Inner.reset(); relabel(o) }
        func use() {
            var o = Outer{Inner: Inner{N: 1}, Label: \"a\"}
            var n = 0
            bump(o.Inner, 2)
            set(n, 3)
            o.Inner.reset()
            o.clear()
            relabel(o)
            println(o.Inner.N + n)
        }",
    ));
    rejects(
        &program("type U struct { A int }\nfunc f(u mut U) {}\nfunc g() { let u = U{A: 1}\nf(u) }"),
        "cannot pass immutable binding `u` as a `mut` argument",
    );
    rejects(
        &program("type U struct { A int }\nfunc f(u mut U) {}\nfunc g(u U) { f(u) }"),
        "cannot pass shared parameter `u` as a `mut` argument",
    );
    rejects(
        &program("type U struct { A int }\nfunc f(u mut U) {}\nfunc g(u own U) { f(u) }"),
        "cannot pass `own` parameter `u` as a `mut` argument",
    );
    rejects(
        &program(
            "type P struct { X int }\ntype O struct { P P }\nfunc f(p mut P) {}\nfunc g() { let o = O{P: P{X: 1}}\nf(o.P) }",
        ),
        "cannot pass immutable binding `o` as a `mut` argument",
    );
    rejects(
        &program("func f(n mut int) {}\nfunc g() { f(1) }"),
        "a `mut` argument must be a mutable place",
    );
    rejects(
        &program("func f(n mut int) {}\nfunc h() int { return 1 }\nfunc g() { f(h()) }"),
        "a `mut` argument must be a mutable place",
    );
    rejects(
        &program(
            "type U struct { A int }\nfunc (u mut U) m() {}\nfunc g() { let u = U{A: 1}\nu.m() }",
        ),
        "cannot pass immutable binding `u` as a `mut` argument",
    );
    rejects(
        &program(
            "type U struct { A int }\nfunc f(a mut U, b mut U) {}\nfunc g() { var u = U{A: 1}\nf(u, u) }",
        ),
        "`u` is also borrowed by another argument of this call",
    );
    rejects(
        &program(
            "type U struct { A int }\nfunc f(a mut U, b U) {}\nfunc g() { var u = U{A: 1}\nf(u, u) }",
        ),
        "`u` is also borrowed by another argument of this call",
    );
    rejects(
        &program(
            "type U struct { A int }\nfunc f(a int, b mut U) {}\nfunc g() { var u = U{A: 1}\nf(u.A, u) }",
        ),
        "`u` is also borrowed by another argument of this call",
    );
    accepts(&program(
        "type U struct { A int; B int }
        func f(a mut int, b mut int) {}
        func g() { var u = U{A: 1, B: 2}\nf(u.A, u.B) }",
    ));
    rejects(
        &program("type U struct { A int }\nfunc f(u U) { u.A = 1 }"),
        "cannot assign to parameter `u`",
    );
}

#[test]
fn drop_methods_make_structs_move() {
    let case = Case::new(&program(
        "type Handle struct { id int }
        type Wrapper struct { handle Handle }
        type Point struct { X int }
        func (h mut Handle) drop() { h.id = 0 }
        func wrapped(w Wrapper) {}
        func point(p Point) {}",
    ));
    let package = case.package();
    let param_type = |name: &str| case.function(name).locals[0].ty;
    assert!(!package.is_copy(param_type("Handle.drop")));
    assert!(!package.is_copy(param_type("wrapped")));
    assert!(package.is_copy(param_type("point")));
    assert!(package.structs[0].drop.is_some());
    assert!(package.structs[1].drop.is_none());
    assert!(package.structs[2].drop.is_none());
    accepts(&program(
        "type Handle struct { id int }\nfunc (h mut Handle) drop() {}",
    ));
    rejects(
        &program("type H struct { id int }\nfunc (h H) drop() {}"),
        "`drop` must have a `mut` receiver",
    );
    rejects(
        &program("type H struct { id int }\nfunc (h own H) drop() {}"),
        "`drop` must have a `mut` receiver",
    );
    rejects(
        &program("type H struct { id int }\nfunc (h mut H) drop(reason int) {}"),
        "`drop` takes no parameters",
    );
    rejects(
        &program("type H struct { id int }\nfunc (h mut H) drop() int { return 0 }"),
        "`drop` returns no result",
    );
    rejects(
        &program(
            "type H struct { id int }\nfunc (h mut H) drop() {}\nfunc f(h mut H) { h.drop() }",
        ),
        "the `drop` method cannot be called directly",
    );
    rejects(
        &program("type H struct { drop int }\nfunc (h mut H) drop() {}"),
        "duplicate member `drop`",
    );
    rejects(
        &program("func (x int) drop() {}"),
        "methods can be declared only on struct types",
    );
    accepts(&program(
        "type P struct { X int }
        func (p P) show() { println(p.X) }
        func use(p P) { let q = p\np.show()\nq.show() }",
    ));
}

#[test]
fn move_values_transfer_and_ordinary_calls_borrow() {
    let declarations = "type Resource struct { id int }
        func (r mut Resource) drop() { println(r.id) }
        func inspect(r Resource) { println(r.id) }
        func consume(r own Resource) { println(r.id) }
        func inspect_then_consume(left Resource, right own Resource) {}";
    accepts_move(&program(&format!(
        "{declarations}\nfunc use() {{ let a = Resource{{id: 1}}\ninspect(a)\ninspect(a)\nconsume(a) }}"
    )));
    let moved = rejects_move(
        &program(&format!(
            "{declarations}\nfunc use() {{ let a = Resource{{id: 1}}\nconsume(a)\ninspect(a) }}"
        )),
        "use of moved value `a`",
    );
    let rendered = moved
        .checked
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message().contains("use of moved value `a`"))
        .unwrap()
        .render(&moved.sources)
        .unwrap();
    assert!(rendered.contains("value moved here"), "{rendered}");
    rejects_move(
        &program(&format!(
            "{declarations}\nfunc use() {{ let a = Resource{{id: 1}}\ndrop(a)\ndrop(a) }}"
        )),
        "use of moved value `a`",
    );
    rejects_move(
        &program(&format!(
            "{declarations}\nfunc invalid(r Resource) {{ consume(r) }}"
        )),
        "cannot move borrowed value `r`",
    );
    rejects_move(
        &program(&format!(
            "{declarations}\nfunc use() {{ let a = Resource{{id: 1}}\ninspect_then_consume(a, a) }}"
        )),
        "cannot move `a` while it is borrowed by this call",
    );
    accepts_move(&body("drop(5)"));
    accepts(&body("drop(5)"));
    rejects(&body("drop()"), "`drop` takes exactly 1 argument");
    accepts(&program(&format!(
        "{declarations}\nfunc use() {{ drop(Resource{{id: 1}}) }}"
    )));
}

#[test]
fn move_state_flows_through_branches_loops_and_reinitialization() {
    let resource = "type Resource struct { id int }
        func (r mut Resource) drop() {}";
    accepts_move(&program(&format!(
        "{resource}\nfunc use() {{ var a = Resource{{id: 1}}\nlet b = a\na = Resource{{id: 2}}\ndrop(a)\ndrop(b) }}"
    )));
    accepts_move(&program(&format!(
        "{resource}\nfunc use(flag bool) {{ let a = Resource{{id: 1}}\nif flag {{ drop(a) }} else {{ drop(a) }} }}"
    )));
    rejects_move(
        &program(&format!(
            "{resource}\nfunc use(flag bool) {{ let a = Resource{{id: 1}}\nif flag {{ drop(a) }}\ndrop(a) }}"
        )),
        "use of moved value `a`",
    );
    rejects_move(
        &program(&format!(
            "{resource}\nfunc use() {{ let a = Resource{{id: 1}}\nfor {{ drop(a) }} }}"
        )),
        "use of moved value `a`",
    );
    accepts_move(&program(&format!(
        "{resource}\ntype Wrapper struct {{ resource Resource }}\nfunc use(w own Wrapper) {{ let r = w.resource\ndrop(r) }}"
    )));
}

#[test]
fn partial_moves_track_fields_and_reinitialization() {
    let resource = "type Resource struct { id int }
        func (r mut Resource) drop() {}";
    let guard = format!(
        "{resource}\ntype Guard struct {{ resource Resource }}\nfunc (g mut Guard) drop() {{}}"
    );

    accepts_move(&program(&format!(
        "{resource}\ntype Wrapper struct {{ a Resource; b Resource }}\nfunc use(w own Wrapper) {{ let taken = w.a\ndrop(w.b)\ndrop(taken) }}"
    )));
    accepts_move(&program(&format!(
        "{resource}\ntype Wrapper struct {{ a Resource }}\nfunc consume(w own Wrapper) {{}}\nfunc use() {{ var w = Wrapper{{a: Resource{{id: 1}}}}\nlet taken = w.a\nw.a = Resource{{id: 2}}\ndrop(taken)\nconsume(w) }}"
    )));
    accepts_move(&program(&format!(
        "{resource}\ntype Wrapper struct {{ a Resource }}\nfunc take(r own Resource) {{}}\nfunc use() {{ var w = Wrapper{{a: Resource{{id: 1}}}}\nlet first = w.a\nw.a = Resource{{id: 2}}\ntake(first)\ntake(w.a) }}"
    )));
    accepts_move(&program(&format!(
        "{guard}\ntype Box struct {{ guard Guard }}\nfunc take_guard(g own Guard) {{}}\nfunc use(b own Box) {{ take_guard(b.guard) }}"
    )));

    rejects_move(
        &program(&format!(
            "{resource}\ntype Wrapper struct {{ a Resource; b Resource }}\nfunc inspect(w Wrapper) {{}}\nfunc use(w own Wrapper) {{ let taken = w.a\ninspect(w)\ndrop(taken)\ndrop(w.b) }}"
        )),
        "cannot use `w` as a whole value while a field is moved out",
    );
    rejects_move(
        &program(&format!(
            "{resource}\ntype Wrapper struct {{ a Resource; b Resource }}\nfunc consume(w own Wrapper) {{}}\nfunc use(w own Wrapper) {{ let taken = w.a\nconsume(w)\ndrop(taken) }}"
        )),
        "cannot use `w` as a whole value while a field is moved out",
    );
    rejects_move(
        &program(&format!(
            "{resource}\ntype Wrapper struct {{ a Resource }}\nfunc use(w own Wrapper) {{ let first = w.a\nlet second = w.a\ndrop(first)\ndrop(second) }}"
        )),
        "use of moved value `w.a`",
    );
    rejects_move(
        &program(&format!(
            "{guard}\nfunc use(g own Guard) {{ let r = g.resource\ndrop(r) }}"
        )),
        "cannot move `g.resource` out of a value with a custom `drop` method",
    );
    rejects_move(
        &program(&format!(
            "{guard}\ntype Box struct {{ guard Guard }}\nfunc use(b own Box) {{ let r = b.guard.resource\ndrop(r) }}"
        )),
        "cannot move `b.guard.resource` out of a value with a custom `drop` method",
    );
    rejects_move(
        &program(&format!(
            "{resource}\ntype Wrapper struct {{ a Resource }}\nfunc use(w Wrapper) {{ let taken = w.a\ndrop(taken) }}"
        )),
        "cannot move borrowed value `w.a`",
    );
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
        "assigning to `own` parameters is not supported",
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
fn error_results_require_one_trailing_position() {
    for declaration in [
        "func f() (error, int) { return nil, 0 }",
        "func f() (error, error) { return nil, nil }",
        "type Item struct {}\nfunc (item Item) f() (error, int) { return nil, 0 }",
    ] {
        let case = rejects(
            &program(declaration),
            "an `error` result must be the last result and appear only once",
        );
        assert!(case.errors().iter().any(|(_, span)| *span == "error"));
    }

    accepts(&program("func f() (int, error) { return 0, nil }"));
}

#[test]
fn error_values_and_explicit_discards() {
    accepts(&program(
        "func ok() error { return nil }
func fail(message string) error { return error(message) }
func pair() (int, error) { return 7, error(\"failed\") }",
    ));
    accepts(&body(
        "_ = error(\"ignored\")\nlet _ = error(\"ignored\")\nprintln(error(\"x\") == error(\"x\"))\nprintln(error(\"\") != nil)",
    ));
    accepts(&program(
        "func pair() (int, error) { return 7, nil }
func use() { let value, _ = pair(); println(value) }",
    ));
    rejects(&body("error(\"ignored\")"), "error result must be used");
    rejects(
        &program("func fail() error { return nil }\nfunc use() { fail() }"),
        "error result must be used",
    );
    rejects(
        &program("func pair() (int, error) { return 7, nil }\nfunc use() { pair() }"),
        "error result must be used",
    );
    rejects(
        &body("let err = error(\"x\")"),
        "error value in `err` may be unused",
    );
    rejects(
        &program("func use(err error) {}"),
        "error value in `err` may be unused",
    );
    rejects(&body("let value = nil"), "`nil` needs an `error` context");
    rejects(
        &body("println(error(\"x\") < error(\"y\"))"),
        "operator `<` cannot be applied to `error`",
    );
    rejects(&body("_ = error(1)"), "expected `string`");
    rejects(
        &body("const err error = nil"),
        "`nil` is not a constant expression",
    );
    rejects(
        &body("const err error = error(\"x\")"),
        "constant initializer is not a constant expression",
    );
}

#[test]
fn named_errors_require_use_on_every_path() {
    accepts(&body("let err = error(\"x\")\n_ = err"));
    accepts(&body("let err = error(\"x\")\nlet _ = err"));
    accepts(&body("let err = error(\"x\")\nvar _ = err"));
    accepts(&body("let _err = error(\"x\")\n_ = _err"));
    accepts(&program("func use(err error) { _ = err }"));
    accepts(&program(
        "func use(flag bool, err error) {
            if flag { _ = err } else { _ = err }
        }",
    ));
    accepts(&program("func use(err error) error { return err }"));
    accepts(&program(
        "func pair() (int, error) { return 1, nil }
         func use() { let value, err = pair(); _ = err; println(value) }",
    ));
    accepts(&program(
        "func read(err error) { _ = err }
         func use(err error) { read(err) }",
    ));
    accepts(&body(
        "var err = error(\"first\")
         if err != nil { println(\"found\") }
         err = error(\"second\")
         _ = err",
    ));
    accepts(&body(
        "var err = error(\"first\")
         for var i = 0; i < 2; i += 1 {
             _ = err
             err = error(\"again\")
         }
         _ = err",
    ));
    accepts(&program(
        "type Item struct { failure error }
         func use() { let item = Item{ failure: error(\"x\") }; _ = item.failure }",
    ));

    rejects(
        &body("let _err = error(\"x\")"),
        "error value in `_err` may be unused",
    );
    rejects(
        &program(
            "func pair() (int, error) { return 1, nil }
             func use() { let value, err = pair(); println(value) }",
        ),
        "error value in `err` may be unused",
    );
    let case = rejects(
        &program("func use(flag bool, err error) { if flag { _ = err } }"),
        "error value in `err` may be unused",
    );
    assert!(case.errors().iter().any(|(_, span)| *span == "err"));
    rejects(
        &body("var err = error(\"first\")\nerr = error(\"second\")\n_ = err"),
        "error value in `err` may be overwritten before use",
    );
    rejects(
        &body(
            "var err = error(\"first\")
             var flag = false
             if flag { _ = err }
             err = error(\"second\")
             _ = err",
        ),
        "error value in `err` may be overwritten before use",
    );
    rejects(
        &body(
            "var err = error(\"first\")
             for var i = 0; i < 2; i += 1 {
                 err = error(\"again\")
             }
             _ = err",
        ),
        "error value in `err` may be overwritten before use",
    );
    rejects(
        &body("{ let err = error(\"x\") }"),
        "error value in `err` may be unused",
    );
}

#[test]
fn propagation_checks_call_shape_and_return_contract() {
    accepts(&program(
        "func source(flag bool) (int, error) {
            if flag { return 0, error(\"failed\") }
            return 7, nil
         }
         func forward(flag bool) (int, error) { return source(flag)? }
         func parenthesized(flag bool) (int, error) { return (source(flag)?) }
         func add(flag bool) (int, error) {
            let value = source(flag)?
            return value + 1, nil
         }
         func check(flag bool) error {
            source(flag)?
            return nil
         }",
    ));
    accepts(&program(
        "func source() (int, string, error) { return 1, \"ok\", nil }
         func forward() (int, string, error) { return source()? }
         func bind() error { let value, text = source()?; println(value); println(text); return nil }",
    ));
    accepts(&program(
        "func fail() error { return error(\"failed\") }
         func forward() error { return fail()? }
         func constructed() error { return error(\"failed\")? }",
    ));
    rejects(
        &program("func source() error { return nil }\nfunc use() { source()? }"),
        "requires a trailing `error` result in this function",
    );
    rejects(
        &program("func source() int { return 1 }\nfunc use() error { source()?; return nil }"),
        "requires a call with a trailing `error` result",
    );
    rejects(
        &program("func use(err error) error { _ = err?; return nil }"),
        "requires a call or awaited operation",
    );
    rejects(
        &program(
            "func source() (int, error) { return 1, nil }\nfunc use() (string, error) { return source()? }",
        ),
        "does not match this function's non-error results",
    );
}

#[test]
fn unsupported_features_are_never_accepted() {
    for (text, message) in [
        (
            program("async func f() {}"),
            "`async` functions are not supported",
        ),
        (
            program("func g() int { return 1 }\nfunc f() { let x = g()? }"),
            "requires a trailing `error` result in this function",
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
        (body("let x = clone(1)"), "`clone` is not supported"),
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

/// Untyped constants follow Go's model.
#[test]
fn untyped_constant_arithmetic_and_conversion() {
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
fn untyped_constant_arithmetic_errors() {
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
    // Runtime conversions are checked at runtime, not rejected.
    accepts(&body("var f = 2.5\nlet n = int64(f)"));
}
