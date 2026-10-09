//! Accepted programs are paired with rejections; unsupported features must never be accepted.

use zore::check::{Checked, check_file};
use zore::hir::{self, Const, ExprKind, StmtKind};
use zore::resolve::LocalKind;
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

fn program(decls: &str) -> String {
    format!("package main\n\n{decls}\n\nfunc main() {{}}\n")
}

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
    assert_eq!((local.name.as_str(), local.kind), ("user", LocalKind::Let));
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
    accepts(&program("func greet() {}\nfunc g() { let f = greet\nf() }"));
    rejects(&body("let x = int"), "`int` is a type, not a value");
    rejects(
        &body("let x = string(1)"),
        "cannot convert an untyped constant to `string`",
    );
    rejects(&body("let x = bool(1)"), "only numeric conversions exist");
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
    rejects(
        &body("let value = nil"),
        "`nil` needs an `error` or `Task<...>` context",
    );
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
            program("func g() int { return 1 }\nfunc f() { let x = g()? }"),
            "requires a trailing `error` result in this function",
        ),
        (
            "package main\nimport \"zore/fmt\"\nfunc main() {}\n".into(),
            "no standard package `zore/fmt`",
        ),
        (
            program("let top = 1"),
            "package-level `let` and `var` are not supported",
        ),
        (
            program("func f(xs Array) {}"),
            "`Array` needs an element type",
        ),
        (body("let x = clone(1)"), "cannot clone a value of type"),
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

#[test]
fn fixed_arrays_literals_and_indexing() {
    accepts(&body(
        "let xs = [int; 3]{1, 2, 3}
        let first = xs[0]
        println(first)",
    ));
    accepts(&body(
        "var xs = [int; 3]{1, 2, 3}
        xs[1] = 9
        println(xs[1])",
    ));
    accepts(&body("let empty = [int; 0]{}"));
    accepts(&program(
        "type Point struct { xs [int; 2] }
        func use() { var p = Point{xs: [int; 2]{1, 2}}\np.xs[0] = 9\nprintln(p.xs[0]) }",
    ));
    rejects(
        &body("let xs = [int; 3]{1, 2}"),
        "array literal has 2 elements, expected 3",
    );
    rejects(
        &body("let xs = [int; 2]{1, 2, 3}"),
        "array literal has 3 elements, expected 2",
    );
    rejects(&body("let xs = [int; 2]{1, \"two\"}"), "mismatched types");
    rejects(
        &body("let xs = [int; 3]{1, 2, 3}\nlet y = xs[\"a\"]"),
        "array index must be an integer",
    );
    rejects(&body("let xs = 5\nlet y = xs[0]"), "cannot be indexed");
    rejects(
        &body("let n = 3\nlet xs = [int; n]{1, 2, 3}"),
        "array size must be a constant expression",
    );
    rejects(
        &body("let xs = [int; -1]{}"),
        "array size must be a nonnegative constant",
    );
    rejects(
        &body("let xs = [int; 3]{1, 2, 3}\nlet y = xs[3]"),
        "array index `3` is out of range for `[int64; 3]`",
    );
    rejects(
        &body("let xs = [int; 3]{1, 2, 3}\nlet y = xs[-1]"),
        "array index `-1` is out of range for `[int64; 3]`",
    );
    rejects(
        &body("var xs = [int; 3]{1, 2, 3}\nxs[3] = 9"),
        "array index `3` is out of range for `[int64; 3]`",
    );
    // A non-constant index stays a pure runtime check, never a compile error.
    accepts(&body(
        "let xs = [int; 3]{1, 2, 3}\nlet i = 5\nlet y = xs[i]",
    ));
}

#[test]
fn self_containing_array_structs_are_rejected() {
    rejects(
        &program("type S struct { a [S; 2] }"),
        "contains itself by value",
    );
}

#[test]
fn array_mutable_places_and_aliasing() {
    accepts(&program(
        "func edit(n mut int) {}
        func use() { var xs = [int; 2]{1, 2}\nedit(xs[0]) }",
    ));
    rejects(
        &program(
            "func edit(n mut int) {}
            func use() { let xs = [int; 2]{1, 2}\nedit(xs[0]) }",
        ),
        "cannot pass",
    );
    rejects(
        &program(
            "func edit(n mut int) {}
            func use(xs [int; 2]) { edit(xs[0]) }",
        ),
        "cannot pass",
    );
    rejects(
        &program(
            "func edit2(a mut int, b mut int) {}
            func use() { var xs = [int; 2]{1, 2}\nvar i = 0\nvar j = 1\nedit2(xs[i], xs[j]) }",
        ),
        "is also borrowed by another argument of this call",
    );
}

const SLICE_FUNCS: &str = "func inspect(items []int) int { return items[0] }
func edit(items mut []int) { items[0] = 9 }";

fn slice_program(decls: &str) -> String {
    program(&format!("{SLICE_FUNCS}\n{decls}"))
}

fn local_type(case: &Case, function: &str, local: &str) -> String {
    let ty = case
        .function(function)
        .locals
        .iter()
        .find(|l| l.name == local)
        .unwrap_or_else(|| panic!("no local {local}"))
        .ty;
    case.package().types.display(ty).to_string()
}

#[test]
fn slicing_is_shared_unless_a_mutable_view_is_requested() {
    let shared = accepts(&slice_program(
        "func use() { var data = [int; 3]{1, 2, 3}\nlet view = data[:]\n_ = view[0] }",
    ));
    assert_eq!(local_type(&shared, "use", "view"), "[]int64");
    let exclusive = accepts(&slice_program(
        "func use() { var data = [int; 3]{1, 2, 3}\nvar part mut []int = data[1:]\nedit(part) }",
    ));
    assert_eq!(local_type(&exclusive, "use", "part"), "mut []int64");
    accepts(&slice_program(
        "func use() { var data = [int; 3]{1, 2, 3}\n_ = inspect(data[:])\nedit(data[:])\n_ = inspect(data[1:3])\n_ = inspect(data[0:0]) }",
    ));
    accepts(&slice_program(
        "func half(a mut [int; 4]) mut []int { return a[:2] }
        func whole(a [int; 4]) []int { return a[:] }",
    ));
    rejects(
        &slice_program(
            "func use() { var data = [int; 3]{1, 2, 3}\nlet view = data[:]\nedit(view) }",
        ),
        "mismatched types: expected `mut []int64`, found `[]int64`",
    );
    rejects(
        &slice_program("func use(s mut []int) { _ = inspect(s) }"),
        "mismatched types: expected `[]int64`, found `mut []int64`",
    );
    accepts(&slice_program(
        "func use(s mut []int) { _ = inspect(s[:]) }",
    ));
}

#[test]
fn arrays_never_convert_to_slices_implicitly() {
    let case = rejects(
        &slice_program("func use() { let data = [int; 3]{1, 2, 3}\n_ = inspect(data) }"),
        "mismatched types: expected `[]int64`, found `[int64; 3]`",
    );
    assert!(
        case.checked.diagnostics[0]
            .notes()
            .iter()
            .any(|note| note.contains("borrow a view with `x[:]`"))
    );
}

#[test]
fn mutable_views_need_a_mutable_source() {
    rejects(
        &slice_program("func use() { let data = [int; 3]{1, 2, 3}\nedit(data[:]) }"),
        "cannot take a mutable slice of immutable binding `data`",
    );
    rejects(
        &slice_program("func use(data [int; 3]) { edit(data[:]) }"),
        "cannot take a mutable slice of shared parameter `data`",
    );
    accepts(&slice_program(
        "func use(data mut [int; 3]) { edit(data[:]) }",
    ));
    rejects(
        &slice_program("func use(s []int) { edit(s[1:]) }"),
        "cannot take a mutable slice of shared slice `s`",
    );
    accepts(&slice_program("func use(s mut []int) { edit(s[1:]) }"));
    rejects(
        &slice_program(
            "func use() { var data = [int; 3]{1, 2, 3}\nlet p mut []int = data[:]\nedit(p) }",
        ),
        "cannot pass immutable binding `p` as a `mut` argument",
    );
    accepts(&slice_program(
        "func use() { var data = [int; 3]{1, 2, 3}\nvar p mut []int = data[:]\nedit(p) }",
    ));
    accepts(&slice_program("func use(s mut []int) { edit(s) }"));
}

#[test]
fn slice_elements_are_writable_only_through_mutable_views() {
    rejects(
        &slice_program("func use(s []int) { s[0] = 1 }"),
        "cannot assign through shared slice `s`",
    );
    accepts(&slice_program(
        "func use(s mut []int) { s[0] = 1\ns[1] += 2 }",
    ));
    accepts(&slice_program(
        "func use() { var data = [int; 3]{1, 2, 3}\nlet p mut []int = data[:]\np[0] = 4 }",
    ));
    rejects(
        &slice_program("func use(s mut []int) { s = s[1:] }"),
        "cannot assign to parameter `s`",
    );
    let bump = "func bump(n mut int) { n += 1 }";
    rejects(
        &slice_program(&format!("{bump}\nfunc use(s []int) {{ bump(s[0]) }}")),
        "cannot pass an element of shared slice `s` as a `mut` argument",
    );
    accepts(&slice_program(&format!(
        "{bump}\nfunc use(s mut []int) {{ bump(s[0]) }}"
    )));
}

#[test]
fn slice_bounds_and_indices_are_checked_statically() {
    for (expr, message) in [
        (
            "data[1:4]",
            "slice bound `4` is out of range for `[int64; 3]`",
        ),
        (
            "data[-1:]",
            "slice bound `-1` is out of range for `[int64; 3]`",
        ),
        ("data[2:1]", "slice lower bound `2` exceeds upper bound `1`"),
        (
            "data[1.5:]",
            "slice bound must be an integer, found `float64`",
        ),
        (
            "data[:true]",
            "slice bound must be an integer, found `bool`",
        ),
    ] {
        rejects(
            &slice_program(&format!(
                "func use() {{ let data = [int; 3]{{1, 2, 3}}\n_ = inspect({expr}) }}"
            )),
            message,
        );
    }
    accepts(&slice_program(
        "func use() { let data = [int; 3]{1, 2, 3}\n_ = inspect(data[3:3])\n_ = inspect(data[:3]) }",
    ));
    rejects(
        &slice_program("func use(s []int) { _ = s[-1] }"),
        "slice index `-1` is out of range for `[]int64`",
    );
    rejects(
        &slice_program("func use(s []int) { _ = s[true] }"),
        "slice index must be an integer",
    );
    accepts(&slice_program(
        "func use(s []int) { var i uint8 = 1\n_ = s[i]\n_ = s[100] }",
    ));
    rejects(
        &slice_program("func use(n int) { _ = n[1:] }"),
        "type `int64` cannot be sliced",
    );
}

