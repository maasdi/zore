use zore::async_lowering::{self, LocalStorage, Suspension};
use zore::source::SourceMap;
use zore::{check, codegen, dropck, hir, mir};

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

fn storage_for<'a>(
    package: &hir::Package,
    program: &'a mir::Program,
    plan: &'a async_lowering::Plan,
    name: &str,
) -> (&'a mir::Body, &'a [LocalStorage]) {
    let (&id, machine) = plan
        .machines
        .iter()
        .find(|(id, _)| package.function(**id).name == name)
        .unwrap();
    (&program.bodies[id.0 as usize], &machine.storage)
}

fn named_storage(body: &mir::Body, storage: &[LocalStorage], name: &str) -> LocalStorage {
    body.locals
        .iter()
        .zip(storage)
        .find(|(local, _)| local.name.as_deref() == Some(name))
        .unwrap()
        .1
        .to_owned()
}

#[test]
fn storage_distinguishes_poll_local_and_resume_values() {
    let source = "package main
async func run(ch channel<int>) int {
    var early = [int; 512]{VALUES}
    early[0] = 7
    let before = early[0]
    ch.send(before)
    var late = [int; 512]{VALUES}
    late[0] = 9
    return late[0]
}
func main() {}"
        .replace("VALUES", &vec!["0"; 512].join(", "));
    let (package, program, plan) = plan(&source);
    let (body, storage) = storage_for(&package, &program, &plan, "run");
    assert_eq!(named_storage(body, storage, "early"), LocalStorage::Poll);
    assert_eq!(named_storage(body, storage, "late"), LocalStorage::Frame);
    assert_eq!(named_storage(body, storage, "ch"), LocalStorage::Frame);
}

#[test]
fn storage_keeps_borrowed_and_cleanup_values_stable() {
    let (package, program, plan) = plan(
        "package main
async func run(ch channel<int>) int {
    var borrowed = [int; 4]{0, 0, 0, 0}
    borrowed[0] = 3
    let view = borrowed[:]
    let value = view[0]
    let owned = Array<int>{1, 2}
    ch.send(value)
    println(owned.len())
    return 1
}
func main() {}",
    );
    let (body, storage) = storage_for(&package, &program, &plan, "run");
    assert_eq!(
        named_storage(body, storage, "borrowed"),
        LocalStorage::Frame
    );
    assert_eq!(named_storage(body, storage, "owned"), LocalStorage::Frame);
}

#[test]
fn implicit_mutex_and_io_waits_preserve_later_values() {
    let (package, program, plan) = plan(
        "package main
import \"zore/os\"
async func run(m Mutex<int>, path string) int {
    let early = 7
    println(early)
    let held = 3
    m.withLock(func(n mut int) { n += 1 })
    let text, _ = os.ReadFile(path)
    return held + text.len()
}
func main() {}",
    );
    let (body, storage) = storage_for(&package, &program, &plan, "run");
    assert_eq!(named_storage(body, storage, "early"), LocalStorage::Poll);
    assert_eq!(named_storage(body, storage, "held"), LocalStorage::Frame);
    assert!(
        plan.machines[&body.function]
            .suspensions
            .iter()
            .any(|(_, kind)| *kind == Suspension::Mutex)
    );
    assert!(
        plan.machines[&body.function]
            .suspensions
            .iter()
            .any(|(_, kind)| matches!(kind, Suspension::Io(_)))
    );
}

