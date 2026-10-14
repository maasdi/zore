use super::{accepts, let_value, program, rejects};
use zore::hir::ExprKind;

#[test]
fn async_literals_keep_async_types_and_independent_nested_body_contexts() {
    let case = accepts(&program(
        r#"async func pause() int { return 1 }
async func outer() int {
    let plain = func() int {
        let nested = async func() int { return await pause() }
        let task = go nested()
        return task.wait()
    }
    let job = async func(n int) int { return n + await pause() }
    return plain() + await job(2)
}
func acceptsAsync(f async func(int) int) {}
func create() { let f = async func(n int) int { return n }; acceptsAsync(f) }
"#,
    ));
    let value = let_value(case.function("outer"), "job");
    let signature = case.package().types.func_signature(value.ty()).unwrap();
    assert!(signature.is_async);
    let ExprKind::Closure { function, .. } = value.kind else {
        panic!("expected closure")
    };
    assert!(case.package().function(function).is_async);
    for (source, expected) in [
        (
            "async func outer() { let f = func() { let n = await pause() }; f() }",
            "this function literal is not async",
        ),
        (
            "func outer() { let f = async func() int { return 1 }; let n = f() }",
            "neither awaited nor spawned",
        ),
        (
            "async func outer() { let f = async func() int { return 1 }; let g func() int = f }",
            "expected",
        ),
        (
            "async func outer() { let f = func() int { return 1 }; let g async func() int = f }",
            "expected",
        ),
        (
            "func outer() { let f = async func() { await plain() } }\nfunc plain() {}",
            "`await` needs",
        ),
        (
            "func outer() { let f = async func() int { await pause() } }",
            "without returning",
        ),
    ] {
        rejects(
            &program(&format!("async func pause() int {{ return 1 }}\n{source}")),
            expected,
        );
    }
}

#[test]
fn concrete_method_values_specialize_captured_generic_roots_and_nested_literals() {
    let case = accepts(&program(
        r#"type Counter struct { N int }
async func (c Counter) read() int { return c.N }
type Holder<T any> struct { Counter Counter; Value T }
func reader<T any>(h own Holder<T>) async func() int { return h.Counter.read }
func nested<T any>(h own Holder<T>) async func() int {
    return async func() int { let read = h.Counter.read; return await read() }
}
func exercise() {
    let f = reader(Holder<int>{Counter: Counter{N: 1}, Value: 7})
    let g = reader(Holder<Array<int>>{Counter: Counter{N: 2}, Value: Array<int>{8}})
    let h = nested(Holder<int>{Counter: Counter{N: 3}, Value: 9})
    let a = go f()
    let b = go g()
    let c = go h()
    println(a.wait() + b.wait() + c.wait())
}
"#,
    ));
    let package = case.package();
    let wrappers: Vec<_> = package
        .functions
        .iter()
        .filter(|function| function.name.contains("$method") && function.compiled())
        .collect();
    assert!(wrappers.len() >= 3);
    for function in wrappers {
        assert!(function.is_async);
        assert!(
            function
                .locals
                .iter()
                .all(|local| !package.types.mentions_param(local.ty))
        );
    }
    rejects(
        &program(
            "type Counter struct { N int }\nasync func (c Counter) read() int { return c.N }\ntype Holder<T any> struct { Counter Counter; Value T }\nfunc reader<T any>(h Holder<T>) async func() int { return h.Counter.read }",
        ),
        "cannot move borrowed",
    );
}

#[test]
fn async_method_values_keep_receiver_modes_and_existing_place_restrictions() {
    let prelude = r#"type Counter struct { N int }
type Owner struct { Counter Counter }
async func (c Counter) read() int { return c.N }
async func (c mut Counter) bump(n int) int { c.N += n; return c.N }
func make() Counter { return Counter{N: 1} }
"#;
    let case = accepts(&program(&format!(
        r#"{prelude}
async func work() int {{
    var owner = Owner{{Counter: Counter{{N: 1}}}}
    let read = owner.Counter.read
    let n = await read()
    let bump = owner.Counter.bump
    return await bump(n)
}}"#
    )));
    for name in ["read", "bump"] {
        let value = let_value(case.function("work"), name);
        assert!(
            case.package()
                .types
                .func_signature(value.ty())
                .unwrap()
                .is_async
        );
        let ExprKind::Closure { function, .. } = value.kind else {
            panic!("expected method wrapper")
        };
        assert!(case.package().function(function).is_async);
    }
    accepts(&program(
        "type Counter int\nasync func (c mut Counter) bump() { c += Counter(1) }\nasync func work() { var c = Counter(1); let f = c.bump; await f() }",
    ));
    for (body, expected) in [
        ("let f = make().read", "receiver of a method value"),
        ("let c = Counter{N: 1}; let f = c.bump", "immutable"),
        (
            "let c = Counter{N: 1}; let f = c.read; f()",
            "neither awaited nor spawned",
        ),
        (
            "let values = Array<Counter>{Counter{N: 1}}; let f = values[0].read",
            "receiver of a method value",
        ),
    ] {
        rejects(
            &program(&format!("{prelude}\nfunc work() {{ {body} }}")),
            expected,
        );
    }
    rejects(
        &program(
            "type Box<T any> struct { Value T }\nasync func (b Box<T>) read() T { return b.Value }\nfunc work() { let b = Box<int>{Value: 1}; let f = b.read }",
        ),
        "generic type",
    );
}
