//! Parser tests for AST shape, spans, rejection, and recovery.

use zore::ast::*;
use zore::parser::{Parsed, parse};
use zore::source::{SourceMap, Span};

struct Case {
    sources: SourceMap,
    parsed: Parsed,
}

impl Case {
    fn new(text: &str) -> Self {
        let mut sources = SourceMap::new();
        let id = sources.add("test.ore", text.into()).unwrap();
        let parsed = parse(sources.file(id).unwrap());
        Self { sources, parsed }
    }

    /// Parse `body` as the statements of `func main()`.
    fn body(body: &str) -> Self {
        Self::new(&format!("package main\n\nfunc main() {{\n{body}\n}}\n"))
    }

    fn text(&self, span: Span) -> &str {
        self.sources.slice(span).unwrap()
    }

    fn errors(&self) -> Vec<(&str, &str)> {
        self.parsed
            .diagnostics
            .iter()
            .map(|d| (d.message(), self.text(d.span())))
            .collect()
    }

    fn assert_clean(&self) -> &Self {
        assert!(self.parsed.diagnostics.is_empty(), "{:#?}", self.errors());
        self
    }

    fn main_stmts(&self) -> &[Stmt] {
        match self.parsed.file.items.last() {
            Some(Item::Func(func)) => &func.body.stmts,
            other => panic!("expected function, found {other:?}"),
        }
    }

    /// S-expression rendering of `main`'s statements.
    fn shape(&self) -> Vec<String> {
        self.main_stmts().iter().map(|s| stmt(self, s)).collect()
    }

    fn render(&self) -> String {
        self.parsed
            .diagnostics
            .iter()
            .map(|d| d.render(&self.sources).unwrap())
            .collect()
    }
}

fn expr(case: &Case, e: &Expr) -> String {
    match &e.kind {
        ExprKind::Name(name) => name.clone(),
        ExprKind::Int(_) | ExprKind::Float => case.text(e.span).to_owned(),
        ExprKind::String(value) => format!("{value:?}"),
        ExprKind::Rune(value) => format!("{value:?}"),
        ExprKind::Bool(value) => value.to_string(),
        ExprKind::Nil => "nil".into(),
        ExprKind::Malformed => "<malformed>".into(),
        ExprKind::Paren(inner) => format!("(paren {})", expr(case, inner)),
        ExprKind::Unary { op, operand } => {
            let op = match op {
                UnaryOp::Plus => "+",
                UnaryOp::Neg => "-",
                UnaryOp::Not => "!",
                UnaryOp::Complement => "^",
            };
            format!("({op} {})", expr(case, operand))
        }
        ExprKind::Binary { op, lhs, rhs } => {
            format!("({} {} {})", binary(*op), expr(case, lhs), expr(case, rhs))
        }
        ExprKind::Await(inner) => format!("(await {})", expr(case, inner)),
        ExprKind::Try(inner) => format!("(? {})", expr(case, inner)),
        ExprKind::Call { callee, args } => {
            let mut out = format!("(call {}", expr(case, callee));
            for arg in args {
                out.push(' ');
                out.push_str(&expr(case, arg));
            }
            out + ")"
        }
        ExprKind::Field { base, name } => format!("(. {} {})", expr(case, base), name.text),
        ExprKind::StructLit { ty, fields } => {
            let mut out = format!("(lit {}", ty.text);
            for field in fields {
                out.push_str(&format!(
                    " {}:{}",
                    field.name.text,
                    expr(case, &field.value)
                ));
            }
            out + ")"
        }
    }
}

fn binary(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Rem => "%",
        BinaryOp::Shl => "<<",
        BinaryOp::Shr => ">>",
        BinaryOp::BitAnd => "&",
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::BitOr => "|",
        BinaryOp::BitXor => "^",
        BinaryOp::Eq => "==",
        BinaryOp::NotEq => "!=",
        BinaryOp::Lt => "<",
        BinaryOp::LtEq => "<=",
        BinaryOp::Gt => ">",
        BinaryOp::GtEq => ">=",
        BinaryOp::And => "&&",
        BinaryOp::Or => "||",
    }
}

fn block(case: &Case, b: &Block) -> String {
    let stmts: Vec<String> = b.stmts.iter().map(|s| stmt(case, s)).collect();
    format!("{{{}}}", stmts.join("; "))
}

