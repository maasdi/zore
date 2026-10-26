# Standard string methods

Authority: spec §9.2 and §37.2. These rows map executable assertions to the
locked string-method contract; they do not replace the historical string-literal
or standard-package inventories.

| Contract | Executable coverage |
| --- | --- |
| Import-free chains and ordinary string/array/multiple-result types | `standard_string_methods_work_without_imports_and_preserve_result_types` |
| Nested closures, conditions, loop headers, select arms, task scopes and global initializers discover the provider | `string_method_discovery_covers_nested_headers_closures_and_initializers` |
| Named types retain their methods; literal/index method values are rejected | `string_methods_keep_named_type_identity_and_existing_method_value_rules` |
| Wrong arguments, nil strings, unsupported methods and user primitive declarations are rejected | `string_methods_reject_bad_arguments_nil_and_primitive_extensions` |
| Interface-only use discovers methods; incompatible signatures and conflicting captures are rejected | `string_methods_satisfy_interfaces_and_use_ordinary_task_rules` |
| Every fixed method, Unicode/byte indexing, empty values and package-call compatibility | `standard_string_methods_preserve_unicode_empty_and_package_function_behavior` |
| Receiver evaluated once before arguments; owned results and unobserved task results are cleaned up | `string_methods_evaluate_receivers_once_before_arguments_and_release_owned_results` |
| Method values, interface dispatch, owning task captures, async temporary receivers and canonical provider identity | `string_method_values_interfaces_and_async_receivers_follow_existing_ownership_rules` |
| Existing Repeat panic diagnostic and ordinary cleanup | `string_method_panics_preserve_the_package_diagnostic_and_cleanup` |

Frontend assertions live in `tests/typecheck/string_methods.rs`; execution and
leak-checked exit assertions live in `tests/codegen/string_methods.rs`. Existing
package-function tests remain in place.
