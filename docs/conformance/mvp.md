# MVP implementation and evidence inventory

Reviewed baseline: `main` at `f76fc7d802f5c6efeb130b7379e4740e74bc758d`
(2026-10-13, after PR #113). Scope: specification §§39, 46, and 52,
read together with each feature's detailed locked rules. This is a coverage
inventory, not a proof of soundness for every program or a declaration that
the entire MVP is complete.

**Covered** identifies executable assertions for the named behavior.
**Partial** identifies incomplete direct evidence across feature combinations.
**Design pending** identifies an explicitly unresolved contract.
**Extension** identifies behavior excluded from current locked forms.
Markdown scenarios in `tests/conformance/` are requirements, not executed tests.
Tests below are identified by their function names so references survive line
number changes. The earlier [ownership](ownership.md) and
[async/runtime](async-runtime.md) inventories remain historical records;
their follow-up sections supersede the original missing-test lists.

## Feature families in §39

| Requirement | Status and implementation | Executable evidence | Remaining scope |
| --- | --- | --- | --- |
| §39.1 `.ore` files, packages, imports, and visibility | Covered: project loading, per-file imports, package scopes, cycle/visibility checks | [Package tests](../../tests/packages/packages.rs): `files_in_one_folder_share_a_package_and_sibling_folders_do_not`, `exported_functions_types_methods_and_constants_work_across_packages`, `unexported_names_cannot_be_used_from_another_package`; [CLI tests](../../tests/driver/cli.rs): `a_file_is_built_together_with_the_rest_of_its_folder` | Directory targets for check/build/run remain later CLI work, as §45 explicitly permits. |
| §39.1 `let`, `var`, `const` | Covered: local mutability, exact constants, declaration scopes, explicit mixed bindings; immutable package values | [Checker tests](../../tests/typecheck/check.rs): `bindings_types_and_defaults`, `constants_fold_exactly`, `constant_errors_are_compile_time`, `mixed_bindings_declare_individual_locals_and_assign_existing_variables`, `package_level_lets_are_typed_ordered_and_immutable`; [native mixed bindings](../../tests/codegen/mixed_bindings.rs) | Package-level `var` is forbidden by §3.21, not a missing feature. |
| §39.1 Functions and multiple returns | Covered: signatures, result forwarding, evaluation order, completion checks | [Checker tests](../../tests/typecheck/check.rs): `calls_arguments_and_results`, `control_flow_and_return_completeness`; [native suite](../../tests/codegen/native.rs) | No newly identified missing locked form in this review. |
| §39.1 Structs and methods | Covered: nominal types, construction, receiver modes, recursive owners | [Checker tests](../../tests/typecheck/check.rs): `structs_fields_and_literals`, `methods_declare_and_call`, `recursive_owned_types_have_finite_layout_and_move_classification`; [native suite](../../tests/codegen/native.rs): `named_types_compute_with_their_base_representation` | Infinite-size layouts are intentionally rejected; recursive clone/drop remains native-stack bounded. |
| §39.1 Primitive types and strings | Covered: exact constants, checked conversions/arithmetic, Unicode/byte boundaries and operations | [Checker tests](../../tests/typecheck/check.rs): `untyped_constant_arithmetic_and_conversion`, `float_types_and_conversions`, `strings_have_length_index_slice_loops_and_a_rune_conversion`; [lexer tests](../../tests/lexer/lexer.rs); [native suite](../../tests/codegen/native.rs): `runes_and_integers_convert_with_checks`, `strconv_formats_and_parses_floats` | Q13 records conservative lexical choices; it does not block the implemented forms. |
| §39.1 Fixed arrays, borrowed slices, `Array<T>` | Covered: construction, indexing, bounds, length, iteration, mutation, push/pop and whole-value ownership | [Checker tests](../../tests/typecheck/check.rs): `fixed_arrays_literals_and_indexing`, `slice_bounds_and_indices_are_checked_statically`, `collections_have_len_push_and_pop`; [ownership tests](../../tests/ownership/ownership.rs): `fixed_arrays_copy_or_move_on_assignment_by_their_element_type`, `push_and_pop_move_values_and_carry_views`; [native suite](../../tests/codegen/native.rs) | Moving an element out through an index remains an explicit safe rejection. |
| §39.1 Maps | Covered for §13.3's locked construction, Copy lookup, insert/replace, ownership-transferring remove, length and iteration | [Checker tests](../../tests/typecheck/check.rs): `map_lookups_always_produce_presence_then_value`, `map_assignment_targets_a_mutable_map_and_one_entry`; [ownership tests](../../tests/ownership/ownership.rs): `maps_move_whole_and_removed_values_belong_to_the_caller`; [native suite](../../tests/codegen/native.rs) | Borrowed entry access is Q05 design work ([#116](https://github.com/maasdi/zore/issues/116)), not an implicitly specified API. |
| §39.1 Closures | Covered: borrowing/owning captures, function values, call-once values, receiver-capturing synchronous method values | [Checker tests](../../tests/typecheck/check.rs): `closures_become_functions_that_borrow_their_captures`, `call_once_closures_are_bound_with_let_and_called_directly`; [ownership tests](../../tests/ownership/ownership.rs): `a_closure_cannot_outlive_what_it_captures`, `an_escaping_closure_owns_copies_and_moves_of_its_captures`; [native suite](../../tests/codegen/native.rs): `method_values_destroy_an_owned_receiver_exactly_once` | Async closure literals and concrete async method values are delivered separately by #117 under §16.7; see [coverage](../../tests/conformance/async-callables.md). |
| §39.2 Copy/Move semantics and derived classification | Covered for every §6.1 primitive and the listed library resources | [Checker tests](../../tests/typecheck/check.rs): `drop_methods_make_structs_move`; [ownership tests](../../tests/ownership/ownership.rs): `move_values_transfer_and_ordinary_calls_borrow`, `fixed_arrays_copy_or_move_on_assignment_by_their_element_type` | The #114 evidence below covers file buffers, metadata, Listener and Conn. No public `os.File` handle is currently defined; these cases do not prove classification for every possible composite. |
| §39.2 Borrow by default, `mut`, `own`, borrow checking | Covered: parameter contracts, places, exclusive loans, captures, partial moves and reinitialization | [Checker tests](../../tests/typecheck/check.rs): `mut_parameters_and_receivers_require_mutable_places`; [ownership tests](../../tests/ownership/ownership.rs): `partial_moves_track_fields_and_reinitialization`, `shared_and_mutable_views_cannot_overlap_while_live`, `backing_owners_cannot_be_moved_replaced_or_dropped_while_viewed` | Only mutable places may be assigned or passed as `mut` (§11.6); ownership alone does not grant mutability. Generic restrictions follow §22.1. |
| §39.2 Inferred lifetimes | Covered for named loan/provenance cases, including nested returned containers and later calls through clones | [Ownership tests](../../tests/ownership/ownership.rs): `the_owner_is_usable_again_after_the_view_last_use`, `contracts_bind_results_only_to_the_inputs_they_derive_from`, `recursive_functions_get_consistent_contracts`, `an_owner_cannot_be_returned_alongside_its_own_view` | The #114 evidence below adds these direct cases; unproven provenance is still conservatively rejected. |
| §39.2 Deterministic destruction, automatic/explicit drop, clone | Covered for named normal/error/panic exits and destructor-observed views across channel/timer/mutex/socket waits | [Ownership tests](../../tests/ownership/ownership.rs): `clone_borrows_its_argument_and_never_consumes_it`, `a_custom_drop_cannot_export_the_value_being_destroyed`; [native suite](../../tests/codegen/native.rs): `a_drop_that_reads_a_view_keeps_the_backing_owner_live_across_suspension`, `a_drop_that_reads_a_view_still_runs_when_a_task_panics_after_suspension` | The #114 evidence below adds observed timer/mutex/socket suspensions and collection-field resurrection rejection. Early owned-value destruction is a direction, not a required timing schedule (§14.2). |
| §39.3 `error`, multiple-return errors, `?`, `panic`, no hidden ordinary exceptions | Covered: singular trailing error, path-sensitive use, explicit discard, zero-fill propagation, cleanup and panic containment | [Checker tests](../../tests/typecheck/check.rs): `error_results_require_one_trailing_position`, `named_errors_require_use_on_every_path`, `propagation_checks_call_shape_and_return_contract`; [native suite](../../tests/codegen/native.rs): `panic_calls_report_their_message_and_location_after_cleanup`; [mixed native tests](../../tests/codegen/mixed_bindings.rs) | No new error-control syntax is needed to reuse one mutable error variable. |
| §39.4 `async`, `await`, state machines and scheduler | Covered: persistent frames, child polling, waiting operations, cooperative budgets, worker compensation | [Async lowering tests](../../tests/async/lowering.rs): `calls_and_task_awaits_keep_their_cleanup_edges_and_spans`, `loop_budget_points_preserve_live_storage_and_resume_at_block_entry`; [runtime scheduler tests](../../runtime/src/scheduler.rs); [native suite](../../tests/codegen/native.rs) | General frame shrinking and forced preemption are optimization/future-runtime work, not established MVP requirements. |
| §39.4 `go`, Task, wait/retrieval, captures and task borrowing | Covered under independent-input restrictions: owning spawns, single retrieval, detach, panic re-raise, process-exit abandonment | [Checker tests](../../tests/typecheck/check.rs): `spawned_inputs_must_not_borrow_the_spawner`, `wait_and_await_follow_the_async_boundary`; [native suite](../../tests/codegen/native.rs): `go_runs_function_values_call_once_closures_and_owning_arguments`, `process_exit_abandons_a_pending_detached_task_and_its_buffered_value`; [runtime task tests](../../runtime/src/task.rs) | Plain and async scoped tasks are an optional implemented extension under §§18.12–18.13; see [scope coverage](../../tests/conformance/scoped-tasks.md). A spawn callee in a field/element/map is a recorded restriction, not proof that ordinary task borrowing is unsafe. |
| §39.4 Channels, buffers, close, Copy handles and message transfer | Covered: queue operations, select, close wakeups/panics, zero channel, buffer destruction, Move messages and shared handles | [Checker tests](../../tests/typecheck/check.rs): `channel_handles_are_copy_and_never_nil`, `sending_a_move_value_gives_it_up`; [runtime channel tests](../../runtime/src/channel.rs); [native suite](../../tests/codegen/native.rs) | Exhaustive fairness for every possible select pattern has no direct proof; existing rotation/wake tests cover the identified cases. |
| §39.4 Borrowing across await and async I/O | Covered for valid persistent backing and rejected unsafe inputs; plain/polled parity and controlled-pool progress | [Ownership tests](../../tests/ownership/ownership.rs); [native suite](../../tests/codegen/native.rs): `io_poll_files_and_byte_views_preserve_results_and_cleanup`, `pending_async_accepts_leave_workers_for_an_unrelated_task`, `cancelling_from_another_task_wakes_a_pending_done_select_exactly_once` | Conservative safe rejection is explicitly allowed by §17.6. TLS, HTTP client, HTTP/2 and UDP are not delivered by the HTTP server work. |
| §39.5 Lexer, parser, AST, name resolution, checker and HIR | Covered: recovery, spans, scopes, type/ownership diagnostics and backend-independent checking | [Lexer](../../tests/lexer/lexer.rs), [parser](../../tests/parser/parser.rs), [checker](../../tests/typecheck/check.rs), [packages](../../tests/packages/packages.rs), [CLI](../../tests/driver/cli.rs) tests | Lexical questions remain localized in Q13. No new self-hosting stage is scheduled. |
| §39.5 Ownership/borrow checking, regions, MIR/CFG, async lowering and drop insertion | Covered for named observable behavior; partial evidence across the combinations above | [Ownership](../../tests/ownership/ownership.rs), [async lowering](../../tests/async/lowering.rs), [native](../../tests/codegen/native.rs) tests; [MIR](../../compiler/src/mir), [ownership](../../compiler/src/ownership), [drop insertion](../../compiler/src/dropck) | Full-MVP completion remains conditional on the remaining evidence inventory and any defects it uncovers. |
| §39.5 LLVM backend and native executable | Covered on Linux/macOS and Windows x64 MSVC through check/build/run and native subprocess tests | [Native suite](../../tests/codegen/native.rs); [.github/workflows/ci.yml](../../.github/workflows/ci.yml) | Host/toolchain evidence and Windows API limits are in [platform support](../platform-support.md); cross-compilation remains out of MVP. |
| §39.5 High-quality diagnostics | Covered for source rendering and named semantic cases; partial as a universal quality claim | [Source diagnostics](../../tests/diagnostics/source_diagnostics.rs): `utf8_spans_are_bytes_but_columns_are_unicode_scalars`, `diagnostics_retain_primary_and_related_locations_in_different_files`; [checker](../../tests/typecheck/check.rs), [ownership](../../tests/ownership/ownership.rs) | Diagnostics must keep pointing to creation/move/loan/conflict when available (§24.2). The illustrative error code is not a locked code registry. |

§22 generics/interfaces and §37 standard-library rules also have executable
checker/package/native suites, including ownership through interfaces,
generic specialization, `std/io`, JSON, HTTP and signals. Their detailed rules
include explicit restrictions; the feature list does not erase those rules.

## Test layers in §46

| Section | Executable layer and assertions |
| --- | --- |
| §46.1 Lexer | `tests/lexer/lexer.rs` covers token spelling/kinds, decoded literals, Unicode byte spans, invalid input, EOF, automatic semicolons and recovery. |
| §46.2 Parser | `tests/parser/parser.rs` checks AST shape, precedence, declarations, control flow, spans, diagnostics and recovery. |
| §46.3 Semantic | `tests/typecheck/check.rs` and `tests/packages/packages.rs` pair accepted programs with rejected types, names, result shapes, mutability, visibility and initializer dependencies. |
| §46.4 Ownership | `tests/ownership/ownership.rs` covers Copy/Move, double drop, overlapping loans, partial moves, branches, loops, result provenance, capture lifetime and task inputs; async lowering/native tests cover actual suspension. |
| §46.5 Code generation | `tests/codegen/native.rs` and its registered HTTP/JSON/signal/mixed-binding modules compile and execute, asserting status/stdout/stderr, cleanup ordering, bounds/arithmetic panics and resource leak checks. |
| §46.6 Async/concurrency | Async lowering plus runtime/native tests assert persistence, task results/errors/panics, detach and abandonment, channel ownership/close/buffers, wait progress and worker compensation. Controlled two-worker fixtures make the critical pending states observable. |

The supported baseline's [CI run](https://github.com/maasdi/zore/actions/runs/38350681714)
passed formatting, Clippy with warnings denied, build, documentation and the
full all-target suite on Ubuntu/macOS: 789 tests passed on each platform.
The intentionally ignored subprocess fixture is explicitly exercised by the
harness tests. These results belong to the reviewed baseline; new formatter
checks are reported in its PR, not retroactively counted in that run.

## Final invariants in §52

| Invariants | Current evidence and limit |
| --- | --- |
| 1–4: borrowed default, explicit `mut` and callee `own`, ordinary call sites | ParamMode is represented separately in AST/resolution/HIR; parameter-mode, mutable-place and native caller-update/cleanup cases assert these contracts. |
| 5–6: type-derived Copy/Move and struct classification | Type classification feeds ownership transfer and cleanup; resource/aggregate assignment and recursive-layout tests cover the named cases. The #114 matrix covers every primitive, shared handles, file buffers/metadata and socket handles. |
| 7–8: deterministic cleanup, rejected use-after-move/double-drop | Separate ownership and drop insertion passes; native success/error/panic/partial-construction tests assert exact destruction. The #114 cases add controlled timer, mutex and socket suspension on normal and panic cleanup paths. |
| 9: inferred hidden lifetimes | Region liveness, return/output provenance, reborrow and escape tests enforce lifetime relationships without source annotations. The #114 parser cases directly reject source lifetime annotations. |
| 10–11: explicit errors and propagation with cleanup | Error-use analysis and native propagation/zero-fill/cleanup tests, including awaited and mixed-target calls. |
| 12–13: ordinary ownership in async and tasks | MIR ownership runs before async lowering; captured/spawned inputs are copied/moved or conservatively rejected, with persistent-view and cleanup tests. |
| 14–15: ordinary message ownership and Copy channel capabilities | Channel handle retain/release, Move send/receive transfer, buffer cleanup and shared-handle tests. |
| 16: no tracing GC requirement | Native owners, explicit drop edges and reference-counted Copy capabilities; no tracing collector is linked. Leak tests supplement cleanup assertions. |
| 17: no source raw pointers or unsafe | Parser/keyword rejection and source Type/Expr variants expose no unsafe/raw-pointer operations. Native runtime pointers remain an implementation detail. |
| 18: safe rejection | Negative cases cover unproven view/task lifetimes and unsupported forms; accepting more programs requires the same proof and cleanup machinery. |
| 19: distinct AST/HIR/MIR | Separate stage modules and representations; semantic checking does not invoke LLVM. |
| 20: eventual self-hosting architecture | Existing Zore lexer/parser comparisons and ordinary data-oriented compiler structures remain available. Further self-hosting implementation is deferred by maintainer request. |

## Direct evidence added for issue #114

Reviewed against `main` at `9d9cccd7193e217b3b03484d4af6cb0b526a16a1`,
after PR #122. The original baseline/CI counts above remain historical.
These cases close the specific evidence gaps identified in #114 without
changing source-language rules or production runtime behavior.

| Acceptance case | Executable assertion |
| --- | --- |
| Nested shared views in returned arrays/maps | [Ownership suite](../../tests/ownership/ownership.rs): `nested_array_and_map_results_keep_external_view_provenance` pairs acceptance with mutation/escape rejection for `Array<Array<View>>`, `map[int]Array<View>`, and `Array<map[int]View>`. An owned forwarding call retains external provenance. Map removal or Copy lookup then reads the nested view, rather than only checking collection length. |
| Later call through a clone holding a shared view | [Ownership suite](../../tests/ownership/ownership.rs): `calls_through_cloned_containers_preserve_shared_view_origins` passes a Move clone through an owned forwarding call, with the original destroyed before the conflicting write; escape of local backing is also rejected. [Native suite](../../tests/codegen/conformance.rs): `nested_returned_and_cloned_views_keep_their_backing_through_moves_and_calls` reads every case, then successfully mutates backing after the view scopes end. |
| Destructor-observed views across timer suspension | [Native suite](../../tests/codegen/conformance.rs): `destructor_views_survive_an_observed_timer_suspend_and_both_cleanup_paths` registers a long `time.Sleep`, observes the task Idle after Pending, then expires the registered timer using a test-only hook. Both normal return and panic read fixed-array and owned-array backing in `drop`, exactly once and before owner destruction. |
| Destructor-observed views across mutex suspension | [Native suite](../../tests/codegen/conformance.rs): `destructor_views_survive_an_observed_mutex_suspend_and_both_cleanup_paths` holds a lock while the child reaches Pending in acquisition; only then is the lock released. The same normal/panic drop-order assertions apply. |
| Destructor-observed views across I/O suspension | [Native suite](../../tests/codegen/conformance.rs): `destructor_views_survive_an_observed_socket_suspend_and_both_cleanup_paths` observes a pending TCP read before the peer writes its byte. The same normal/panic drop-order assertions apply. |
| Source lifetime syntax is excluded | [Parser suite](../../tests/parser/parser.rs): `source_lifetime_annotations_are_rejected_while_slice_lifetimes_are_inferred` rejects lifetime parameters and parameter/result/field annotations, with diagnostics at the offending syntax; ordinary slice signatures and rune literals parse. |
| Classification and transfer matrix | [Ownership suite](../../tests/ownership/ownership.rs): `primitive_and_library_resource_assignments_follow_copy_move_classification` checks all 16 §6.1 primitives, `error`, channel/Mutex handles, file bytes, `os.FileInfo`, `net.Listener` and `net.Conn`. Copy sources remain usable; Move resources reject use after transfer. [Native suite](../../tests/codegen/conformance.rs): `moved_file_buffers_and_socket_handles_keep_their_single_owner` moves real file-read buffers and live socket handles, copies metadata, transfers a byte and observes EOF after client close. |
| Resurrection through a collection field | [Ownership suite](../../tests/ownership/ownership.rs): `a_custom_drop_cannot_resurrect_a_resource_through_a_collection_field` rejects `r.rescued.push(r)` as moving a borrowed receiver, while insertion of an independent fresh resource is accepted. [Native suite](../../tests/codegen/conformance.rs): `a_destructor_can_add_a_fresh_resource_without_resurrecting_itself` asserts destruction of the original followed by its fresh child, each once. |

All three suspension fixtures run on a private two-worker pool with a
10-second scheduler checkpoint and a 30-second subprocess bound. The timer
hook invokes the existing expiry collector/completion path after suspension;
it does not measure wall-clock timing. Real elapsed-time timer behavior retains
its existing runtime/native tests. Socket tests use loopback endpoints; file
tests use temporary directories. Native runs enable resource leak checks.

The current file API returns owned `Array<byte>` data and Copy `FileInfo`
metadata. §10.3's illustrative `File` name does not define an additional
standard-library handle API. A new File abstraction would require its own
contract, not a guessed conformance fixture.

The focused cases demonstrate no new compiler/runtime defect. The only
runtime additions are excluded unless `zore_test_hooks` is enabled. General
proofs for all alias shapes, suspension combinations and destructor programs
are not implied by these named assertions.

## Work classification and next steps

1. The specific direct evidence gaps in #114 are covered by the cases above.
   New demonstrated defects need their own reproducer and implementation work.
2. `zore fmt` is delivered in PR #122; next is the test-runner contract
   and implementation in #115. Tooling is §45 direction; it does not prove a
   language safety invariant.
3. Resolve borrowed map-entry Q05 in #116. Async closures/method values (#117)
   required explicit specification changes. Plain scoped tasks (#120) now use
   locked §§18.12–18.13, including direct async scope cleanup (#132).
4. Extend existing performance baselines (#118) and establish Windows host
   evidence (#119). Avoid claiming missing benchmarks: three workload families
   already exist under `benchmarks/`.
5. Keep further self-hosting deferred. Package-level mutable values remain
   intentionally forbidden; no implementation ticket is created for them.

This review did not demonstrate a new safety defect. It also did not execute
every Markdown scenario or establish exhaustive conformance for every feature
combination. The #114 follow-up supplies the named direct assertions; broader
conformance and other roadmap work remain separate.