fn binding(case: &Case, b: &Binding) -> String {
    let kind = match b.kind {
        BindingKind::Let => "let",
        BindingKind::Var => "var",
        BindingKind::Const => "const",
    };
    let targets: Vec<&str> = b
        .targets
        .iter()
        .map(|t| match t {
            BindingTarget::Name(name) => name.text.as_str(),
            BindingTarget::Discard(_) => "_",
        })
        .collect();
    let ty =
        b.ty.as_ref()
            .map(|t| format!(" {}", t.name.text))
            .unwrap_or_default();
    format!(
        "({kind} {}{ty} {})",
        targets.join(","),
        expr(case, &b.value)
    )
}

fn stmt(case: &Case, s: &Stmt) -> String {
    match &s.kind {
        StmtKind::Binding(b) => binding(case, b),
        StmtKind::Assign {
            targets,
            op,
            values,
        } => {
            let targets: Vec<String> = targets
                .iter()
                .map(|t| match t {
                    AssignTarget::Place(e) => expr(case, e),
                    AssignTarget::Discard(_) => "_".into(),
                })
                .collect();
            let values: Vec<String> = values.iter().map(|v| expr(case, v)).collect();
            let op = match op {
                AssignOp::Assign => "=".to_owned(),
                AssignOp::Compound(op) => format!("{}=", binary(*op)),
            };
            format!("({op} {} {})", targets.join(","), values.join(","))
        }
        StmtKind::Expr(e) => expr(case, e),
        StmtKind::Return(values) => {
            let values: Vec<String> = values.iter().map(|v| expr(case, v)).collect();
            format!(
                "(return{})",
                values.iter().map(|v| format!(" {v}")).collect::<String>()
            )
        }
        StmtKind::Break => "break".into(),
        StmtKind::Continue => "continue".into(),
        StmtKind::If(i) => if_stmt(case, i),
        StmtKind::For(f) => {
            let header = match &f.header {
                ForHeader::Infinite => String::new(),
                ForHeader::Condition(c) => format!(" {}", expr(case, c)),
                ForHeader::Counting {
                    init,
                    condition,
                    update,
                } => format!(
                    " {}; {}; {}",
                    stmt(case, init),
                    expr(case, condition),
                    stmt(case, update)
                ),
            };
            format!("(for{header} {})", block(case, &f.body))
        }
        StmtKind::Block(b) => block(case, b),
    }
}

fn if_stmt(case: &Case, i: &If) -> String {
    let mut out = format!(
        "(if {} {}",
        expr(case, &i.condition),
        block(case, &i.then_block)
    );
    match &i.else_branch {
        None => {}
        Some(Else::Block(b)) => out.push_str(&format!(" else {}", block(case, b))),
        Some(Else::If(inner)) => out.push_str(&format!(" else {}", if_stmt(case, inner))),
    }
    out + ")"
}

/// Parse `source` as one expression statement `_ = source` and render it.
fn expr_shape(source: &str) -> String {
    let case = Case::body(&format!("_ = {source}"));
    case.assert_clean();
    let shape = case.shape();
    shape[0]
        .strip_prefix("(= _ ")
        .and_then(|s| s.strip_suffix(')'))
        .unwrap()
        .to_owned()
}

/// Assert that `body` is rejected with a diagnostic containing `message`.
fn rejects(body: &str, message: &str) {
    let case = Case::body(body);
    let errors = case.errors();
    assert!(
        errors.iter().any(|(m, _)| m.contains(message)),
        "{body:?}: expected {message:?}, got {errors:#?}"
    );
}

fn rejects_file(text: &str, message: &str) {
    let case = Case::new(text);
    let errors = case.errors();
    assert!(
        errors.iter().any(|(m, _)| m.contains(message)),
        "{text:?}: expected {message:?}, got {errors:#?}"
    );
}

