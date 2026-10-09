use zore::async_lowering::{self, Suspension};
use zore::source::SourceMap;
use zore::{check, dropck, hir, mir};

fn plan(source: &str) -> (hir::Package, mir::Program, async_lowering::Plan) {
    let mut sources = SourceMap::new();
    let id = sources.add("async.ore", source.to_owned()).unwrap();
    let checked = check::check_file(sources.file(id).unwrap());
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    let package = checked.package.unwrap();
    let mut program = mir::lower::lower(&package);
    dropck::insert(&package, &mut program);
    let plan = async_lowering::lower(&package, &program);
    (package, program, plan)
}

#[test]
fn calls_and_task_awaits_keep_their_cleanup_edges_and_spans() {
    let (package, program, plan) = plan(
        "package main
async func leaf(t own Task<int>) int { return await t }
async func parent(t own Task<int>) int { return await leaf(t) }
func plain() int { return 1 }
func main() { let t = go parent(go plain()); println(t.wait()) }",
    );
    for (id, machine) in &plan.machines {
        let body = &program.bodies[id.0 as usize];
        assert!(package.function(*id).is_async);
        assert!(body.unwind.is_some());
        for (block, kind) in &machine.suspensions {
            let mir::Terminator::Call {
                callee,
                unwind,
                span,
                ..
            } = &body.blocks[block.0 as usize].terminator
            else {
                panic!("a suspension must be a call")
            };
            assert!(unwind.is_some());
            assert!(span.end() > span.start());
            match kind {
                Suspension::Call(id) => {
                    assert!(matches!(callee, mir::Callee::Function(callee) if id == callee))
                }
                Suspension::Value => assert!(matches!(callee, mir::Callee::Value(_))),
                Suspension::Sleep => assert!(
                    matches!(callee, mir::Callee::Function(id) if package.function(*id).name == "zore/time.Sleep")
                ),
                Suspension::Io(id) => {
                    assert!(matches!(callee, mir::Callee::Function(callee) if id == callee))
                }
                Suspension::Mutex => assert!(matches!(callee, mir::Callee::MutexWithLock)),
                Suspension::Task => assert!(matches!(callee, mir::Callee::TaskWait)),
                Suspension::Channel => assert!(matches!(
                    callee,
                    mir::Callee::ChannelSend
                        | mir::Callee::ChannelReceive
                        | mir::Callee::Select { .. }
                )),
            }
        }
    }
    let names: Vec<_> = plan
        .machines
        .keys()
        .map(|id| package.function(*id).name.as_str())
        .collect();
    assert!(names.contains(&"leaf"));
    assert!(names.contains(&"parent"));
    assert!(!names.contains(&"plain"));
    assert!(names.iter().any(|name| name.contains("$go")));
}

#[test]
fn io_calls_and_their_awaited_callers_are_polled_but_plain_helpers_stay_ordinary() {
    let (package, _, plan) = plan(
        "package main
import \"zore/io\"
async func ioWait() int { let _, _ = io.ReadLine(); return 1 }
async func fallback() int { return await ioWait() }
async func simple() int { let task = go ioWait(); return await task }
func helper(ch channel<int>) int { let n, _ = ch.receive(); return n }
async func blockingHelper(ch channel<int>) int { return helper(ch) }
func main() {}",
    );
    let names: Vec<_> = plan
        .machines
        .keys()
        .map(|id| package.function(*id).name.as_str())
        .collect();
    assert!(names.contains(&"ioWait"));
    assert!(names.contains(&"fallback"));
    assert!(names.contains(&"simple"));
    assert!(names.contains(&"blockingHelper"));
}

