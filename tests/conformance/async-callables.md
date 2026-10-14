# Async captured callable coverage

The extension is locked in specification §§16.1–16.7 and uses existing async
function types, capture inference, task restrictions and persistent frames.

Required acceptance and paired rejection:
- Parse `async func(...) ... { ... }` with the same header/ASI rules as plain
  literals; preserve the async property in both frontend ASTs and formatting.
- Type async literals and shared/mutable/own receiver method values; reject bare
  async calls, sync/async conversions, generic method values and invalid receivers.
- Check nested literal async contexts independently; await in a plain nested
  literal remains invalid, while an async literal in a plain body may be spawned.
- Preserve shared/exclusive capture loans, views, nested capture chains and
  whole-call exclusivity across await; reject escape of temporary/local backing.
- Preserve reusable owning environments across repeated awaits and storage in
  fields; release call-once captures once after return, error, panic or no call.
- Spawn async literals and method values only with independent captures; reject
  borrowed Move parameters, external views, mutable parameters, Copy mutation,
  unknown callable inputs, moved callables and repeated call-once calls.
- Lower captured bodies and async method wrappers into polled child frames;
  retain addresses, drop flags and borrowing environments across real waits.
- Execute partial moves, nested awaits, replacement and initializer failures,
  propagation and destructor panic without leaks or duplicate destruction.

| Evidence | Executable regression |
| --- | --- |
| Literal syntax, spans, modes and malformed headers | `parser::async_literals_preserve_flags_spans_nested_modes_and_header_rules` |
| Both frontend ASTs and diagnostics | `selfhost_parser::async_literal_headers_and_nested_body_modes_match` |
| Formatting and nested modes | `format::async_literal_formatting_preserves_nested_body_modes` |
| Signature identity, independent body contexts and async call requirements | `check::async_literals_keep_async_types_and_independent_nested_body_contexts` |
| Concrete struct/named-type method signatures and invalid receivers | `check::async_method_values_keep_receiver_modes_and_existing_place_restrictions` |
| Concrete method wrappers specialize generic capture roots and nested literals; borrowed Move roots stay rejected | `check::concrete_method_values_specialize_captured_generic_roots_and_nested_literals` |
| Suspension loans, nested aliases, results and local-view escapes | `ownership::async_captures_keep_loans_across_suspension_and_release_after_last_use` |
| Call-once moves, task capture independence and unknown callable inputs | `ownership::async_call_once_and_spawned_inputs_follow_ordinary_ownership_rules` |
| Receiver loans and capture writeback provenance | `ownership::async_method_captures_and_writeback_keep_receiver_and_backing_exclusive` |
| Captures and callers pinned in frame storage; callable suspension edges | `async_lowering::async_captured_callables_keep_captures_and_callers_in_persistent_frames` |
| Real waiting operations and nested capture environments | `native::async_borrowing_literals_and_nested_captures_survive_real_waits` |
| Returned and field-stored owning callables survive repeated awaits | `native::reusable_owning_async_literals_keep_environments_after_each_await` |
| Shared/mutable/own methods, field paths, named receivers and uncalled owners | `native::async_method_values_borrow_field_receivers_and_consume_owned_receivers_once` |
| Spawned literal/local/method/call-once cleanup | `native::spawned_async_literals_method_values_and_call_once_jobs_destroy_captures_once` |
| Error propagation and partial capture moves | `native::async_call_once_error_paths_drop_locals_and_remaining_capture_parts` |
| Borrowed/owned body panics and destructor panic after suspension | `native::async_capture_panics_clean_initialized_parts_and_callers_without_leaks` |
| Owned callable arguments, borrowed views and loop environments | `native::async_callable_arguments_and_views_survive_waits_with_independent_cleanup` |
| Callee/argument initialization and replacement destructor failure | `native::async_callable_argument_and_environment_replacement_panics_clean_every_owner` |
| Replacement and detached-task ownership | `native::async_closure_replacement_and_detached_execution_preserve_owned_lifetimes` |
| Plain/async method wrappers over generic roots, multiple instantiations, mutable/nested captures and Move cleanup | `native::method_values_specialize_generic_capture_roots_for_plain_async_and_nested_calls` |
| Capture destruction panic discards initialized tuple/error results before wait/await rethrows | `native::task_capture_panics_destroy_returned_results_before_wait_or_await_rethrows` |
| Detachment around result cleanup, drop-completion signaling and released resources | `native::detached_tasks_drop_returned_resources_when_capture_destruction_panics` |
| Body panics skip uninitialized result slots | `native::task_body_panics_do_not_destroy_uninitialized_results` |
| Result destructor panics during capture-panic cleanup abort on Linux/macOS | `native::a_second_panic_while_discarding_task_results_aborts` |

The existing example checker/native runner also executes `examples/async-callables`.
Native assertions require the supported Rust/clang toolchain and check for leaks.
Generic and interface method values, capture writeback from parameters, task
results containing views/callables, and scoped tasks remain excluded. Accepted
ordinary forms do not override conservative rejection when lifetime cannot be proven.