#[test]
fn semantic_target_parses_to_expected_shape() {
    let mut sources = SourceMap::new();
    let id = sources.load("examples/semantic-target/main.ore").unwrap();
    let parsed = parse(sources.file(id).unwrap());
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let file = &parsed.file;
    assert_eq!(file.package.as_ref().unwrap().text, "main");
    let [Item::Struct(user), Item::Func(greet), Item::Func(main)] = &file.items[..] else {
        panic!("{:#?}", file.items);
    };
    assert_eq!(user.name.text, "User");
    assert_eq!(user.fields[0].name.text, "Name");
    assert_eq!(user.fields[0].ty.name.text, "string");
    assert_eq!(greet.params[0].mode, ParamMode::Borrow);
    assert_eq!(greet.params[0].ty.name.text, "User");
    assert!(greet.results.is_empty() && main.params.is_empty());
    assert_eq!(
        sources.slice(greet.span).unwrap().lines().next(),
        Some("func greet(user User) {")
    );
    assert!(
        sources
            .slice(main.span)
            .unwrap()
            .ends_with("greet(user)\n}")
    );

    let Some(Item::Func(_)) = file.items.last() else {
        unreachable!()
    };
    let case = Case { sources, parsed };
    assert_eq!(
        case.shape(),
        ["(let user (lit User Name:\"John\"))", "(call greet user)"]
    );
}

#[test]
fn hello_example_parses() {
    let mut sources = SourceMap::new();
    let id = sources.load("examples/hello/main.ore").unwrap();
    let parsed = parse(sources.file(id).unwrap());
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
}

#[test]
fn precedence_and_associativity_follow_the_operator_table() {
    for (source, expected) in [
        ("a + b * c", "(+ a (* b c))"),
        ("a - b - c", "(- (- a b) c)"),
        ("a + b << c", "(+ a (<< b c))"),
        ("a | b ^ c", "(^ (| a b) c)"),
        ("a == b || c", "(|| (== a b) c)"),
        ("a || b && c", "(|| a (&& b c))"),
        ("a & b + c", "(+ (& a b) c)"),
        ("a % b / c", "(/ (% a b) c)"),
        ("a >> b != c", "(!= (>> a b) c)"),
        ("(a + b) * c", "(* (paren (+ a b)) c)"),
        ("-a * b", "(* (- a) b)"),
        ("!-^+a", "(! (- (^ (+ a))))"),
        ("a &^ b", "(& a (^ b))"),
        ("- -x", "(- (- x))"),
        ("f(a)(b).c.d(e)", "(call (. (. (call (call f a) b) c) d) e)"),
        ("-read()?", "(- (? (call read)))"),
        ("read()??", "(? (? (call read)))"),
        ("(value?).field", "(. (paren (? value)) field)"),
        ("a < b && b < c", "(&& (< a b) (< b c))"),
        ("(a < b) == c", "(== (paren (< a b)) c)"),
        ("int64(value)", "(call int64 value)"),
        ("\"a\" + `b`", "(+ \"a\" \"b\")"),
        ("'x'", "'x'"),
        ("1.5 + 0xFF", "(+ 1.5 0xFF)"),
        ("true || false || nil", "(|| (|| true false) nil)"),
    ] {
        assert_eq!(expr_shape(source), expected, "{source}");
    }
}

#[test]
fn await_groups_trailing_propagation_outside() {
    for (source, expected) in [
        ("await operation()?", "(? (await (call operation)))"),
        (
            "await object.method()?",
            "(? (await (call (. object method))))",
        ),
        ("await task?", "(? (await task))"),
        (
            "await (operation()?)",
            "(await (paren (? (call operation))))",
        ),
        ("await operation()", "(await (call operation))"),
        ("-await operation()?", "(- (? (await (call operation))))"),
        ("await a + b", "(+ (await a) b)"),
        ("await -x", "(await (- x))"),
    ] {
        assert_eq!(expr_shape(source), expected, "{source}");
    }
}

#[test]
fn comparison_chains_and_misplaced_propagation_are_rejected() {
    for source in ["a < b < c", "a == b == c", "a < b == c", "a != b >= c"] {
        rejects(&format!("_ = {source}"), "cannot be chained");
    }
    rejects("_ = value?.field", "binds more loosely");
    rejects("_ = call()?()", "binds more loosely");
}

#[test]
fn bindings_follow_initialization_syntax() {
    let case = Case::body(
        "let count = 0
        var total int64 = 0
        const limit = 100
        const capacity int64 = 100
        let value, err = load()
        var first, second = pair()
        let _, _ = pair()
        let _ = calculate()
        var _ = calculate()",
    );
    case.assert_clean();
    assert_eq!(
        case.shape(),
        [
            "(let count 0)",
            "(var total int64 0)",
            "(const limit 100)",
            "(const capacity int64 100)",
            "(let value,err (call load))",
            "(var first,second (call pair))",
            "(let _,_ (call pair))",
            "(let _ (call calculate))",
            "(var _ (call calculate))",
        ]
    );
    let Stmt { span, .. } = &case.main_stmts()[1];
    assert_eq!(case.text(*span), "var total int64 = 0");
}

