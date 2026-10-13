# Application test contract coverage

Specification §45.1 is locked. Executable coverage lives in
`tests/driver/testing.rs`, registered as Cargo's `application_tests` suite.

| Contract | Regression |
| --- | --- |
| No-result and `error` signatures; normal/error/`?`/panic cleanup; later tests continue | `sample_runs_both_signatures_and_cleans_up_on_return_error_and_panic` |
| Ordinary-file versus test-file selection; full-package checking | `file_selection_checks_the_whole_package_but_runs_only_selected_tests` |
| Private helpers, hidden imports, initialization per process, naming rules, methods skipped, async work waited | `discovery_uses_private_helpers_and_hygienic_imports_with_fresh_initialization` |
| Invalid async/generic/parameter/result signatures diagnosed before native build | `invalid_test_signatures_are_diagnostics_before_native_building` |
| Ownership and error-use rejection | `tests_reuse_ordinary_ownership_and_error_use_checking` |
| Ordinary builds/imports exclude tests; direct test targets rejected outside `test` | `imported_and_ordinary_build_packages_exclude_test_files` |
| All selected packages build before execution | `every_package_builds_before_any_test_is_executed` |
| No selected tests still check sources and need no native tools | `no_tests_checks_packages_without_a_native_toolchain` |
| Test-only main package and validation of an existing application main | `a_test_only_main_package_works_but_an_invalid_application_main_is_rejected` |
| Other nonzero exits fail and continue | `explicit_nonzero_exit_fails_and_later_tests_continue` |
| Signal termination fails and continues | `a_signal_terminated_test_fails_and_later_tests_continue` |
| Temporary executable cleanup on success/child failure/build failure | `temporary_build_files_are_removed_after_success_and_failure` |
| Deterministic package traversal; skipped generated/hidden folders and symlinks | `recursive_targets_run_in_path_order_and_skip_symlinks_and_generated_folders` |

Command help, argument validation and delimiters also use `tests/driver/cli.rs`.
Linux/macOS are the native CI hosts. No new assertion syntax, runtime panic
recovery, external registry, implicit timeout, forced task cancellation, or
Windows support is claimed.