#[test]
fn unsupported_slice_forms_are_rejected() {
    rejects(
        &program("func f(s own []int) {}"),
        "`own []T` is not part of Zore",
    );
    rejects(
        &program("func f(s mut mut []int) {}"),
        "a `mut` mode on a slice parameter is not supported",
    );
    accepts(&program("type T struct { s mut []int }"));
    rejects(
        &program("func f(rows [mut []int; 2]) {}"),
        "a shared parameter of type `[mut []int64; 2]` cannot hold a `mut []T` view",
    );
    rejects(
        &program("func f(rows []mut []int) {}"),
        "a shared slice cannot hold `mut []T` views",
    );
    accepts(&program(
        "type T struct { s []int }\nfunc (t mut T) drop() {}",
    ));
    rejects(
        &body("let d = [int; 2]{1, 2}\nprintln(d[:])"),
        "`println` cannot print values of type `[]int64`",
    );
    rejects(
        &program("func f(a []int, b []int) bool { return a == b }"),
        "operator `==` cannot be applied to `[]int64`",
    );
    accepts(&program(
        "type Node struct { Children []Node }\nfunc f(n Node) Node { return n }",
    ));
}

#[test]
fn later_arguments_cannot_mutate_an_earlier_borrowed_argument() {
    rejects(
        &program(
            "func g(a mut [int; 2]) int { return 1 }
            func f(a [int; 2], n int) {}
            func use() { var a = [int; 2]{1, 2}\nf(a, g(a)) }",
        ),
        "`a` is borrowed by an earlier argument and mutated by a later one",
    );
    accepts(&program(
        "func g(a mut [int; 2]) int { return 1 }
        func f(n int, a [int; 2]) {}
        func use() { var a = [int; 2]{1, 2}\nf(g(a), a) }",
    ));
}

#[test]
fn dynamic_array_literals_take_any_count_of_typed_elements() {
    let case = accepts(&program(
        "func use() { let empty = Array<int>{}\nlet three = Array<int>{1, 2, 3}\n_ = three[2] }",
    ));
    assert_eq!(local_type(&case, "use", "three"), "Array<int64>");
    rejects(
        &program("func use() { let xs = Array<int>{1, \"two\"} }"),
        "mismatched types: expected `int64`, found `string`",
    );
    rejects(
        &program("func use() { let xs = Array<uint8>{256} }"),
        "does not fit",
    );
    rejects(
        &program(
            "func pair() (int, int) { return 1, 2 }\nfunc use() { let xs = Array<int>{pair()} }",
        ),
        "expected one value, but this call returns 2 values",
    );
}

#[test]
fn dynamic_arrays_are_move_values() {
    let case = accepts(&program(
        "type Bag struct { Items Array<int> }\nfunc use() { let bag = Bag{Items: Array<int>{1}}\n_ = bag.Items[0] }",
    ));
    let package = case.package();
    let bag = package.types.struct_type(zore::types::StructId(0));
    assert!(!package.is_copy(bag));
    let xs = package
        .functions
        .iter()
        .find(|f| f.name == "use")
        .unwrap()
        .locals[0]
        .ty;
    assert!(!package.is_copy(xs));
}

#[test]
fn dynamic_array_elements_follow_place_mutability() {
    accepts(&program(
        "func set(xs mut Array<int>) { xs[0] = 1 }
        func take(xs own Array<int>) { var mine = xs\nmine[0] = 2 }
        func use() { var xs = Array<int>{1, 2}\nxs[1] = 3\nxs[0] += 1\nset(xs)\ntake(xs) }",
    ));
    rejects(
        &program("func use() { let xs = Array<int>{1}\nxs[0] = 2 }"),
        "cannot assign to immutable binding `xs`",
    );
    rejects(
        &program("func use(xs Array<int>) { xs[0] = 2 }"),
        "cannot assign to parameter `xs`",
    );
    rejects(
        &program("func use(xs Array<int>) { _ = xs[-1] }"),
        "array index `-1` is out of range for `Array<int64>`",
    );
    rejects(
        &program("func use(xs Array<int>) { _ = xs[true] }"),
        "array index must be an integer",
    );
    accepts(&program("func use(xs Array<int>) { _ = xs[100] }"));
}

#[test]
fn dynamic_arrays_slice_like_fixed_arrays() {
    accepts(&slice_program(
        "func use() { var xs = Array<int>{1, 2, 3}\n_ = inspect(xs[1:])\nedit(xs[:]) }",
    ));
    rejects(
        &slice_program("func use() { let xs = Array<int>{1, 2}\nedit(xs[:]) }"),
        "cannot take a mutable slice of immutable binding `xs`",
    );
    let case = rejects(
        &slice_program("func use() { let xs = Array<int>{1}\n_ = inspect(xs) }"),
        "mismatched types: expected `[]int64`, found `Array<int64>`",
    );
    assert!(
        case.checked.diagnostics[0]
            .notes()
            .iter()
            .any(|note| note.contains("borrow a view with `x[:]`"))
    );
}

#[test]
fn unsupported_dynamic_array_forms_are_rejected() {
    rejects(
        &program("func f(xs Array<mut []int>) {}"),
        "cannot hold a `mut []T` view",
    );
    rejects(&body("let xs = Array"), "`Array` needs an element type");
    let case = rejects(
        &program("func f(xs Array<int>) { _ = xs.size() }"),
        "type `Array<int64>` has no method `size`",
    );
    assert!(
        case.checked.diagnostics[0]
            .notes()
            .iter()
            .any(|note| note.contains("`len`, `push`, and `pop`"))
    );
    accepts(&program("func f(xs Array<int>) { let ys = clone(xs) }"));
    rejects(
        &body("let xs = Array<int>{1}\nprintln(xs)"),
        "`println` cannot print values of type `Array<int64>`",
    );
    rejects(
        &program("func f(a Array<int>, b Array<int>) bool { return a == b }"),
        "operator `==` cannot be applied to `Array<int64>`",
    );
    accepts(&program("type Node struct { Kids Array<Node> }"));
    accepts(&program(
        "type Bag struct { Views Array<[]int> }\nfunc (b mut Bag) drop() {}",
    ));
}

const MAP_GUARD: &str = "type Guard struct { id int }\nfunc (g mut Guard) drop() {}";

#[test]
fn recursive_owned_types_have_finite_layout_and_move_classification() {
    let case = accepts(&program(
        "type Node struct { Children Array<Node>; Named map[string]Node }
         type Left struct { Right Right }
         type Right struct { Back [Array<Left>; 2] }
         func inspect(n Node, l Left) { let a = clone(n); let b = clone(l) }",
    ));
    let package = case.package();
    for local in &case.function("inspect").locals {
        assert!(!package.is_copy(local.ty));
        assert!(package.needs_drop(local.ty));
        assert!(!package.contains_view(local.ty));
        assert!(!package.contains_mut_view(local.ty));
        assert!(!package.drop_observes_view(local.ty));
    }
    let case = rejects(
        &program(
            "type Bad struct { Next Bad; Children Array<Bad> }\nfunc f(n Bad) { let c = clone(n) }",
        ),
        "contains itself by value",
    );
    assert!(case.errors().iter().any(|(_, at)| *at == "Bad"));
    let case = rejects(
        &program(
            "type A struct { Next B }\ntype B struct { Next [A; 1] }\nfunc f(n A) { let c = clone(n) }",
        ),
        "contains itself by value",
    );
    assert!(case.errors().iter().any(|(_, at)| *at == "A" || *at == "B"));
}

#[test]
fn recursive_clone_eligibility_checks_every_reachable_field() {
    for fields in [
        "Children Array<Node>; Guard Guard",
        "Guard Guard; Children Array<Node>",
    ] {
        rejects(
            &program(&format!(
                "{MAP_GUARD}\ntype Node struct {{ {fields} }}\nfunc f(n Node) {{ let copy = clone(n) }}"
            )),
            "type `Guard` cannot be cloned",
        );
    }
    accepts(&program(&format!(
        "{MAP_GUARD}
         func (g Guard) clone() Guard {{ return Guard{{id: g.id}} }}
         type A struct {{ Children Array<B> }}
         type B struct {{ Children map[int]A; Guard Guard }}
         func f(a A) {{ let copy = clone(a) }}"
    )));
}

#[test]
fn recursive_field_validation_waits_for_complete_types() {
    accepts(&program(
        "type Node struct { Children Array<Node>; Queue channel<Node>; Job Task<Node>; Cell Mutex<Node> }
         func f(n Node) {}",
    ));
    for container in ["channel<Node>", "Task<Node>", "Mutex<Node>"] {
        rejects(
            &program(&format!(
                "type Envelope struct {{ Value {container} }}
                 type Node struct {{ Children Array<Node>; View []int }}"
            )),
            "slices or function values",
        );
    }
    rejects(
        &program(
            "type Node struct { Children Array<Node>; View mut []int }
             func f(n Node) {}",
        ),
        "cannot hold a `mut []T` view",
    );
    let case = rejects(
        &program("type Node struct { Children map[float64]Node }"),
        "cannot be a map key",
    );
    assert_eq!(case.checked.diagnostics.len(), 1);
}

#[test]
fn map_keys_are_bool_integer_rune_or_string() {
    accepts(&program(
        "func use() { let a = map[bool]int{}\nlet b = map[uint8]int{}\nlet c = map[rune]int{}\nlet d = map[string]int{} }",
    ));
    for key in [
        "float64",
        "error",
        "[]int",
        "[int; 2]",
        "Array<int>",
        "map[int]int",
        "Guard",
    ] {
        rejects(
            &program(&format!("{MAP_GUARD}\nfunc use(m map[{key}]int) {{}}")),
            "cannot be a map key",
        );
    }
    rejects(
        &program("func f(m map[string]mut []int) {}"),
        "cannot hold a `mut []T` view",
    );
}

