# JSON conformance

The locked contract is language specification §37.7. All JSON operations use
memory; HTTP owns request-size and I/O limits.

| Contract | Executable coverage |
| --- | --- |
| RFC 8259 whitespace, literals, containers, number grammar, trailing data | `json_tokens_validate_rfc_syntax_ranges_and_sticky_errors` |
| UTF-8, control escapes, escaped NUL, surrogate pairs and lone surrogates | Token tests and REST request validation |
| Signed/unsigned integer widths, uint64 maximum, float32 round-trip text, overflow and negative zero | `json_generated_collections_recursive_types_and_numeric_widths` |
| Sticky errors, byte line/column positions, HTML/JavaScript escaping, indentation | Token syntax tests |
| Token state, Peek/IsNull, duplicate keys, depth configuration, zero Encoder/Value | `json_token_call_order_zero_values_and_peek_are_checked` |
| Number spelling, document order, replacement in place, independent lookups, iterative parse/skip/encode at depth 600 | `json_tree_preserves_numbers_order_clones_and_depth_limits` |
| Declaration order, private fields, acronyms, all naming modes, missing ordinary/optional fields, null, unknown fields | `json_generated_struct_codecs_support_naming_optional_strict_and_custom_types` |
| Generic callers, generic custom methods, interface codecs, optional array elements | Struct and interface codec tests |
| Recursive structs, slices, fixed arrays, sorted map keys, fresh values, duplicate map members, nested strict errors | Collection and interface codec tests |
| Partially decoded owned arrays/maps clean up after error | Native tests run with `ZORE_CHECK_LEAKS=1` |
| Unsupported types and paths, bad custom signatures, naming collisions, preserved ordinary generic restrictions | JSON package-checking tests |
| Borrowed Decoder cannot escape local input, channel, mutex, or task | `json_decoder_preserves_input_provenance` and codec validation tests |
| Parsed Value may outlive its input | Package acceptance test |
| User structs, Optional, strict requests, Value patches, pretty output | `examples/json` with exact expected output |
| REST response serialization and single-name request validation | REST loopback example and model validation test |

Current implementation costs are explicit: Value lookups clone their backing
node storage, and map encoding scans the entries for each sorted emitted key
until the language has a borrowed map-entry API. These affect performance,
not output order or ownership. General struct recursion still uses ordinary
compiled calls; the decoder's depth limit bounds recursive JSON input.