#[test]
fn cleanup_paths_conservatively_retain_owned_values() {
    let (package, program, plan) = plan(
        "package main
type Job struct { Name string }
func (j mut Job) drop() {}
async func run(ch channel<int>) {
    { let short = Job{Name: \"short\"} }
    let cleanup = Job{Name: \"cleanup\"}
    ch.send(1)
}
func main() {}",
    );
    let (body, storage) = storage_for(&package, &program, &plan, "run");
    assert_eq!(named_storage(body, storage, "short"), LocalStorage::Frame);
    assert_eq!(named_storage(body, storage, "cleanup"), LocalStorage::Frame);
}

#[test]
fn captured_local_keeps_address_stable_across_suspend() {
    let (package, program, plan) = plan(
        "package main
async func run(ch channel<int>) {
    let captured = [int; 2]{1, 2}
    let callback = func() int { return captured[0] }
    println(callback())
    ch.send(1)
}
func main() {}",
    );
    let (body, storage) = storage_for(&package, &program, &plan, "run");
    assert_eq!(
        named_storage(body, storage, "captured"),
        LocalStorage::Frame
    );
}

#[test]
fn poll_local_arrays_leave_the_frame_without_reusing_slots() {
    let values = vec!["0"; 512].join(", ");
    let source = "package main
async func run(ch channel<int>) int {
    let early = [int; 512]{VALUES}
    let before = early[0]
    ch.send(before)
    let late = [int; 512]{VALUES}
    return late[0]
}
func main() {}"
        .replace("VALUES", &values);
    let mut sources = SourceMap::new();
    let id = sources.add("async.ore", source).unwrap();
    let checked = check::check_file(sources.file(id).unwrap());
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    let package = checked.package.unwrap();
    let mut program = mir::lower::lower(&package);
    dropck::insert(&package, &mut program);
    let mut plan = async_lowering::lower(&package, &program);
    let ir = codegen::emit(&package, &program, &plan, &sources).unwrap();
    let frame = ir
        .lines()
        .find(|line| line.contains("AsyncFrame.0\" = type"))
        .unwrap();
    let retained_arrays = frame.matches("[512 x i64]").count();
    assert_eq!(retained_arrays, 2);
    assert!(ir.contains("alloca [512 x i64]"));

    for machine in plan.machines.values_mut() {
        machine.storage.fill(LocalStorage::Frame);
    }
    let baseline = codegen::emit(&package, &program, &plan, &sources).unwrap();
    let baseline_frame = baseline
        .lines()
        .find(|line| line.contains("AsyncFrame.0\" = type"))
        .unwrap();
    assert_eq!(baseline_frame.matches("[512 x i64]").count(), 4);
}

#[test]
fn loop_budget_points_preserve_live_storage_and_resume_at_block_entry() {
    let source = "package main
async func count() int {
    let seed = 3
    var total = seed
    for total < 1000 { total += 1 }
    return total
}
func main() {}";
    let (package, program, plan) = plan(source);
    let (body, storage) = storage_for(&package, &program, &plan, "count");
    assert_eq!(plan.machines[&body.function].budget_blocks.len(), 1);
    assert_eq!(named_storage(body, storage, "seed"), LocalStorage::Poll);
    assert_eq!(named_storage(body, storage, "total"), LocalStorage::Frame);

    let mut sources = SourceMap::new();
    sources.add("async.ore", source.to_owned()).unwrap();
    let ir = codegen::emit(&package, &program, &plan, &sources).unwrap();
    let poll = ir
        .split("define private i8 @\"main.count$async$poll\"")
        .nth(1)
        .unwrap()
        .split("\n}\n")
        .next()
        .unwrap();
    assert!(poll.matches("load i16, ptr %context").count() >= 2);
    assert!(poll.contains("call void @zore_budget_yield"));
    for block in &plan.machines[&body.function].budget_blocks {
        assert!(poll.contains(&format!("label %bb{}", block.0)));
    }
}

#[test]
fn budget_points_follow_loop_headers_instead_of_block_numbers() {
    let (package, program, plan) = plan(
        "package main
async func branch(value int) int {
    if value > 0 { return value + 1 }
    return value - 1
}
async func nested() int {
    var sum = 0
    for var outer = 0; outer < 3; outer += 1 {
        for var inner = 0; inner < 4; inner += 1 { sum += 1 }
    }
    return sum
}
func main() {}",
    );
    let (branch, _) = storage_for(&package, &program, &plan, "branch");
    let (nested, _) = storage_for(&package, &program, &plan, "nested");
    assert!(plan.machines[&branch.function].budget_blocks.is_empty());
    assert_eq!(plan.machines[&nested.function].budget_blocks.len(), 2);
}

#[test]
fn loop_temporaries_rebuilt_after_budget_resume_use_poll_storage() {
    let values = vec!["0"; 512].join(", ");
    let source = "package main
async func run() int {
    let anchor = [int; 512]{VALUES}
    var total = 0
    for var i = 0; i < 1000; i += 1 {
        let scratch = [int; 512]{VALUES}
        total += anchor[0] + scratch[0]
    }
    return total
}
func main() {}"
        .replace("VALUES", &values);
    let (package, program, mut plan) = plan(&source);
    let (body, storage) = storage_for(&package, &program, &plan, "run");
    assert_eq!(named_storage(body, storage, "anchor"), LocalStorage::Frame);
    assert_eq!(named_storage(body, storage, "scratch"), LocalStorage::Poll);
    assert_eq!(named_storage(body, storage, "total"), LocalStorage::Frame);

    let mut sources = SourceMap::new();
    sources.add("async.ore", source).unwrap();
    let ir = codegen::emit(&package, &program, &plan, &sources).unwrap();
    let frame = ir
        .lines()
        .find(|line| line.contains("AsyncFrame.0\" = type"))
        .unwrap();
    assert_eq!(frame.matches("[512 x i64]").count(), 1);
    assert!(ir.contains("alloca [512 x i64]"));

    for machine in plan.machines.values_mut() {
        machine.storage.fill(LocalStorage::Frame);
    }
    let baseline = codegen::emit(&package, &program, &plan, &sources).unwrap();
    let baseline_frame = baseline
        .lines()
        .find(|line| line.contains("AsyncFrame.0\" = type"))
        .unwrap();
    assert!(baseline_frame.matches("[512 x i64]").count() > frame.matches("[512 x i64]").count());
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