#[test]
fn map_literals_type_entries_and_reject_constant_duplicates() {
    let case = accepts(&program(
        "func use() { let m = map[string]int{\"Ada\": 10, \"Lin\": 20,}\nlet found, _ = m[\"Ada\"]\n_ = found }",
    ));
    assert_eq!(local_type(&case, "use", "m"), "map[string]int64");
    rejects(
        &program("func use() { let m = map[string]int{\"a\": \"b\"} }"),
        "mismatched types: expected `int64`, found `string`",
    );
    rejects(
        &program("func use() { let m = map[uint8]int{256: 1} }"),
        "does not fit",
    );
    rejects(
        &program("func use() { let m = map[string]int{\"a\": 1, \"a\": 2} }"),
        "duplicate key `\"a\"` in map literal",
    );
    accepts(&program(
        "func use(k string) { let m = map[string]int{k: 1, \"a\": 2}\nlet f, _ = m[k]\n_ = f }",
    ));
}

#[test]
fn map_lookups_always_produce_presence_then_value() {
    accepts(&program(
        "func use(m map[string]int) int { let found, value = m[\"a\"]\nlet _, other = m[\"b\"]\n_, _ = m[\"c\"]\nif found { return value }\nreturn other }",
    ));
    accepts(&program(
        "func get(m map[string]int) (bool, int) { return m[\"a\"] }",
    ));
    rejects(
        &program("func use(m map[string]int) { let value = m[\"a\"] }"),
        "map lookup produces two results",
    );
    rejects(
        &program("func take(n int) {}\nfunc use(m map[string]int) { take(m[\"a\"]) }"),
        "map lookup produces two results",
    );
    rejects(
        &program(&format!(
            "{MAP_GUARD}\nfunc use(m map[string]Guard) {{ let found, g = m[\"a\"] }}"
        )),
        "cannot look up a Move value of type `Guard`",
    );
    rejects(
        &program("func use(m map[string]error) error { let found = m[\"a\"]?\nreturn nil }"),
        "`?` requires a call or awaited operation",
    );
    rejects(
        &program("func use(m map[string]int) { m[\"a\"] }"),
        "expression is not a statement",
    );
}

#[test]
fn map_removal_needs_a_mutable_map_and_supports_propagation() {
    accepts(&program(&format!(
        "{MAP_GUARD}\nfunc use(m mut map[string]Guard) {{ let found, g = m.remove(\"a\")\n_ = found }}"
    )));
    accepts(&program(
        "func use(m mut map[string]error) (bool, error) { let found = m.remove(\"a\")?\nreturn found, nil }",
    ));
    rejects(
        &program(
            "func use(m mut map[string]error) bool { let found = m.remove(\"a\")?\nreturn found }",
        ),
        "`?` requires a trailing `error` result",
    );
    rejects(
        &program("func use() { let m = map[string]int{}\nlet f, v = m.remove(\"a\") }"),
        "cannot pass immutable binding `m` as a `mut` argument",
    );
    rejects(
        &program("func use(m map[string]int) { let f, v = m.remove(\"a\") }"),
        "cannot pass shared parameter `m` as a `mut` argument",
    );
    rejects(
        &program(
            "func use(m mut map[string]error) { let found, err = m.remove(\"a\")\n_ = found }",
        ),
        "error value in `err` may be unused before scope exit",
    );
    let case = rejects(
        &program("func use(m map[string]int) { _ = m.size() }"),
        "type `map[string]int64` has no method `size`",
    );
    assert!(
        case.checked.diagnostics[0]
            .notes()
            .iter()
            .any(|note| note.contains("`len` and `remove`"))
    );
}

#[test]
fn map_assignment_targets_a_mutable_map_and_one_entry() {
    accepts(&program(
        "func set(m mut map[string]int) { m[\"a\"] = 1 }\nfunc use() { var m = map[string]int{}\nm[\"b\"] = 2\nset(m) }",
    ));
    rejects(
        &program("func use() { let m = map[string]int{}\nm[\"a\"] = 1 }"),
        "cannot assign to immutable binding `m`",
    );
    rejects(
        &program("func use(m map[string]int) { m[\"a\"] = 1 }"),
        "cannot assign to parameter `m`",
    );
    rejects(
        &program("func use() { var m = map[string]int{}\nm[\"a\"] += 1 }"),
        "compound map assignment is not allowed",
    );
    rejects(
        &program("type P struct { X int }\nfunc use() { var m = map[string]P{}\nm[\"a\"].X = 1 }"),
        "map entries are not addressable places",
    );
    rejects(
        &program("func use() { var m = map[string]int{}\nvar x = 0\nm[\"a\"], x = 1, 2 }"),
        "map entries are not addressable places",
    );
    rejects(
        &program("func use() { var m = map[string]int{}\nm[\"a\"] = \"b\" }"),
        "mismatched types: expected `int64`, found `string`",
    );
}

#[test]
fn maps_are_move_values_without_operators() {
    let case = accepts(&program("func use() { let m = map[string]int{} }"));
    let ty = case.function("use").locals[0].ty;
    assert!(!case.package().is_copy(ty));
    rejects(
        &body("let m = map[string]int{}\nprintln(m)"),
        "`println` cannot print values of type `map[string]int64`",
    );
    rejects(
        &program("func f(a map[string]int, b map[string]int) bool { return a == b }"),
        "operator `==` cannot be applied to `map[string]int64`",
    );
    accepts(&program("type Node struct { Kids map[string]Node }"));
}