#[test]
fn invalid_bindings_are_rejected() {
    for (body, message) in [
        ("let count: int64 = 0", "not written with `:`"),
        ("let count int64", "require an initializer"),
        ("var count int64", "require an initializer"),
        ("let count", "require an initializer"),
        ("let a, b = 1, 2", "exactly one initializer"),
        ("let a, b int = pair()", "typed multiple bindings"),
        ("const a, b = pair()", "exactly one name"),
        ("const _ = 1", "`_` cannot be used as a binding name"),
        ("let func = 1", "`func` is a keyword"),
        ("let own = 1", "`own` is a keyword"),
        ("let match = 1", "`match` is reserved"),
        ("let count := 1", "`:=` is not Zore syntax"),
    ] {
        rejects(body, message);
    }
}

#[test]
fn assignments_and_call_statements() {
    let case = Case::body(
        "count = count + 1
        count += 1
        mask <<= 2
        left, right = right, left
        left, right = pair()
        user.Name = name
        _ = save()
        _, _ = pair()
        greet(user)
        await operation()
        read(file)?
        await operation()?",
    );
    case.assert_clean();
    assert_eq!(
        case.shape(),
        [
            "(= count (+ count 1))",
            "(+= count 1)",
            "(<<= mask 2)",
            "(= left,right right,left)",
            "(= left,right (call pair))",
            "(= (. user Name) name)",
            "(= _ (call save))",
            "(= _,_ (call pair))",
            "(call greet user)",
            "(await (call operation))",
            "(? (call read file))",
            "(? (await (call operation)))",
        ]
    );
}

#[test]
fn invalid_assignments_and_bare_expressions_are_rejected() {
    for (body, message) in [
        ("_ += 1", "`_` cannot be a compound"),
        ("a, b += 1", "exactly one target"),
        ("a += 1, 2", "exactly one value"),
        ("f() = 1", "invalid assignment target"),
        ("1 = x", "invalid assignment target"),
        ("(a) = 1", "invalid assignment target"),
        ("a = b = c", "expected newline or `;`"),
        ("a, b", "expected `=`"),
        ("_", "expected `=`"),
        ("x = _", "`_` cannot be used as a value"),
        ("count", "not a statement"),
        ("1 + 2", "not a statement"),
        ("a == b", "not a statement"),
        ("\"text\"", "not a statement"),
        ("await task", "not a statement"),
        ("(greet(user))", "not a statement"),
        (";", "empty statement"),
        ("a();;", "empty statement"),
    ] {
        rejects(body, message);
    }
}

#[test]
fn functions_methods_and_parameters() {
    let case = Case::new(
        "package main

        func add(a int, b int,) int { return a + b }
        func pair() (int, error) { return 1, nil }
        func forward() (int, int) { return pair() }
        func (user User) greet() {}
        func (user mut User) rename(name string) { user.Name = name }
        func (user own User) save() {}
        func edit(user mut User, sink own Sink) {}
        async func fetch(id int) (User, error) {
            let response = await get(id)?
            return parse(response)?
        }
        func multi(
            first int,
            second int,
        ) {}
        ",
    );
    case.assert_clean();
    let funcs: Vec<&FuncDecl> = case
        .parsed
        .file
        .items
        .iter()
        .map(|item| match item {
            Item::Func(func) => func,
            other => panic!("{other:?}"),
        })
        .collect();
    let names: Vec<&str> = funcs.iter().map(|f| f.name.text.as_str()).collect();
    assert_eq!(
        names,
        [
            "add", "pair", "forward", "greet", "rename", "save", "edit", "fetch", "multi"
        ]
    );
    assert_eq!(funcs[0].params.len(), 2);
    assert_eq!(funcs[0].results[0].name.text, "int");
    let results: Vec<&str> = funcs[1]
        .results
        .iter()
        .map(|t| t.name.text.as_str())
        .collect();
    assert_eq!(results, ["int", "error"]);
    let modes: Vec<Option<ParamMode>> = funcs[3..6]
        .iter()
        .map(|f| f.receiver.as_ref().map(|r| r.mode))
        .collect();
    assert_eq!(
        modes,
        [
            Some(ParamMode::Borrow),
            Some(ParamMode::Mut),
            Some(ParamMode::Own)
        ]
    );
    assert_eq!(funcs[6].params[0].mode, ParamMode::Mut);
    assert_eq!(funcs[6].params[1].mode, ParamMode::Own);
    assert!(funcs[7].is_async && !funcs[0].is_async);
    assert_eq!(funcs[8].params.len(), 2);
    assert_eq!(case.text(funcs[6].params[1].span), "sink own Sink");
}

