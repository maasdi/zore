# Expression evaluation order conformance cases

Authority: spec §7.5, §15.3, and §17. These are pending semantic/lowering/native
tests, not executable coverage. Use fixture functions with observable event logs,
appropriate return types, and valid ownership contracts when these stages exist.

| Scenario | Expected result |
| --- | --- |
| `combine(first(), second())` | Log first evaluation, second evaluation, then combine invocation |
| `receiver().method(first(), second())` | Receiver evaluation precedes first and second argument evaluations |
| `outer(inner(first(), second()), third())` | Evaluate first, second, invoke inner, evaluate third, invoke outer |
| Binary expression with effectful left and right operands | Evaluate left operand before right, according to the eventual parsed tree |
| `combine(first()?, second())` where first propagates an error | Neither second nor combine executes |
| Same expression where first succeeds | Evaluate second, then invoke combine |
| Earlier operand returns an error value without propagation | No implicit early exit; evaluate later operand if the expression is well-typed |
| First argument creates an owned temporary, second propagates error | Callee not invoked; required temporary cleanup occurs once before return |
| `combine(await first(), second())` | Second evaluation waits for explicit await to complete |
| Earlier argument creates a Task handle | Do not insert an implicit wait for the task to finish |
| Earlier argument moves a resource, later argument uses that moved resource | Reject invalid later use; do not reorder to make it pass |
| Short-circuit expression with skipped right operand | No right-side effects, once operator semantics are locked |

Use explicit synchronization for suspension tests; do not rely on sleeps or
assume an ordering of unrelated tasks. Borrow activation, assignment sequencing,
initializer order, panic cleanup, and operator grammar need their own decisions.