fn let_value<'a>(function: &'a hir::Function, name: &str) -> &'a hir::Expr {
    function
        .body
        .stmts
        .iter()
        .find_map(|stmt| match &stmt.kind {
            StmtKind::Let { targets, value }
                if targets
                    .iter()
                    .flatten()
                    .any(|id| function.locals[id.0 as usize].name == name) =>
            {
                Some(value)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no `let {name}`"))
}

#[test]
fn closures_become_functions_that_borrow_their_captures() {
    let case = accepts(&body(
        "let name = \"x\"
        var count = 0
        let read = func() { println(name) }
        let write = func() { count += 1 }
        read()
        write()
        let pure = func(a int, b int) int { return a + b }
        _ = pure(1, 2)",
    ));
    let main = case.function("main");
    let closure = |name: &str| match &let_value(main, name).kind {
        ExprKind::Closure {
            function, captures, ..
        } => (*function, captures.clone()),
        other => panic!("expected a closure, found {other:?}"),
    };
    let local = |name: &str| {
        let index = main.locals.iter().position(|l| l.name == name).unwrap();
        zore::resolve::LocalId(index as u32)
    };
    let (read, captures) = closure("read");
    assert_eq!(captures, [(local("name"), false)]);
    let (write, captures) = closure("write");
    assert_eq!(captures, [(local("count"), true)]);
    let (pure, captures) = closure("pure");
    assert!(captures.is_empty());
    let package = case.package();
    for (id, name) in [
        (read, "main$closure1"),
        (write, "main$closure2"),
        (pure, "main$closure3"),
    ] {
        let function = package.function(id);
        assert_eq!(function.name, name);
        assert!(function.is_closure);
    }
    let write_body = package.function(write);
    assert_eq!(write_body.captures.len(), 1);
    let capture = &write_body.locals[write_body.captures[0].0 as usize];
    assert_eq!(capture.name, "count");
    assert!(matches!(capture.kind, LocalKind::Capture(_)));
    assert_eq!(
        package
            .types
            .display(let_value(main, "pure").ty())
            .to_string(),
        "func(int64, int64) int64"
    );
    assert!(!package.is_copy(let_value(main, "pure").ty()));
}

#[test]
fn nested_captures_borrow_through_each_enclosing_closure() {
    let case = accepts(&body(
        "var total = 0
        let outer = func() {
            let inner = func() { total += 1 }
            inner()
        }
        outer()",
    ));
    let ExprKind::Closure { captures, .. } = &let_value(case.function("main"), "outer").kind else {
        panic!("expected a closure");
    };
    assert_eq!(captures.len(), 1);
    assert!(
        captures[0].1,
        "a write in the inner closure is exclusive outside too"
    );
    rejects(
        &body(
            "let total = 0
            let outer = func() {
                let inner = func() { total += 1 }
                inner()
            }
            outer()",
        ),
        "cannot assign to immutable binding `total`",
    );
}

#[test]
fn captured_bindings_keep_their_mutability_rules() {
    rejects(
        &body("let count = 0\nlet f = func() { count = 1 }\nf()"),
        "cannot assign to immutable binding `count`",
    );
    rejects(
        &program("func g(n int) { let f = func() { n += 1 }\nf() }"),
        "cannot assign to parameter `n`",
    );
    accepts(&program(
        "func g(n mut int) { let f = func() { n += 1 }\nf() }",
    ));
    rejects(
        &program(
            "func bump(x mut int) { x += 1 }\nfunc g() { let n = 0\nlet f = func() { bump(n) }\nf() }",
        ),
        "cannot pass immutable binding `n` as a `mut` argument",
    );
    accepts(&body(
        "let name = \"x\"\nlet f = func() { let name = 1\n_ = name }\nf()",
    ));
}

#[test]
fn function_types_check_calls_through_values() {
    accepts(&body(
        "let f func(int) int = func(x int) int { return x }
        let g = f
        _ = g(1)
        _ = func(x int) int { return x * 2 }(3)",
    ));
    rejects(
        &body("let f func(mut int) = func(x int) {}"),
        "expected `func(mut int64)`, found `func(int64)`",
    );
    rejects(
        &body("let f = func(x int) {}\nf(\"s\")"),
        "expected `int64`, found `string`",
    );
    rejects(
        &body("let f = func(x int) {}\nf(1, 2)"),
        "`f` takes 1 argument but 2 were given",
    );
    rejects(
        &body("let f = func() {}\nlet x = f()"),
        "this call has no value",
    );
    rejects(
        &body("let f = func() {}\nlet g = func() {}\n_ = f == g"),
        "operator `==` cannot be applied to `func()`",
    );
    rejects(
        &body("let f = func() {}\nprintln(f)"),
        "cannot print values of type `func()`",
    );
    rejects(
        &body("let f = func() error { return nil }\nf()"),
        "error result must be used or explicitly discarded",
    );
}

#[test]
fn closure_bodies_are_checked_as_functions() {
    rejects(
        &body("let f = func() int { println(\"x\") }"),
        "function literal can reach the end of its body without returning a value",
    );
    rejects(
        &body("for {\nlet f = func() { break }\n}"),
        "`break` outside of a loop",
    );
    rejects(
        &program(
            "func read() (int, error) { return 1, nil }\nfunc g() { let f = func() { _ = read()? } }",
        ),
        "`?` requires a trailing `error` result in this function",
    );
    accepts(&program(
        "func read() (int, error) { return 1, nil }
        func g() {
            let f = func() (int, error) {
                let v = read()?
                return v, nil
            }
            let _, _ = f()
        }",
    ));
    rejects(
        &body("let f = func() { await work() }"),
        "`await` is only valid inside an `async func`",
    );
    rejects(
        &body("let f = func(a int) { let a = 2 }"),
        "duplicate declaration `a`",
    );
}

#[test]
fn function_values_can_be_returned_and_stored_but_not_sliced() {
    accepts(&program(
        "type S struct { f func() }
        func make() func() { return func() {} }
        func g(a own [func(); 1], b own Array<func()>, c own map[string]func(), f own func()) {}
        func h(s mut S) { (s.f)() }",
    ));
    for decl in [
        "func g(a []func()) {}",
        "type S struct { fs mut []func() }",
        "func g() { var fs = [func(); 1]{func() {}}\nlet s = fs[:] }",
    ] {
        rejects(&program(decl), "a slice cannot hold function values");
    }
    rejects(
        &program("type S struct { f func() }\nfunc g(s S) {}"),
        "a shared parameter of type `S` cannot hold a function value",
    );
    rejects(
        &program("func g(m map[func()]int) {}"),
        "type `func()` cannot be a map key",
    );
    rejects(
        &program("func g(f mut func()) {}"),
        "a function-typed parameter cannot be `mut`",
    );
    accepts(&program("func g(f func([]int) []int) {}"));
    accepts(&program(
        "async func greet() {}\nfunc g() { let f = greet\nlet t = go f()\nt.wait() }",
    ));
    rejects(
        &program("const c = func() {}"),
        "a function literal can only appear inside a function body",
    );
}

#[test]
fn call_once_closures_are_bound_with_let_and_called_directly() {
    let job = "type Job struct { Id int }
        func (j mut Job) drop() {}
        func consume(j own Job) {}
        func run(f func()) { f() }";
    accepts(&program(&format!(
        "{job}\nfunc g() {{ let job = Job{{Id: 1}}\nlet finish = func() {{ consume(job) }}\nfinish() }}"
    )));
    for (body, message) in [
        (
            "let finish = func() { consume(job) }\nrun(finish)",
            "call-once closure `finish` can only be called directly",
        ),
        (
            "let finish = func() { consume(job) }\nlet again = finish",
            "call-once closure `finish` can only be called directly",
        ),
        (
            "let finish = func() { consume(job) }\nlet outer = func() { finish() }",
            "call-once closure `finish` can only be called directly",
        ),
        (
            "var finish = func() { consume(job) }",
            "a call-once function literal must initialize a `let` binding",
        ),
        (
            "run(func() { consume(job) })",
            "a call-once function literal must initialize a `let` binding",
        ),
    ] {
        rejects(
            &program(&format!(
                "{job}\nfunc g() {{ let job = Job{{Id: 1}}\n{body} }}"
            )),
            message,
        );
    }
}

#[test]
fn function_value_arguments_are_borrowed_exclusively() {
    rejects(
        &program("func run(f func(), g func()) {}\nfunc h() { let f = func() {}\nrun(f, f) }"),
        "`f` is also borrowed by another argument of this call",
    );
    rejects(
        &program("func run(n int, f func()) {}\nfunc h() { var n = 0\nrun(n, func() { n += 1 }) }"),
        "`n` is borrowed by an earlier argument and mutated by a later one",
    );
    accepts(&program(
        "func run(f func()) { f()\nf() }\nfunc h() { let f = func() {}\nrun(f)\nrun(f)\nrun(func() {}) }",
    ));
}

const CLONE_TYPES: &str = "type Point struct { x int }
type Res struct { id int }
func (r mut Res) drop() {}
type Tracked struct { id int }
func (t mut Tracked) drop() {}
func (t Tracked) clone() Tracked { return Tracked{id: t.id} }
type Wrap struct { r Res }
type Kept struct { t Tracked
n int }";

fn clone_program(body: &str) -> String {
    program(&format!("{CLONE_TYPES}\nfunc g() {{\n{body}\n}}"))
}

#[test]
fn clone_selects_a_custom_method_before_the_structural_default() {
    let case = accepts(&program(
        "type Point struct { x int }
        type Loud struct { x int }
        func (l Loud) clone() Loud { return Loud{x: l.x + 1} }
        func g() {
            let p = Point{x: 1}
            let l = Loud{x: 1}
            let a = clone(p)
            let b = clone(l)
        }",
    ));
    let g = case.function("g");
    assert!(matches!(let_value(g, "a").kind, ExprKind::Clone(_)));
    let ExprKind::Call { function, .. } = &let_value(g, "b").kind else {
        panic!("expected a call to the custom method")
    };
    assert_eq!(case.package().function(*function).name, "Loud.clone");
}

#[test]
fn clone_produces_a_value_of_the_argument_type() {
    let case = accepts(&clone_program(
        "let list = Array<int>{1}
        let a = clone(list)
        let m = map[string]int{\"k\": 1}
        let b = clone(m)
        let f = [int; 2]{1, 2}
        let c = clone(f)
        let k = Kept{t: Tracked{id: 1}, n: 2}
        let d = clone(k)",
    ));
    let g = case.function("g");
    let package = case.package();
    for (name, shown) in [
        ("a", "Array<int64>"),
        ("b", "map[string]int64"),
        ("c", "[int64; 2]"),
        ("d", "Kept"),
    ] {
        assert_eq!(
            package.types.display(let_value(g, name).ty()).to_string(),
            shown
        );
        assert!(matches!(let_value(g, name).kind, ExprKind::Clone(_)));
    }
}

#[test]
fn clone_needs_every_part_to_be_copy_or_clonable() {
    for (body, blocker) in [
        ("let r = Res{id: 1}\nlet c = clone(r)", "`Res`"),
        ("let w = Wrap{r: Res{id: 1}}\nlet c = clone(w)", "`Res`"),
        ("let a = Array<Res>{Res{id: 1}}\nlet c = clone(a)", "`Res`"),
        ("let a = [Res; 1]{Res{id: 1}}\nlet c = clone(a)", "`Res`"),
        (
            "let m = map[string]Res{\"k\": Res{id: 1}}\nlet c = clone(m)",
            "`Res`",
        ),
    ] {
        let case = rejects(
            &clone_program(body),
            &format!("type {blocker} cannot be cloned"),
        );
        let rendered = case.checked.diagnostics[0].render(&case.sources).unwrap();
        assert!(
            rendered.contains("func (value Res) clone() Res"),
            "{rendered}"
        );
    }
    accepts(&clone_program(
        "let a = Array<Tracked>{Tracked{id: 1}}\nlet c = clone(a)\nlet m = map[int]Tracked{1: Tracked{id: 1}}\nlet d = clone(m)",
    ));
}

#[test]
fn clone_applies_only_to_structs_arrays_and_maps() {
    for body in [
        "let c = clone(1)",
        "let c = clone(\"s\")",
        "let c = clone(true)",
        "let c = clone(error(\"e\"))",
        "var d = [int; 1]{1}\nlet s = d[:]\nlet c = clone(s)",
        "let f = func() {}\nlet c = clone(f)",
    ] {
        rejects(&clone_program(body), "cannot clone a value of type");
    }
    rejects(
        &clone_program("let a = Array<int>{1}\nlet c = clone(a, a)"),
        "`clone` takes exactly 1 argument",
    );
    rejects(
        &clone_program("let c = clone()"),
        "`clone` takes exactly 1 argument",
    );
    rejects(
        &clone_program("let f = clone"),
        "`clone` can only be called",
    );
    rejects(
        &program("func clone() {}"),
        "`clone` shadows a predeclared name",
    );
}

#[test]
fn a_custom_clone_has_a_fixed_signature() {
    for (method, message) in [
        (
            "func (b mut Bad) clone() Bad { return b }",
            "shared receiver",
        ),
        (
            "func (b own Bad) clone() Bad { return b }",
            "shared receiver",
        ),
        (
            "func (b Bad) clone(x int) Bad { return b }",
            "`clone` takes no parameters",
        ),
        (
            "func (b Bad) clone() int { return 1 }",
            "`clone` must return exactly `Bad`",
        ),
        (
            "func (b Bad) clone() {}",
            "`clone` must return exactly `Bad`",
        ),
        (
            "func (b Bad) clone() (Bad, error) { return b, nil }",
            "`clone` must return exactly `Bad`",
        ),
    ] {
        rejects(
            &program(&format!("type Bad struct {{ n int }}\n{method}")),
            message,
        );
    }
    accepts(&program(
        "type Fine struct { n int }\nfunc (f Fine) clone() Fine { return Fine{n: f.n} }",
    ));
}

#[test]
fn the_method_form_calls_only_a_custom_clone() {
    accepts(&clone_program(
        "let t = Tracked{id: 1}\nlet a = t.clone()\nlet b = clone(t)",
    ));
    let case = rejects(
        &clone_program("let p = Point{x: 1}\nlet c = p.clone()"),
        "type `Point` has no method `clone`",
    );
    let rendered = case.checked.diagnostics[0].render(&case.sources).unwrap();
    assert!(rendered.contains("write `clone(value)`"), "{rendered}");
}

const WINDOW: &str = "type Window struct { items mut []int\nlabel int }";

#[test]
fn mutable_views_can_be_struct_fields_and_fixed_array_elements() {
    accepts(&program(&format!(
        "{WINDOW}
        type Pair struct {{ left Window\nright Window }}
        func fill(w mut Window) {{ w.items[0] = 1 }}
        func take(w own Window) int {{ return w.items[0] }}
        func narrow(data mut []int) Window {{ return Window{{items: data[1:], label: 0}} }}
        func g() {{
            var data = [int; 2]{{1, 2}}
            var w = Window{{items: data[:], label: 1}}
            fill(w)
            _ = take(w)
            var other = [int; 2]{{3, 4}}
            var rows = [mut []int; 2]{{data[:], other[:]}}
            rows[1][0] = 5
        }}"
    )));
}

#[test]
fn a_shared_parameter_cannot_hold_a_nested_mutable_view() {
    for decl in [
        "func peek(w Window) {}",
        "func peek(rows [mut []int; 2]) {}",
        "func (w Window) peek() {}",
        "func peek(f func(Window)) {}",
        "func g() { let f = func(w Window) {} }",
    ] {
        rejects(
            &program(&format!("{WINDOW}\n{decl}")),
            "cannot hold a `mut []T` view",
        );
    }
    accepts(&program(&format!(
        "{WINDOW}\nfunc peek(w mut Window) {{}}\nfunc keep(w own Window) {{}}\nfunc view(s mut []int) {{}}"
    )));
}

#[test]
fn only_shared_slices_cannot_hold_mutable_views() {
    for decl in [
        "type Bag struct { view []Window }",
        "type Bag struct { rows Array<[]Window> }",
        "func g() { var d = [int; 1]{1}\nvar a = [Window; 1]{Window{items: d[:], label: 0}}\nlet s = a[:] }",
        "func g(rows []mut []int) {}",
    ] {
        rejects(
            &program(&format!("{decl}\n{WINDOW}")),
            "a shared slice cannot hold `mut []T` views",
        );
    }
    accepts(&program(&format!(
        "{WINDOW}
        type Bag struct {{ list Array<Window>\ntable map[string]Window\nrows mut []Window }}
        func g() {{ var d = [int; 1]{{1}}\nvar a = [Window; 1]{{Window{{items: d[:], label: 0}}}}\nvar s mut []Window = a[:]\n_ = s
        let b = Array<Window>{{}}\n_ = b }}"
    )));
}

#[test]
fn a_map_lookup_cannot_copy_out_a_mutable_view() {
    rejects(
        &body(
            "var d = [int; 1]{1}\nvar m = map[int]mut []int{1: d[:]}\nvar found, v = m[1]\n_ = found\n_ = v",
        ),
        "which holds a `mut []T` view; use `m.remove(key)` to take it",
    );
    accepts(&body(
        "var d = [int; 1]{1}\nvar m = map[int]mut []int{1: d[:]}\nvar found, v = m.remove(1)\nif found { v[0] = 2 }",
    ));
}

#[test]
fn a_value_holding_a_mutable_view_cannot_be_cloned() {
    rejects(
        &program(&format!(
            "{WINDOW}\nfunc g() {{ var d = [int; 1]{{1}}\nlet w = Window{{items: d[:], label: 0}}\nlet c = clone(w) }}"
        )),
        "cannot clone `Window`, which holds a `mut []T` view",
    );
}

#[test]
fn function_types_may_return_views() {
    accepts(&body(
        "var data = [int; 2]{1, 2}
        let tail = func(items []int) []int { return items[1:] }
        println(tail(data[:])[0])",
    ));
}

#[test]
fn collections_have_len_push_and_pop() {
    accepts(&program(
        "func sizes(a [int; 3], s []int, m map[string]int, xs Array<int>) int {
            return a.len() + s.len() + m.len() + xs.len()
        }
        func grow(xs mut Array<string>) { xs.push(\"a\")\nlet found, last = xs.pop()\n_ = found\n_ = last }",
    ));
    rejects(
        &body("let xs = Array<int>{}\nxs.push(1)"),
        "cannot pass immutable binding `xs` as a `mut` argument",
    );
    rejects(
        &program("func f(xs Array<int>) { let found, last = xs.pop()\n_ = found\n_ = last }"),
        "shared parameter",
    );
    rejects(
        &body("var xs = Array<int>{}\nxs.push(\"a\")"),
        "mismatched types",
    );
    rejects(
        &body("var xs = Array<int>{}\nxs.push()"),
        "`push` takes 1 argument but 0 were given",
    );
    rejects(
        &body("var xs = Array<int>{}\n_ = xs.len(1)"),
        "`len` takes 0 arguments but 1 was given",
    );
    rejects(
        &body("var xs = Array<int>{}\nlet n = xs.pop()"),
        "bind them first",
    );
    rejects(
        &body("var a = [int; 2]{1, 2}\na.push(3)"),
        "type `[int64; 2]` has no method `push`",
    );
    rejects(
        &body("let s = \"ab\"\n_ = s.size()"),
        "type `string` has no method `size`",
    );
    rejects(
        &program(
            "func use(m mut map[string]error) { var xs = Array<error>{}\nlet found, err = xs.pop()\n_ = found }",
        ),
        "error value in `err` may be unused before scope exit",
    );
}

#[test]
fn collection_loops_bind_an_index_or_key_and_an_item() {
    accepts(&program(
        "func total(a [int; 3], s []int, xs Array<int>, m map[string]int) int {
            var sum = 0
            for x in a { sum += x }
            for i, x in s { sum += i + x }
            for _, x in xs { sum += x }
            for k, v in m { if k == \"a\" { sum += v } }
            for x in xs[1:] { sum += x }
            for _ in Array<int>{1} { sum += 1 }
            return sum
        }",
    ));
    rejects(
        &body("var m = map[int]int{}\nfor v in m { _ = v }"),
        "names both the key and the value",
    );
    rejects(
        &body("for x in 3 { _ = x }"),
        "a constant cannot be looped over",
    );
    rejects(
        &body("let n = 5\nfor c in n { _ = c }"),
        "type `int64` cannot be looped over",
    );
    rejects(
        &body("var xs = Array<int>{}\nfor x in xs { x = 1 }"),
        "cannot assign to loop item `x`",
    );
    rejects(
        &body("var xs = Array<int>{}\nfor x in xs { }\n_ = x"),
        "cannot find `x` in this scope",
    );
    rejects(
        &body("var xs = Array<int>{}\nfor x, x in xs { }"),
        "duplicate declaration `x`",
    );
    rejects(
        &body("var d = [int; 1]{1}\nvar xs = Array<mut []int>{d[:]}\nfor v in xs { _ = v }"),
        "whose elements hold a `mut []T` view",
    );
    rejects(
        &body("var xs = Array<int>{}\nfor x in xs { break }\nbreak"),
        "`break` outside of a loop",
    );
    accepts(&body("var xs = Array<error>{}\nfor e in xs { }"));
}

#[test]
fn strings_have_length_index_slice_loops_and_a_rune_conversion() {
    accepts(&program(
        "func f(s string, t string, r rune) int {
            var n = s.len() + int(s[0])
            let part string = s[1:]
            let head = s[:n]
            let mid = s[1:n]
            var joined = s + t
            joined += \"!\"
            for ch in s { _ = ch }
            for i, ch in t { n += i\n_ = ch }
            _ = string(r) + string('x')
            _ = part + head + mid + joined
            return n
        }",
    ));
    for (body, message) in [
        (
            "var s = \"ab\"\ns[0] = 1",
            "cannot assign to a string index",
        ),
        (
            "var s = \"ab\"\ns[0] += 1",
            "cannot assign to a string index",
        ),
        (
            "var s = \"ab\"\nedit(s[0])",
            "cannot pass a string index as a `mut` argument",
        ),
        (
            "let s = \"ab\"\nlet x mut []byte = s[:]",
            "cannot take a mutable slice of a string",
        ),
        (
            "let s = string(65)",
            "cannot convert an untyped constant to `string`",
        ),
        (
            "let s = string(\"a\")",
            "cannot convert `string` to `string`",
        ),
        (
            "let s = string(1.5)",
            "cannot convert an untyped constant to `string`",
        ),
        (
            "let n int32 = 1\nlet s = string(n)",
            "cannot convert `int32` to `string`",
        ),
        (
            "let s = \"ab\"\n_ = s.len(1)",
            "`len` takes 0 arguments but 1 was given",
        ),
        ("let s = \"ab\"[-1:]", "is out of range for `string`"),
        ("let s = \"ab\"[1:0]", "exceeds upper bound"),
        (
            "let s = \"ab\"\nfor c in s { c = 'x' }",
            "cannot assign to loop item `c`",
        ),
        (
            "let s = \"ab\"\n_ = s[1.5]",
            "string index must be an integer",
        ),
        ("let s = \"ab\"\n_ = s[1:2:3]", "takes at most two bounds"),
    ] {
        rejects(
            &program(&format!(
                "func edit(b mut byte) {{}}\nfunc g() {{\n{body}\n}}"
            )),
            message,
        );
    }
}

#[test]
fn standard_packages_have_typed_signatures() {
    let head = "package main\nimport \"zore/strings\"\nimport \"zore/strconv\"\n";
    let accepts_main = |stmts: &str| {
        accepts(&format!("{head}\nfunc main() {{\n{stmts}\n}}\n"));
    };
    accepts_main(
        "let has bool = strings.Contains(\"abc\", \"b\")
        let index int = strings.Index(\"abc\", \"c\")
        let text string = strings.Upper(\"a\") + strings.Lower(\"B\") + strings.TrimSpace(\" c \")
        let again = strings.Repeat(text, 2) + strings.Replace(text, \"a\", \"b\")
        let parts Array<string> = strings.Split(again, \",\")
        let joined string = strings.Join(parts[:], \", \")
        let pieces = [string; 2]{\"x\", \"y\"}
        println(strings.Join(pieces[:], \"\"))
        println(strings.HasPrefix(joined, \"a\"))
        println(strings.HasSuffix(joined, \"b\"))
        let digits string = strconv.Itoa(index)
        let number, err = strconv.Atoi(digits)
        println(number)
        println(err == nil)
        let truth, failure = strconv.ParseBool(strconv.FormatBool(true))
        println(truth)
        _ = failure",
    );
    for (stmts, message) in [
        ("_ = strings.Contains(1, \"a\")", "mismatched types"),
        (
            "_ = strings.Contains(\"a\")",
            "takes 2 arguments but 1 was given",
        ),
        ("let n int = strings.Upper(\"a\")", "mismatched types"),
        (
            "let n, err = strconv.Atoi(5)\n_ = n\n_ = err",
            "mismatched types",
        ),
        ("let n = strconv.Atoi(\"5\")", "bind them first"),
        (
            "let n, err = strconv.Atoi(\"5\")\n_ = n",
            "may be unused before scope exit",
        ),
        (
            "let parts = strings.Split(\"a\", \",\")\n_ = strings.Join(parts, \",\")",
            "mismatched types",
        ),
        ("_ = strings.Repeat(\"a\", \"b\")", "mismatched types"),
        (
            "_ = strings.Nope(\"a\")",
            "package `strings` does not declare `Nope`",
        ),
        (
            "_ = strings.upper(\"a\")",
            "package `strings` does not declare `upper`",
        ),
    ] {
        rejects(
            &format!(
                "{head}\nfunc main() {{\n_ = strings.Upper(\"\") + strconv.Itoa(1)\n{stmts}\n}}\n"
            ),
            message,
        );
    }
}

const ASYNC_PRELUDE: &str = "
async func double(n int) int { return n * 2 }
async func parse(text string) (int, error) {
    if text.len() == 0 { return 0, error(\"empty\") }
    return text.len(), nil
}
func plain(n int) int { return n }
";

fn async_program(decls: &str) -> String {
    program(&format!("{ASYNC_PRELUDE}\n{decls}"))
}

#[test]
fn awaited_async_calls_are_accepted() {
    accepts(&async_program(
        "async func f() (int, error) {
            let a = await parse(\"ab\")?
            let b = await double(a)
            let c, err = await parse(\"\")
            if err != nil { return 0, err }
            _ = await double(c)
            await double(1)
            return a + b + c, nil
        }",
    ));
    let case = accepts(&async_program(
        "async func f() int { return await double(await double(1)) }",
    ));
    assert!(case.function("f").results == vec![TypeStore::INT]);
    accepts(&async_program(
        "type Counter struct { N int }
        async func (c mut Counter) bump() { c.N += 1 }
        async func f() { var c = Counter{N: 0}; await c.bump() }",
    ));
}

#[test]
fn async_calls_must_be_awaited_or_spawned() {
    let neither = "is neither awaited nor spawned";
    for stmts in [
        "double(1)",
        "let x = double(1)",
        "plain(double(1))",
        "let x = await plain(double(1))",
        "let x = (double(1))",
    ] {
        rejects(
            &async_program(&format!("async func f() {{ {stmts} }}")),
            neither,
        );
    }
    rejects(&async_program("func f() { double(1) }"), neither);
    rejects(
        &async_program("async func f() { let x, err = parse(\"a\") }"),
        neither,
    );
}

#[test]
fn await_is_valid_only_in_async_bodies() {
    let outside = "`await` is only valid inside an `async func`";
    let case = rejects(
        &async_program("func f() int { return await double(1) }"),
        outside,
    );
    assert!(
        case.errors()
            .iter()
            .any(|(m, _)| m.contains("`f` is not async"))
    );
    rejects(
        &async_program("async func f() { let g = func() int { return await double(1) }\n_ = g() }"),
        "this function literal is not async",
    );
}

#[test]
fn await_needs_an_async_call() {
    let message = "`await` needs a call to an `async func`";
    rejects(
        &async_program("async func f() int { return await plain(1) }"),
        message,
    );
    rejects(
        &async_program("async func f() int { let g = func() int { return 1 }\nreturn await g() }"),
        message,
    );
    rejects(
        &async_program("async func f() int { let n = 1\nreturn await n }"),
        "`await` needs a call to an `async func` or a `Task<...>` value",
    );
}

#[test]
fn async_entry_point_is_rejected() {
    rejects(
        "package main\nasync func main() {}\n",
        "the entry point `main` cannot be `async`",
    );
}

#[test]
fn awaited_errors_follow_error_rules() {
    rejects(
        &async_program("async func f() { await parse(\"a\") }"),
        "error result must be used or explicitly discarded",
    );
    rejects(
        &async_program("async func f() { let n = await parse(\"a\")? }"),
        "`?` requires a trailing `error` result in this function",
    );
}

const TASK_PRELUDE: &str = "
func compute() int { return 1 }
func load() (int, error) { return 1, nil }
func log(message string) {}
func bump(n mut int) { n += 1 }
func look(values Array<int>) {}
func eat(values own Array<int>) {}
func view(values []int) {}
async func fetch(id int) (string, error) { return \"x\", nil }
type Counter struct { N int }
func (c Counter) read() int { return c.N }
";

fn task_program(decls: &str) -> String {
    program(&format!("{TASK_PRELUDE}\n{decls}"))
}

fn task_body(stmts: &str) -> String {
    task_program(&format!("func entry() {{\n{stmts}\n}}"))
}

#[test]
fn spawned_calls_have_task_types() {
    let case = accepts(&task_body(
        "let a = go compute()
        let b = go load()
        let c = go log(\"x\")
        let d = go fetch(1)
        let e Task<int, error> = go load()
        let f Task = go log(\"y\")
        let g Task<string, error> = go fetch(2)
        let counter = Counter{N: 1}
        let h = go counter.read()
        println(a.wait())
        let v, err = b.wait()
        _ = err
        c.wait()
        let s, serr = d.wait()
        _ = serr
        let e1, e2 = e.wait()
        _ = e2
        f.wait()
        let t1, t2 = g.wait()
        _ = t2
        println(h.wait())",
    ));
    let entry = case.function("entry");
    let types: Vec<String> = entry
        .locals
        .iter()
        .take(4)
        .map(|local| case.package().types.display(local.ty).to_string())
        .collect();
    assert_eq!(
        types,
        [
            "Task<int64>",
            "Task<int64, error>",
            "Task",
            "Task<string, error>"
        ]
    );
}

#[test]
fn spawn_statements_detach_without_a_handle() {
    accepts(&task_body("go compute()\ngo load()\ngo log(\"x\")"));
}

#[test]
fn task_annotations_must_mirror_the_result_list() {
    rejects(
        &task_body("let t Task<int> = go load()"),
        "mismatched types",
    );
    rejects(
        &task_body("let t Task<int, error> = go compute()"),
        "mismatched types",
    );
    rejects(
        &task_body("let t Task<int> = go log(\"x\")"),
        "mismatched types",
    );
    rejects(
        &task_program("func f(t Task<error, int>) {}"),
        "`error` can only be the last result",
    );
    rejects(
        &task_program("func f(t Task<[]int>) {}"),
        "a task cannot return slices or function values",
    );
}

#[test]
fn tasks_are_move_values() {
    rejects(
        &task_body("let t = go compute()\nlet a = t.wait()\nlet b = t.wait()"),
        "use of moved value `t`",
    );
    rejects(
        &task_body("let t = go compute()\nlet u = t\nlet v = t.wait()"),
        "use of moved value `t`",
    );
    rejects(
        &task_program(
            "async func f() int {
                let t = go compute()
                let a = await t
                let b = await t
                return a + b
            }",
        ),
        "use of moved value `t`",
    );
    rejects(
        &task_body("let t = go compute()\nlet u = clone(t)"),
        "cannot clone",
    );
    rejects(
        &task_body("let t = go compute()\n_ = t == t"),
        "cannot be applied",
    );
    accepts(&task_program(
        "func take(t own Task<int>) int { return t.wait() }
        func f() int {
            let t = go compute()
            return take(t)
        }",
    ));
    accepts(&task_program(
        "func f() { var tasks = Array<Task<int>>{}
            tasks.push(go compute())
            let found, t = tasks.pop()
            if found { _ = t.wait() }
        }",
    ));
}

#[test]
fn nil_is_a_task_with_no_work() {
    accepts(&task_body("var t Task<int> = nil\nprintln(t.wait())"));
    rejects(&task_body("let t = nil"), "`nil` needs an");
    rejects(&task_body("var t int = nil"), "`nil` needs an");
}

#[test]
fn wait_and_await_follow_the_async_boundary() {
    rejects(
        &task_program("async func f() int { let t = go compute()\nreturn t.wait() }"),
        "`wait` blocks, so it is not allowed inside an `async func`",
    );
    rejects(
        &task_body("let t = go compute()\nlet v = await t"),
        "`await` is only valid inside an `async func`",
    );
    accepts(&task_program(
        "async func f() (int, error) {
            let t = go load()
            let v = await t?
            return v, nil
        }",
    ));
    accepts(&task_program(
        "async func f() int {
            let a = go compute()
            return await a
        }",
    ));
    rejects(
        &task_body("let t = go compute()\nt.wait(1)"),
        "takes no arguments",
    );
    rejects(
        &task_body("let t = go compute()\nt.cancel()"),
        "has no method `cancel`",
    );
}

#[test]
fn awaited_task_errors_follow_error_rules() {
    rejects(
        &task_body("let t = go load()\nlet v, err = t.wait()"),
        "may be unused before scope exit",
    );
    rejects(
        &task_body("let t = go load()\nt.wait()"),
        "error result must be used or explicitly discarded",
    );
    accepts(&task_body(
        "let t = go load()\nlet v, err = t.wait()\n_ = v\n_ = err",
    ));
    rejects(
        &task_program("async func f() { let t = go load()\nlet v = await t? }"),
        "`?` requires a trailing `error` result in this function",
    );
}

#[test]
fn go_needs_a_call_to_a_declared_function() {
    rejects(&task_body("let t = go 5"), "`go` needs a call");
    rejects(&task_body("let n = 1\nlet t = go n"), "`go` needs a call");
    rejects(
        &task_body("let t = go println(\"x\")"),
        "`go` needs a call to a declared function or method",
    );
    rejects(&task_body("let t = go undefined()"), "cannot find");
}

#[test]
fn spawned_inputs_must_not_borrow_the_spawner() {
    rejects(
        &task_body("var n = 1\nlet t = go bump(n)"),
        "cannot take a `mut` parameter",
    );
    rejects(
        &task_program("func f(values own Array<int>) { let t = go look(values) }"),
        "needs `own` to take a value that is moved",
    );
    rejects(
        &task_program("func f(values []int) { let t = go view(values) }"),
        "cannot take a view",
    );
    rejects(
        &task_program("func f() { var values = [int; 3]{1, 2, 3}\nlet t = go view(values[0:2]) }"),
        "cannot take a view",
    );
    rejects(
        &task_body("let f = func(n int) int { return n }\nlet t = go inspect(f)"),
        "cannot find",
    );
    accepts(&task_program(
        "func f(values own Array<int>) { let t = go eat(values)\n t.wait() }",
    ));
    accepts(&task_program(
        "func f(count int, name string) {
            let a = go compute()
            let b = go log(name)
            let c = go fetch(count)
        }",
    ));
    rejects(
        &task_program("func f(values own Array<int>) { let t = go eat(values)\n eat(values) }"),
        "use of moved value `values`",
    );
}

const CHANNEL_PRELUDE: &str = "
type Job struct { Name string }
func (j mut Job) drop() {}
type Request struct {
    Text string
    Reply channel<string>
}
func consume(ch channel<int>) {}
";

fn channel_body(stmts: &str) -> String {
    program(&format!("{CHANNEL_PRELUDE}\nfunc entry() {{\n{stmts}\n}}"))
}

#[test]
fn channels_are_created_with_a_capacity_or_without() {
    let case = accepts(&channel_body(
        "let a = channel<int>()
        let b = channel<Job>(4)
        let c = channel<channel<int>>(1 + 2)
        let d channel<string> = channel<string>()
        let e = [channel<int>; 2]{a, a}
        let f = Array<channel<int>>{a}",
    ));
    let entry = case.function("entry");
    let types: Vec<String> = entry
        .locals
        .iter()
        .take(4)
        .map(|local| case.package().types.display(local.ty).to_string())
        .collect();
    assert_eq!(
        types,
        [
            "channel<int64>",
            "channel<Job>",
            "channel<channel<int64>>",
            "channel<string>"
        ]
    );
    rejects(
        &channel_body("let c = channel<int>(\"big\")"),
        "mismatched types",
    );
    rejects(
        &channel_body("let c = channel<int>(-1)"),
        "capacity cannot be negative",
    );
    rejects(&channel_body("let c = channel<Missing>()"), "cannot find");
}

#[test]
fn channels_cannot_carry_borrowed_values() {
    let message = "a channel cannot carry slices or function values";
    rejects(&channel_body("let c = channel<[]int>()"), message);
    rejects(&channel_body("let c = channel<mut []int>()"), message);
    rejects(&channel_body("let c = channel<func()>(1)"), message);
    rejects(
        &program("type Wrapper struct { Items []int }\nfunc f(c channel<Wrapper>) {}"),
        message,
    );
    rejects(&program("func f(c channel<Array<[]int>>) {}"), message);
    accepts(&program(
        "func f(c channel<Array<int>>, d channel<[int; 2]>) {}",
    ));
}

#[test]
fn channel_handles_are_copy_and_never_nil() {
    accepts(&channel_body(
        "let a = channel<int>()
        let b = a
        consume(a)
        consume(b)
        let r = Request{Text: \"x\", Reply: channel<string>()}
        let copy = r",
    ));
    rejects(&channel_body("var c channel<int> = nil"), "`nil` needs an");
    rejects(
        &channel_body("let a = channel<int>()\n_ = a == a"),
        "cannot be applied",
    );
    rejects(
        &channel_body("let a = channel<int>()\n_ = a != nil"),
        "`nil` needs an",
    );
    rejects(
        &channel_body("let a = channel<int>()\nprintln(a)"),
        "cannot print",
    );
    rejects(
        &channel_body("let a = channel<int>()\nlet b = clone(a)"),
        "cannot clone",
    );
}

#[test]
fn send_receive_and_close_have_fixed_shapes() {
    accepts(&channel_body(
        "let ch = channel<int>(1)
        ch.send(1)
        ch.send(2 + 3)
        let value, ok = ch.receive()
        _ = value
        _ = ok
        ch.receive()
        ch.close()",
    ));
    rejects(
        &channel_body("let ch = channel<int>()\nch.send(\"x\")"),
        "mismatched types",
    );
    rejects(
        &channel_body("let ch = channel<int>()\nch.send()"),
        "`send` takes 1 argument",
    );
    rejects(
        &channel_body("let ch = channel<int>()\nch.receive(1)"),
        "`receive` takes 0 arguments",
    );
    rejects(
        &channel_body("let ch = channel<int>()\nch.close(1)"),
        "`close` takes 0 arguments",
    );
    rejects(
        &channel_body("let ch = channel<int>()\nlet v = ch.receive()"),
        "this call returns 2 values",
    );
    rejects(
        &channel_body("let ch = channel<int>()\nch.flush()"),
        "has no method `flush`",
    );
    rejects(
        &channel_body("let ch = channel<int>()\nlet v, ok, extra = ch.receive()"),
        "expected 3 values for this binding",
    );
}

#[test]
fn sending_a_move_value_gives_it_up() {
    rejects(
        &channel_body(
            "let ch = channel<Job>(1)\nlet job = Job{Name: \"a\"}\nch.send(job)\nprintln(job.Name)",
        ),
        "use of moved value",
    );
    accepts(&channel_body(
        "let ch = channel<Job>(1)\nlet job = Job{Name: \"a\"}\nch.send(job)\nch.send(Job{Name: \"b\"})",
    ));
    accepts(&channel_body(
        "let ch = channel<Array<int>>(1)\nlet items = Array<int>{1, 2}\nch.send(items)",
    ));
    rejects(
        &channel_body(
            "let ch = channel<Array<int>>(1)\nlet items = Array<int>{1, 2}\nch.send(items)\nch.send(items)",
        ),
        "use of moved value",
    );
    accepts(&channel_body(
        "let ch = channel<string>(1)\nlet text = \"kept\"\nch.send(text)\nprintln(text)",
    ));
}

#[test]
fn channels_work_with_tasks_and_async_functions() {
    accepts(
        "package main
        func worker(ch channel<int>, done channel<bool>) {
            for {
                let value, ok = ch.receive()
                if !ok { break }
                _ = value
            }
            done.send(true)
        }
        async func relay(ch channel<int>) {
            let value, ok = ch.receive()
            if ok { ch.send(value + 1) }
        }
        func main() {
            let ch = channel<int>()
            let done = channel<bool>()
            go worker(ch, done)
            go relay(ch)
            ch.close()
            let finished, _ = done.receive()
            _ = finished
        }",
    );
}

const MUTEX_PRELUDE: &str = "
type Job struct { Name string }
func (j mut Job) drop() {}
type Bank struct {
    Balance int
    Log Array<string>
}
";

fn mutex_body(stmts: &str) -> String {
    program(&format!("{MUTEX_PRELUDE}\nfunc entry() {{\n{stmts}\n}}"))
}

#[test]
fn mutexes_hold_a_value_and_lend_it_to_a_function() {
    let case = accepts(&mutex_body(
        "let counter = mutex(0)
        counter.withLock(func(value mut int) { value += 1 })
        let total = counter.withLock(func(value mut int) int { return value })
        let both, name = mutex(Bank{Balance: 1, Log: Array<string>{}}).withLock(
            func(bank mut Bank) (int, string) {
                bank.Log.push(\"seen\")
                return bank.Balance, \"bank\"
            })
        let job = mutex(Job{Name: \"x\"})
        let flag = job.isPoisoned()
        let typed Mutex<string> = mutex(\"text\")
        let sized = [Mutex<int>; 2]{counter, counter}
        let list = Array<Mutex<int>>{counter}
        _ = total
        _ = both
        _ = name
        _ = flag
        _ = typed
        _ = sized
        _ = list",
    ));
    let entry = case.function("entry");
    let types: Vec<String> = entry
        .locals
        .iter()
        .take(2)
        .map(|local| case.package().types.display(local.ty).to_string())
        .collect();
    assert_eq!(types, ["Mutex<int64>", "int64"]);
}

#[test]
fn the_function_given_to_with_lock_must_take_a_mutable_borrow() {
    let wrong = "`withLock` needs a function that takes `mut int64`";
    rejects(
        &mutex_body("let m = mutex(0)\nm.withLock(func(v int) {})"),
        wrong,
    );
    rejects(
        &mutex_body("let m = mutex(0)\nm.withLock(func(v mut string) {})"),
        wrong,
    );
    rejects(
        &mutex_body("let m = mutex(0)\nm.withLock(func() {})"),
        wrong,
    );
    rejects(
        &mutex_body("let m = mutex(0)\nm.withLock(func(v own int) {})"),
        wrong,
    );
    rejects(
        &mutex_body("let m = mutex(0)\nm.withLock(5)"),
        "needs a function",
    );
    rejects(
        &mutex_body("let m = mutex(0)\nm.withLock()"),
        "`withLock` takes 1 argument",
    );
    rejects(
        &mutex_body("let m = mutex(0)\nm.withLock(func(v mut int) {}, 1)"),
        "`withLock` takes 1 argument",
    );
}

#[test]
fn nothing_borrowed_can_leave_the_lock() {
    let views = "cannot return slices or function values";
    rejects(
        &mutex_body(
            "let m = mutex(Array<int>{1})\nlet s = m.withLock(func(v mut Array<int>) []int { return v[:] })",
        ),
        views,
    );
    rejects(
        &mutex_body(
            "let m = mutex(0)\nlet f = m.withLock(func(v mut int) func() { return func() {} })",
        ),
        views,
    );
    let guards = "a mutex cannot guard slices or function values";
    rejects(
        &mutex_body("let a = Array<int>{1}\nlet m = mutex(a[:])"),
        guards,
    );
    rejects(&mutex_body("let m = mutex(func() {})"), guards);
    rejects(&program("func f(m Mutex<[]int>) {}"), guards);
    rejects(
        &program("type Holder struct { Items []int }\nfunc f(m Mutex<Holder>) {}"),
        guards,
    );
}

#[test]
fn mutex_handles_are_copy_values_that_cannot_be_compared_or_printed() {
    accepts(&mutex_body(
        "let a = mutex(1)
        let b = a
        let c = a
        let list = Array<Mutex<int>>{a, b}
        _ = c
        _ = list",
    ));
    rejects(
        &mutex_body("let a = mutex(1)\nlet b = a\n_ = a == b"),
        "cannot be applied",
    );
    rejects(&mutex_body("var m Mutex<int> = nil"), "`nil` needs an");
    rejects(&mutex_body("let m = mutex(1)\nprintln(m)"), "cannot print");
    rejects(
        &mutex_body("let m = mutex(1)\nlet c = clone(m)"),
        "cannot clone",
    );
    rejects(
        &mutex_body("let m = mutex(1)\nm.lock()"),
        "has no method `lock`",
    );
}

#[test]
fn a_mutex_takes_a_move_value_and_gives_it_up_once() {
    rejects(
        &mutex_body("let job = Job{Name: \"a\"}\nlet m = mutex(job)\nprintln(job.Name)"),
        "use of moved value",
    );
    accepts(&mutex_body(
        "let m = mutex(Job{Name: \"a\"})\nlet n = m\n_ = n",
    ));
}

#[test]
fn mutex_and_mutex_type_names_are_predeclared() {
    rejects(&mutex_body("let mutex = 1"), "shadows a predeclared name");
    rejects(&mutex_body("let Mutex = 1"), "shadows a predeclared name");
    rejects(
        &program("type Mutex struct { x int }"),
        "shadows a predeclared name",
    );
    rejects(&program("func f(m Mutex) {}"), "needs a type argument");
    rejects(
        &mutex_body("let m = mutex()"),
        "`mutex` takes exactly 1 argument",
    );
    rejects(
        &mutex_body("let m = mutex(1, 2)"),
        "`mutex` takes exactly 1 argument",
    );
    rejects(&mutex_body("let f = mutex"), "can only be called");
}

#[test]
fn tasks_share_a_mutex_through_copied_handles() {
    accepts(&program(
        "func worker(counter Mutex<int>, done channel<bool>) {
            counter.withLock(func(value mut int) { value += 1 })
            done.send(true)
        }
        async func async_worker(counter Mutex<int>) int {
            return counter.withLock(func(value mut int) int { return value })
        }
        func start() {
            let counter = mutex(0)
            let done = channel<bool>(2)
            go worker(counter, done)
            go worker(counter, done)
            let t = go async_worker(counter)
            _ = t.wait()
        }",
    ));
}

#[test]
fn select_cases_must_be_channel_operations() {
    accepts(&channel_body(
        "let a = channel<int>()
        let jobs = channel<Job>(2)
        select {
            case let n, ok = a.receive() {
                if ok { consume(a) }
                _ = n
            }
            case jobs.send(Job{Name: \"x\"}) { }
            case a.receive() { }
            default { }
        }",
    ));
    rejects(
        &channel_body("let a = channel<int>()\nselect { case a.close() { } }"),
        "a `select` case must be a channel `send` or `receive`",
    );
    rejects(
        &channel_body("let a = channel<int>()\nselect { case a.send(\"x\") { } }"),
        "",
    );
    rejects(
        &channel_body("let a = channel<int>()\nselect { case consume(a) { } }"),
        "a `select` case must be a channel `send` or `receive`",
    );
}

#[test]
fn declared_functions_are_capture_free_function_values() {
    accepts(&program(
        "func add(a int, b int) int { return a + b }
func edit(values mut []int, factor int) {}
func take(f func(int, int) int) int { return f(1, 2) }
func pick() func(int, int) int { return add }
type Box struct { op func(int, int) int }
func g() {
    let f = add
    let h func(mut []int, int) = edit
    var data = Array<int>{1}
    h(data[:], 2)
    println(f(1, 2) + take(add) + pick()(3, 4))
    let boxed = Box{op: add}
    var all = Array<func(int, int) int>{}
    all.push(add)
    _ = boxed
}",
    ));
    for (decls, message) in [
        (
            "func add(a int, b int) int { return a + b }\nfunc g() { let f func(int) int = add\n_ = f }",
            "mismatched types",
        ),
        (
            "func add(a int, b int) int { return a + b }\nfunc g() { let f = add\nlet h = add\n_ = f == h }",
            "operator `==` cannot be applied",
        ),
        (
            "func add(a int, b int) int { return a + b }\nfunc g() { let f = add\nlet h = f\n_ = f\n_ = h }",
            "use of moved value `f`",
        ),
        (
            "func edit(values mut []int) {}\nfunc g(values []int) { let f = edit\nf(values) }",
            "expected `mut []int64`, found `[]int64`",
        ),
        (
            "func g() { let f = println\n_ = f }",
            "`println` can only be called",
        ),
    ] {
        rejects(&program(decls), message);
    }
}

#[test]
fn go_takes_closures_and_function_values() {
    accepts(&task_program(
        "func twice(n int) int { return n * 2 }
func run(handler own func() int) int { return handler() }
func start(id int, name string, work own Array<int>) {
    let literal = go func() int { return id + 1 }()
    let named = twice
    let value = go named(4)
    let finish = func() { eat(work) }
    let once = go finish()
    let handler = func() int { return id }
    let passed = go run(handler)
    go func(message string) { log(message) }(name)
    println(literal.wait() + value.wait() + passed.wait())
    once.wait()
}
func inside() {
    var total = 0
    let t = go func() {
        var local = total
        local += 1
        println(local)
    }()
    t.wait()
}
async func parent(id int) {
    let t = go func() int { return id }()
    println(await t)
}",
    ));
    for (decls, message) in [
        (
            "func f() { let job = func() {}\nlet a = go job()\nlet b = go job()\na.wait()\nb.wait() }",
            "use of moved value `job`",
        ),
        (
            "func f() { var n = 0\nlet t = go func() { n += 1 }()\nt.wait() }",
            "a spawned closure cannot change `n`",
        ),
        (
            "func f() { var n = 0\nlet job = func() { bump(n) }\nlet t = go job()\nt.wait() }",
            "a spawned closure cannot change `n`",
        ),
        (
            "func f() { var values = Array<int>{1}\nlet part = values[:]\nlet t = go func() { view(part) }()\nt.wait() }",
            "which holds a view",
        ),
        (
            "func f(values Array<int>) { let t = go func() { look(values) }()\nt.wait() }",
            "cannot move borrowed value `values`",
        ),
        (
            "func f(n mut int) { let t = go func() { println(n) }()\nt.wait() }",
            "cannot capture `mut` parameter `n`",
        ),
        (
            "type S struct { op func() }\nfunc f(s own S) { let t = go (s.op)()\nt.wait() }",
            "`go` needs a call to a declared function or method, a function-typed local, or a closure literal",
        ),
        (
            "func f() { var n = 1\nlet inner = func() { println(n) }\nlet t = go func() { inner() }()\nt.wait() }",
            "a spawned task cannot hold a borrow of `n`",
        ),
        (
            "func run(f func()) { f() }\nfunc f() { let g = func() {}\nlet t = go run(g)\nt.wait() }",
            "can take a function value only for an `own` parameter",
        ),
        (
            "func f() { let t = go func() []int { return Array<int>{1}[:] }()\nt.wait() }",
            "a task cannot return slices or function values",
        ),
        (
            "async func f() { let t = go func() { await fetch(1) }()\nt.wait() }",
            "`await`",
        ),
    ] {
        rejects(&task_program(decls), message);
    }
}

#[test]
fn method_values_close_over_their_receivers() {
    let prelude = "type Counter struct { N int }
func (c Counter) read() int { return c.N }
func (c Counter) add(k int) int { return c.N + k }
func (c mut Counter) bump() { c.N += 1 }
func (c own Counter) finish() int { return c.N }
type Holder struct { Inner Counter }
type Res struct { Name string }
func (r mut Res) drop() {}
func (r Res) name() string { return r.Name }
func (r mut Res) rename() { r.Name = \"x\" }
async func (c Counter) load() int { return c.N }
func make() Counter { return Counter{N: 1} }
func apply(f func() int) int { return f() }
";
    let with = |body: &str| program(&format!("{prelude}\nfunc entry() {{\n{body}\n}}"));
    accepts(&with(
        "var counter = Counter{N: 1}
let read = counter.read
let add = counter.add
println(read() + add(1))
let bump = counter.bump
bump()
println(apply(counter.read))
let holder = Holder{Inner: counter}
let inner = holder.Inner.read
println(inner())
let done = counter.finish
println(done())
let again = Counter{N: 2}
let spawned = again.read
let task = go spawned()
_ = task",
    ));
    for (body, message) in [
        (
            "let c = Counter{N: 1}\nlet b = c.bump\nb()",
            "cannot pass immutable binding `c` as a `mut` argument",
        ),
        (
            "let f = make().read\n_ = f",
            "the receiver of a method value must be a local or a field of a local",
        ),
        (
            "let c = Counter{N: 1}\nlet f = c.load\n_ = f",
            "an `async` method cannot be used as a value",
        ),
        (
            "let r = Res{Name: \"a\"}\nlet f = r.drop\n_ = f",
            "the `drop` method cannot be used as a value",
        ),
        (
            "var c = Counter{N: 1}\nlet b = c.bump\nlet t = go b()\nt.wait()",
            "a spawned closure cannot change `c`",
        ),
        (
            "let r = Res{Name: \"a\"}\nlet n = r.nme\n_ = n",
            "no field `nme`",
        ),
    ] {
        rejects(&with(body), message);
    }
}

#[test]
fn async_function_values_follow_the_async_call_contract() {
    let prelude = "async func fetch(id int) (string, error) { return \"x\", nil }
async func ping() {}
func lookup(id int) (string, error) { return \"y\", nil }
type Route struct {
    path string
    handler async func(int) (string, error)
}
async func retry(op async func(int) (string, error), id int) (string, error) {
    let first, err = await op(id)
    if err == nil { return first, nil }
    return await op(id)
}
";
    let with = |decls: &str| program(&format!("{prelude}\n{decls}"));
    let case = accepts(&with(
        "async func serve(r own Route) {
            let h = fetch
            let a, e1 = await h(1)
            let task = go h(2)
            let b, e2 = await task
            let c, e3 = await retry(fetch, 3)
            let d, e4 = await (r.handler)(4)
            _ = e1
            _ = e2
            _ = e3
            _ = e4
        }
        func plain() {
            let h = fetch
            let t = go h(1)
            let v, err = t.wait()
            _ = err
            let p = ping
            go p()
        }",
    ));
    drop(case);
    for (decls, message) in [
        (
            "func f() { let h = fetch\nlet a = h(1)\n_ = a }",
            "call to async function value `h` is neither awaited nor spawned",
        ),
        (
            "async func f() { let h = ping\nh() }",
            "call to async function value `h` is neither awaited nor spawned",
        ),
        (
            "func f() { let h = fetch\nlet v, e = await h(1)\n_ = v\n_ = e }",
            "`await` is only valid inside an `async func`",
        ),
        (
            "async func f() { let g = func() { let h = fetch\nawait h(1) }\n_ = g }",
            "`await` is only valid inside an `async func`",
        ),
        (
            "func f() { let h func(int) (string, error) = fetch\n_ = h }",
            "expected `func(int64) (string, error)`, found `async func(int64) (string, error)`",
        ),
        (
            "func f() { let h async func(int) (string, error) = lookup\n_ = h }",
            "expected `async func(int64) (string, error)`, found `func(int64) (string, error)`",
        ),
        ("func f() { var h = fetch\nh = lookup }", "mismatched types"),
        (
            "func f() { let h = fetch\nlet k = fetch\n_ = h == k }",
            "operator `==` cannot be applied",
        ),
        (
            "type Bag struct { items []async func() }\nfunc f() {}",
            "slice",
        ),
    ] {
        rejects(&with(decls), message);
    }
}

#[test]
fn an_unapproved_copy_call_is_an_unknown_name() {
    let case = rejects(
        &body("let value = 3\nlet other = copy(value)\nprintln(other)"),
        "cannot find `copy` in this scope",
    );
    assert!(
        case.errors()
            .iter()
            .any(|(message, span)| message.contains("cannot find `copy`") && *span == "copy"),
        "{:?}",
        case.errors()
    );
    accepts(&body(
        "let values = Array<int>{1}\nlet other = clone(values)\nprintln(other.len())",
    ));
}
