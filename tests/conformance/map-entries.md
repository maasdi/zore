# Borrowed map-entry coverage

The contract is locked in specification §13.4. Existing lookup result order and
ownership-transferring removal remain unchanged.

| Contract | Executable regression |
| --- | --- |
| Shared/mutable access; shared map captures accepted; overlapping/nested mutation and receiver invalidation rejected | `ownership::map_entries_pair_shared_and_mutable_access_with_alias_rejections` |
| Borrowed-entry moves, view results/capture escape, spawned borrows rejected | `ownership::map_entries_reject_borrow_escape_moves_and_spawned_borrows` |
| Shared external views retain provenance; mutable view-containing entries and shared mutable-view access rejected | `ownership::map_entries_shared_external_views_keep_provenance_and_mutable_view_entries_are_rejected` |
| Parameter modes, key types, callback arity/async/result validation | `ownership::map_entries_reject_invalid_callback_signatures_and_result_types` |
| Generic view constraints and call-once restrictions | `ownership::map_entries_validate_specialized_generic_types_and_preserve_call_once_restrictions` |
| Move entry reads/edits, stable ownership and ordinary remove | `native::map_entries_read_and_edit_move_resources_without_removing_or_cloning` |
| Miss evaluation, callback skipped, presence-first scalar/multiple zero results | `native::map_entries_misses_evaluate_arguments_once_and_return_zero_results` |
| Owned result cleanup, harmless zeros, error propagation and committed mutation | `native::map_entries_owned_results_and_error_propagation_preserve_mutations_and_cleanup` |
| Callback/replacement panics and initialized-part cleanup without double drop | `native::map_entries_mutable_panic_and_replacement_panic_drop_each_initialized_part_once` |
| Copy strings, shared-view reads, unchanged lookup and awaited key evaluation | `native::map_entries_copy_text_shared_views_callbacks_and_async_arguments_run_normally` |
| Owned interface borrows, generic callbacks and reusable capturing callback variables | `native::map_entries_support_interface_borrows_generic_callbacks_and_callback_variables` |

Ownership cases are in `tests/ownership/map_entries.rs`; native cases are in
`tests/codegen/map_entries.rs`. The existing example checker/native runner also
executes `examples/map-entries` and its output fixture. Native cases require
clang/LLVM 15+ and rustc 1.98+ on the supported Linux/macOS hosts.

Direct borrowed lookup, escaping borrowed results, async callbacks, mutable
view-containing entries, key-level disjointness and unrestricted callback
writeback provenance are not claimed.
