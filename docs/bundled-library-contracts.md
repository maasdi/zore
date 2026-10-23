# Bundled library compiler contracts

The compiler and bundled `std/encoding/json` and `std/net/http` sources ship
as a unit. The interfaces below are private implementation seams, not a new
extension mechanism for application packages. Public behavior remains governed
by the language specification. Change both sides together and run the paired
contract and native tests when changing a seam.

## JSON codec synthesis

`hir/lower/json.rs` recognizes the six public generic Marshal/Unmarshal entry
points and private `encode`/`decode` hooks only in `std/encoding/json`. A hook
specialization must have three parameters, in this order:

| Hook | Parameters | Results |
| --- | --- | --- |
| `encode<T>` | borrowed T, mutable Encoder, borrowed Naming | none |
| `decode<T>` | mutable T, mutable Decoder, borrowed Naming | error |

Both hooks are synchronous and reusable. Encoder, Decoder, Value, Naming, and
Optional are identified through resolved package/type identities. Optional
must contain `Value T` then `Set bool`, since generated field access uses that
order. Naming is an integer type; the public AsWritten/LowerCamel/SnakeCase
values remain 0/1/2, as specified and covered by behavioral tests.

`json/contract.rs` resolves every helper used by the selected encode/decode
builder before synthesis. It checks exact parameter IDs/types, parameter modes,
result types, synchronous execution, and absence of remaining generic parameters:

- Both directions use Begin/EndObject, Begin/EndArray, Null, Bool, String, Value.
  Receivers are mutable. Encoder value arguments borrow and return nothing;
  Decoder operations return the value followed by error, or just error.
- Encoding additionally uses Encoder.Field, Int, unsigned, floatWidth, plus
  unsignedText, sortedNames, and `std/strconv.Itoa`. sortedNames owns and returns
  `Array<string>`; the other nonreceiver arguments borrow.
- Decoding additionally uses Decoder.IsNull/More (bool without error), Key,
  unknown, fail, signed/unsigned/floatWidth and signedKey/unsignedKey, plus
  fieldIndex. Numeric readers take an integer width; key readers also take a
  string. fieldIndex borrows a string and `[]string`, returning int.

Helper IDs are retained by the builder instead of rediscovered by name during
each generated call. A missing or incompatible helper reports a bundled JSON
codec contract diagnostic at the specialization call. Unsupported application
types keep their existing codec diagnostics. Generated HIR still passes through
ordinary ownership, MIR, cleanup, and async processing.

## HTTP direct-handler adaptation

`hir/lower/http.rs` activates when the bundled RouterBuilder.Handle hook exists.
It validates the following before modifying any function body:

- Handle and register are synchronous, return no values, and take a mutable
  RouterBuilder receiver, borrowed string pattern, and owned callback/factory.
  The factory type comes from the registration function's third parameter ID,
  independently of local-storage numbering.
- HandlerFactory has exactly one synchronous, shared-receiver NewHandler method,
  no explicit parameters, and results `(Handler, error)`.
- Handler has exactly one synchronous, shared-receiver Handle method, mutable
  ResponseWriter/Request parameters, and an error result. The hook's callback
  signature matches this method exactly.

The adapter retains the existing source checks for known synchronous/reusable
callbacks and shared-read Copy captures without views/functions. It creates
ordinary HIR factory and handler records with the same semantic ID allocation
and invocation order. Contract drift reports a diagnostic at the bundled hook;
it does not panic by indexing a missing method, result, or local.

## Verification and remaining scope

`tests/packages/bundled_contracts.rs` checks current bundled sources and mutates
their AST declarations to exercise missing helper, ownership, hook, and Optional
layout drift through HIR checking. HTTP unit tests additionally mutate interface
method/result lists and move the factory parameter to a different local ID.
Existing package tests retain supported/rejected application behavior, while
native JSON, HTTP, closure, interface, generic, and async tests cover execution
and cleanup. Local raw IR comparison against the base compiler supplements these
tests; full cross-host results belong to CI.

This extraction does not change semantic passes, IDs, spans, ownership rules,
ABI/linkage, initialization, or language features. Region propagation and other
HIR/LLVM responsibilities remain in their existing modules. No file-size quota,
crate split, new dependency, or extraction-driven performance claim is intended.
