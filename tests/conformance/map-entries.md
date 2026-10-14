# Borrowed map-entry coverage

The conditional binding contract is locked in specification §13.4. Copied lookup
result order and ownership-transferring removal remain unchanged.

| Contract | Executable regression |
| --- | --- |
| Shared/mutable access, whole-block aliases and receiver invalidation rejected; missing branch releases borrow | `ownership::map_entries_pair_shared_and_mutable_access_with_alias_rejections` |
| Entry/field moves, returned/outer views, closures and spawned borrows rejected | `ownership::map_entries_reject_borrow_escape_moves_and_spawned_borrows` |
| External-view provenance; mutable view entries and shared mutable capabilities rejected | `ownership::map_entries_shared_external_views_keep_provenance_and_mutable_view_entries_are_rejected` |
| Header/arity/key validation, branch-only scope, shadowing and ordinary comparison | `ownership::map_entries_validate_headers_scope_and_key_types` |
| Generic restrictions and ordinary return/error/loop/async checking | `ownership::map_entries_validate_generic_types_and_allow_independent_control_flow` |
| Owned interface shared/mutable methods and generic return | `native::map_entries_support_interface_borrows_and_generic_functions` |
| Move reads/edits without cloning; ordinary removal | `native::map_entries_read_and_edit_move_resources_without_removing_or_cloning` |
| Receiver/key single evaluation, temporary lifetime, misses and mixed else-if chains | `native::map_entries_misses_evaluate_once_release_borrows_and_select_else_branches` |
| Return, propagated errors, break and continue cleanup; committed mutations | `native::map_entries_return_errors_break_and_continue_preserve_mutations_and_cleanup` |
| Body/whole-entry/field replacement panic and exact initialized-part cleanup | `native::map_entries_mutable_panic_and_replacement_panic_drop_each_initialized_part_once` |
| Nested mutable guards, scope-local destructor panic and panic after scope | `native::map_entries_nested_guards_and_scope_destructor_panics_clean_up_in_order` |
| Move cleanup after timer suspension and panic during awaited key | `native::map_entries_async_move_cleanup_survives_timer_suspension_and_key_panics` |
| Copy/text/view access, unchanged lookup, awaited keys and shared/mutable block bodies | `native::map_entries_copy_text_shared_views_and_awaited_keys_and_bodies_run_normally` |

Self-hosted parser AST modes/names and malformed-header diagnostics match the
Rust frontend in `selfhost_parser::conditional_entry_binding_headers_and_diagnostics_match`;
the existing repository/specification and generated/mutated-source corpora also run.

Parser header and line-boundary cases are in `tests/parser/parser.rs`; formatter
mode preservation and idempotence are in `tests/format/format.rs`.

Ownership cases are in `tests/ownership/map_entries.rs`; native cases are in
`tests/codegen/map_entries.rs`. The existing example checker/native runner also
executes `examples/map-entries` and its output fixture. Native cases require
clang/LLVM 15+ and rustc 1.98+ on the supported Linux/macOS hosts.

Public entry handles, escaping map-owned views, mutable view-containing entries,
and key-level disjointness are not claimed.