#[test]
fn recursive_calls_are_planned_and_nonblocking_natives_preserve_poll_lowering() {
    let (package, _, plan) = plan("package main
import \"zore/strings\"
async func recurse(n int) string { if n == 0 { return strings.Upper(\"done\") }; return await recurse(n - 1) }
func main() { let t = go recurse(5); println(t.wait()) }");
    let (&id, machine) = plan
        .machines
        .iter()
        .find(|(id, _)| package.function(**id).name == "recurse")
        .unwrap();
    assert!(
        machine
            .suspensions
            .iter()
            .any(|(_, kind)| *kind == Suspension::Call(id))
    );
}

#[test]
fn channel_operations_are_suspensions_without_await_and_keep_cleanup_edges() {
    let (package, program, plan) = plan(
        "package main
async func exchange(ch channel<int>) int {
    ch.send(3)
    let n, _ = ch.receive()
    select { case ch.send(n) {}; case let value, ok = ch.receive() {}; default {} }
    return n
}
async func parent(ch channel<int>) int { return await exchange(ch) }
func main() {}",
    );
    let (&id, machine) = plan
        .machines
        .iter()
        .find(|(id, _)| package.function(**id).name == "exchange")
        .unwrap();
    assert_eq!(machine.suspensions.len(), 3);
    for (block, kind) in &machine.suspensions {
        assert_eq!(*kind, Suspension::Channel);
        let mir::Terminator::Call { unwind, span, .. } =
            &program.bodies[id.0 as usize].blocks[block.0 as usize].terminator
        else {
            panic!("expected call")
        };
        assert!(unwind.is_some());
        assert!(span.end() > span.start());
    }
    assert!(
        plan.machines
            .keys()
            .any(|id| package.function(*id).name == "parent")
    );
}

#[test]
fn native_sleep_is_a_suspension_and_nonwaiting_time_calls_stay_ordinary() {
    let (package, program, plan) = plan("package main
import \"zore/time\"
async func sleeper(ms int) int { let start = time.Millis(); time.Sleep(ms); return time.Millis() - start }
async func parent() int { return await sleeper(1) }
func plain() { time.Sleep(1) }
func main() {}");
    let (&id, machine) = plan
        .machines
        .iter()
        .find(|(id, _)| package.function(**id).name == "sleeper")
        .unwrap();
    assert_eq!(machine.suspensions.len(), 1);
    let (block, kind) = machine.suspensions[0];
    assert_eq!(kind, Suspension::Sleep);
    let mir::Terminator::Call { unwind, span, .. } =
        &program.bodies[id.0 as usize].blocks[block.0 as usize].terminator
    else {
        panic!("expected call")
    };
    assert!(unwind.is_some());
    assert!(span.end() > span.start());
    assert!(
        plan.machines
            .keys()
            .any(|id| package.function(*id).name == "parent")
    );
    assert!(
        !plan
            .machines
            .keys()
            .any(|id| package.function(*id).name == "plain")
    );
}

#[test]
fn mutex_acquisition_suspends_but_callbacks_and_poison_queries_stay_ordinary() {
    let (package, program, plan) = plan(
        "package main
async func run(m Mutex<int>, ch channel<int>) int {
    println(m.isPoisoned())
    return m.withLock(func(n mut int) int { let value, _ = ch.receive(); n += value; return n })
}
func main() {}",
    );
    let (id, machine) = plan
        .machines
        .iter()
        .find(|(id, _)| package.function(**id).name == "run")
        .unwrap();
    assert_eq!(machine.suspensions.len(), 1);
    let (block, kind) = machine.suspensions[0];
    assert_eq!(kind, Suspension::Mutex);
    let mir::Terminator::Call { unwind, span, .. } =
        &program.bodies[id.0 as usize].blocks[block.0 as usize].terminator
    else {
        panic!()
    };
    assert!(unwind.is_some());
    assert!(span.end() > span.start());
}

#[test]
fn io_and_waiting_library_wrappers_keep_spans_cleanup_and_plain_helpers() {
    let (package, program, plan) = plan(
        "package main
import \"zore/net\"
import \"zore/os\"
func helper(path string) string { let text, _ = os.ReadFile(path); return text }
async func run(path string, listener own net.Listener) string {
    let _, _ = listener.Accept()
    let text, _ = os.ReadFile(path)
    return text + helper(path)
}
func main() {}",
    );
    let (_, machine) = plan
        .machines
        .iter()
        .find(|(id, _)| package.function(**id).name == "run")
        .unwrap();
    assert_eq!(machine.suspensions.len(), 2);
    assert!(
        machine
            .suspensions
            .iter()
            .any(|(_, kind)| matches!(kind, Suspension::Io(_)))
    );
    assert!(
        machine
            .suspensions
            .iter()
            .any(|(_, kind)| matches!(kind, Suspension::Call(_)))
    );
    for (id, machine) in &plan.machines {
        for (block, _) in &machine.suspensions {
            let mir::Terminator::Call { span, unwind, .. } =
                &program.bodies[id.0 as usize].blocks[block.0 as usize].terminator
            else {
                panic!()
            };
            assert!(unwind.is_some());
            assert!(span.end() > span.start());
        }
    }
    assert!(
        !plan
            .machines
            .keys()
            .any(|id| package.function(*id).name == "helper")
    );
    assert!(
        !plan
            .machines
            .keys()
            .any(|id| package.function(*id).name.ends_with(".Port"))
    );
}

#[test]
fn an_awaited_call_through_an_async_function_value_is_a_value_suspension_with_cleanup_edges() {
    let (package, program, plan) = plan(
        "package main
async func leaf(n int) int { return n }
async func relay(op async func(int) int, n int) int { return await op(n) }
async func run() int { return await relay(leaf, 1) }
func main() { let t = go run(); println(t.wait()) }",
    );
    let (id, machine) = plan
        .machines
        .iter()
        .find(|(id, _)| package.function(**id).name == "relay")
        .unwrap();
    let body = &program.bodies[id.0 as usize];
    assert!(body.unwind.is_some());
    let values: Vec<_> = machine
        .suspensions
        .iter()
        .filter(|(_, kind)| *kind == Suspension::Value)
        .collect();
    assert_eq!(values.len(), 1);
    let mir::Terminator::Call { callee, unwind, .. } =
        &body.blocks[values[0].0.0 as usize].terminator
    else {
        panic!("a suspension must be a call")
    };
    assert!(matches!(callee, mir::Callee::Value(_)));
    assert!(unwind.is_some());
}