#[test]
fn invalid_function_syntax_is_rejected() {
    for (text, message) in [
        ("func add(a, b int) int { return a }", "grouped `a, b T`"),
        ("func add(a int = 1) {}", "default parameter values"),
        ("func add(a int...) {}", "expected `,` or `)`"),
        ("func add(a int) int", "require a body"),
        ("func main()\n{}", "same line as the function signature"),
        ("func one() (int) { return 1 }", "without parentheses"),
        ("func none() () {}", "omitting it"),
        (
            "func pair() (int, int,) { return 1, 2 }",
            "trailing comma is not allowed",
        ),
        ("func (a A, b B) m() {}", "exactly one receiver"),
        (
            "func (_ User) m() {}",
            "`_` cannot be used as a receiver name",
        ),
        ("func f(_ int) {}", "`_` cannot be used as a parameter name"),
        ("func type() {}", "`type` is a keyword"),
        ("async main() {}", "expected `func` after `async`"),
        (
            "func f(\n a int\n) {}",
            "missing trailing comma after the last parameter",
        ),
    ] {
        rejects_file(&format!("package main\n{text}\n"), message);
    }
    rejects("f(a,,b)", "expected an expression");
    rejects("f(,)", "expected an expression");
}

#[test]
fn struct_keyword_does_not_end_a_line() {
    // `struct` is not eligible for semicolon insertion.
    Case::new("package main\ntype User struct\n{}\n").assert_clean();
}

#[test]
fn structs_and_struct_literals() {
    let case = Case::new(
        "package main
        type User struct {
            Name string
            age int
        }
        type Empty struct {}
        type Point struct { X int; Y int }
        func main() {
            let user = User{
                Name: loadName(),
                age: loadAge(),
            }
            let point = Point{Y: 2, X: 1}
            let empty = Empty{}
            greet(User{Name: \"x\", age: 1})
            if (Point{X: 1, Y: 2}).X == 1 { work() }
            for (Point{X: 1, Y: 2}).X == 1 { work() }
            for var p = Point{X: 0, Y: 0}; p.X < 3; p.X += 1 {}
            for p = Point{X: 0, Y: 0}; p.X < 3; p.X += 1 {}
        }",
    );
    case.assert_clean();
    let Item::Struct(user) = &case.parsed.file.items[0] else {
        panic!()
    };
    let fields: Vec<(&str, &str)> = user
        .fields
        .iter()
        .map(|f| (f.name.text.as_str(), f.ty.name.text.as_str()))
        .collect();
    assert_eq!(fields, [("Name", "string"), ("age", "int")]);
    let Item::Struct(point) = &case.parsed.file.items[2] else {
        panic!()
    };
    assert_eq!(point.fields.len(), 2);
    assert_eq!(
        case.shape(),
        [
            "(let user (lit User Name:(call loadName) age:(call loadAge)))",
            "(let point (lit Point Y:2 X:1))",
            "(let empty (lit Empty))",
            "(call greet (lit User Name:\"x\" age:1))",
            "(if (== (. (paren (lit Point X:1 Y:2)) X) 1) {(call work)})",
            "(for (== (. (paren (lit Point X:1 Y:2)) X) 1) {(call work)})",
            "(for (var p (lit Point X:0 Y:0)); (< (. p X) 3); (+= (. p X) 1) {})",
            "(for (= p (lit Point X:0 Y:0)); (< (. p X) 3); (+= (. p X) 1) {})",
        ]
    );
}

#[test]
fn invalid_struct_syntax_is_rejected() {
    for (text, message) in [
        ("type User struct {\n own string\n}", "`own` is a keyword"),
        (
            "type User struct {\n Name string, age int\n}",
            "expected newline or `;` after field",
        ),
        ("type User struct {\n Name\n}", "expected a type"),
        ("type Alias int", "only struct type declarations"),
    ] {
        rejects_file(&format!("package main\n{text}\n"), message);
    }
    for (body, message) in [
        ("let u = User{\"John\"}", "must be named"),
        (
            "let u = User{Name: \"x\"\n}",
            "missing trailing comma after the last field",
        ),
        ("let u = User{Name \"x\"}", "must be named"),
        ("if Point{X: 1}.X == 1 { work() }", "expected"),
    ] {
        rejects(body, message);
    }
}

#[test]
fn control_flow_statements() {
    let case = Case::body(
        "if ready { work() }
        if (ready) { work() } else { rest() }
        if a { x() } else if b { y() } else { z() }
        for { break }
        for ready() { continue }
        for var i = 0; i < limit; i += 1 { work() }
        for i = 0; i < limit; i += 1 { work() }
        for reset(); running(); step() { work() }
        { let inner = 1 }
        return",
    );
    case.assert_clean();
    assert_eq!(
        case.shape(),
        [
            "(if ready {(call work)})",
            "(if (paren ready) {(call work)} else {(call rest)})",
            "(if a {(call x)} else (if b {(call y)} else {(call z)}))",
            "(for {break})",
            "(for (call ready) {continue})",
            "(for (var i 0); (< i limit); (+= i 1) {(call work)})",
            "(for (= i 0); (< i limit); (+= i 1) {(call work)})",
            "(for (call reset); (call running); (call step) {(call work)})",
            "{(let inner 1)}",
            "(return)",
        ]
    );
}

#[test]
fn invalid_control_flow_syntax_is_rejected() {
    for (body, message) in [
        ("if ready work()", "expected `{`"),
        ("if ready\n{ work() }", "same line as the `if` condition"),
        (
            "if a { x() }\nelse { y() }",
            "`else` must be on the same line",
        ),
        ("if let x = 1; x { }", "expected an expression"),
        ("for ;; { }", "cannot be empty"),
        (
            "for var i = 0; i < n; var j = 1 { }",
            "cannot be a declaration",
        ),
        ("for var i = 0; i < n\n i += 1 { }", "on one line"),
        ("for i = 0 { }", "`;` after the loop initializer"),
        ("for var i = 0; ; i += 1 { }", "expected an expression"),
        ("for i = 0; i < n; { }", "expected an expression"),
        ("for ready()\n{ }", "same line as the `for` header"),
        ("for x in items { }", "expected `{`"),
        ("x++", "not a Zore operator"),
    ] {
        rejects(body, message);
    }
}

#[test]
fn statement_boundaries_are_honored_by_the_parser() {
    Case::body("greet(\nuser,\n)").assert_clean();
    Case::new("package main\nfunc main() { greet(user) }").assert_clean();
    Case::body("let a = 1; let b = 2").assert_clean();
    Case::body("let total = first +\n second").assert_clean();
    rejects(
        "greet(\nuser\n)",
        "missing trailing comma after the last argument",
    );
    rejects("let total = first\n+ second", "not a statement");
    rejects("return\nvalue", "not a statement");
    rejects(
        "let a = 1 let b = 2",
        "expected newline or `;` after statement",
    );

    // The unsupported continuation must not turn into one expression.
    let case = Case::body("let total = first\n+ second");
    assert_eq!(case.shape()[0], "(let total first)");
}

#[test]
fn package_and_imports() {
    let case = Case::new("package main\nimport \"zore/fmt\"\nimport \"net\"\nfunc main() {}");
    case.assert_clean();
    let paths: Vec<&str> = case
        .parsed
        .file
        .imports
        .iter()
        .map(|i| i.path.as_str())
        .collect();
    assert_eq!(paths, ["zore/fmt", "net"]);
    assert_eq!(
        case.text(case.parsed.file.imports[0].span),
        "import \"zore/fmt\""
    );

    rejects_file("func main() {}", "expected `package` clause");
    rejects_file("// comment only\n", "expected `package` clause");
    rejects_file("package func\n", "`func` is a keyword");
    rejects_file(
        "package main\nfunc main() {}\nimport \"x\"\n",
        "imports must come before",
    );
    rejects_file("package main\nimport (\n\"x\"\n)\n", "grouped imports");
    rejects_file("package main\nimport `x`\n", "double-quoted import path");
    rejects_file("package main\nimport x\n", "double-quoted import path");
    rejects_file("package main extra\n", "after declaration");
    rejects_file("package main\nprintln(\"x\")\n", "expected a declaration");
}

#[test]
fn package_level_bindings_are_parsed() {
    let case = Case::new("package main\nconst limit = 10\nlet name = \"x\"\nvar count = 0\n");
    case.assert_clean();
    assert_eq!(case.parsed.file.items.len(), 3);
}

#[test]
fn later_milestone_syntax_is_reported_as_unsupported() {
    for body in [
        "_ = items[0]",
        "_ = items[1:2]",
        "let xs = [3]int{1, 2, 3}",
        "let m = map[string]int{}",
        "let t = go work()",
        "go work()",
        "let f = func() { work() }",
    ] {
        rejects(body, "not supported by this compiler yet");
    }
    for text in [
        "func f(xs []int) {}",
        "func f(xs Array<int>) {}",
        "func f(t Task<int>) {}",
        "func f(c channel<int>) {}",
        "func f(m map[string]int) {}",
        "func f(u pkg.User) {}",
        "func f(cb func()) {}",
    ] {
        rejects_file(
            &format!("package main\n{text}\n"),
            "not supported by this compiler yet",
        );
    }
    rejects("defer cleanup()", "`defer` is reserved");
    rejects("unsafe { }", "`unsafe` is reserved");
    rejects_file("package main\nenum Color {}\n", "`enum` is reserved");
}

#[test]
fn recovery_reports_each_statement_once_and_continues() {
    let case = Case::new(
        "package main
        func first() {
            let = 1
            let ok = 2
            greet(,)
            if { }
        }
        func second( {
        }
        func third() {
            1 + 2
        }",
    );
    let errors = case.errors();
    let messages: Vec<&str> = errors.iter().map(|(m, _)| *m).collect();
    assert_eq!(messages.len(), 5, "{errors:#?}");
    assert!(
        messages[0].contains("expected a binding name"),
        "{errors:#?}"
    );
    assert!(
        messages[1].contains("expected an expression"),
        "{errors:#?}"
    );
    assert!(
        messages[2].contains("expected an expression"),
        "{errors:#?}"
    );
    assert!(
        messages[3].contains("expected a parameter name"),
        "{errors:#?}"
    );
    assert_eq!(messages[4], "expression is not a statement");
    let names: Vec<&str> = case
        .parsed
        .file
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Func(f) => Some(f.name.text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(names, ["first", "third"]);
    let Item::Func(first) = &case.parsed.file.items[0] else {
        panic!()
    };
    assert_eq!(first.body.stmts.len(), 1);
}

#[test]
fn lexical_errors_are_not_reported_twice() {
    let case = Case::body("let x = @\nlet y = 0x\nlet z = \"open");
    let errors = case.errors();
    assert_eq!(errors.len(), 3, "{errors:#?}");
    assert_eq!(errors[0].0, "unexpected character '@'");
    assert!(errors[1].0.contains("hexadecimal digits"));
    assert_eq!(errors[2].0, "unterminated string literal");
    // Malformed literals still yield an expression so the binding is kept.
    assert_eq!(case.shape()[0], "(let y <malformed>)");
}

#[test]
fn unclosed_brace_points_at_both_ends() {
    let case = Case::new("package main\nfunc main() {\n    work()\n");
    assert_eq!(case.errors().len(), 1, "{:#?}", case.errors());
    let rendered = case.render();
    assert!(rendered.contains("error: unclosed `{`"), "{rendered}");
    assert!(rendered.contains("test.ore:2:13"), "{rendered}");
    assert!(rendered.contains("opened here"), "{rendered}");
}

#[test]
fn parsing_terminates_on_arbitrary_token_sequences() {
    const PIECES: &[&str] = &[
        "package ", "main ", "func ", "f", "(", ")", "{", "}", "[", "]", "let ", "x", "=", "1",
        ",", ";", "\n", "if ", "for ", "else ", "return ", ".", "?", "await ", "+", "<", "_",
        "type ", "struct ", "\"s\"", "@", ":", "own ", "go ", "import ", "async ", "const ",
    ];
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    for _ in 0..20000 {
        let mut text = String::new();
        for _ in 0..(state % 40) {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            text.push_str(PIECES[(state % PIECES.len() as u64) as usize]);
        }
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let case = Case::new(&text);
        for diagnostic in &case.parsed.diagnostics {
            assert!(case.sources.slice(diagnostic.span()).is_some(), "{text:?}");
        }
    }
}
