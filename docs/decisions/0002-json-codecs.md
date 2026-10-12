# 0002 — JSON codecs generated from concrete types

Status: accepted; language contract locked in specification §37.7.

Issue #106 requires JSON for structs without reflection, annotations, or macros.
The token Encoder/Decoder and owned Value tree are Zore code. Marshal and
Unmarshal use private generic specialization hooks in the bundled JSON package.
After generic arguments become concrete, the compiler validates each reachable
codec type and replaces those hooks with ordinary typed HIR functions. Existing
generic caching handles recursive calls; ownership checking, MIR lowering, drop
insertion, and LLVM generation process these bodies normally. No new runtime
JSON functions or LLVM-dependent semantic checks are required.

Only the bundled JSON encode/decode hooks may declare a generic function without
a source body. Only the six JSON entry points and those hooks use JSON-specific
type-argument validation. Ordinary generic restrictions remain unchanged.
Borrowed codec interfaces dispatch through existing method tables. Custom
methods take precedence over generated field traversal. Generated fresh values
cannot instantiate a codec interface without a concrete implementation; the
compiler rejects those paths unless a custom decoder handles construction.
Compound targets containing existing codec interfaces remain subject to the
ordinary conservative borrow rules for indirect calls; root interface decoding
is covered separately.

Naming values may be computed at runtime. The compiler checks all three naming
policies for collisions and emits precomputed keys for every generated struct
codec; a collision is a compile error even if a particular call uses a different policy. This keeps
collision checking independent of call-site constant propagation and generic
wrappers. Missing optional fields clear Set before decoding each struct object;
missing ordinary fields retain their existing values. Unset optionals outside
struct fields encode null.

Value uses flat node storage and explicit work stacks, so deep trees do not need
recursive destruction or token traversal. Numbers retain their original text.
Get and Index clone node storage; a borrowed collection API can reduce this cost
later. Map encoding sorts emitted key strings and scans borrowed entries for
each key because the language does not yet expose borrowed map lookup for Move
values. This avoids cloning values or invoking a user's clone method.

Conformance covers syntax, UTF-8, ranges, nesting, ordering, naming, optional and
custom codecs, recursive and generic types, ownership, and error cleanup. The
console JSON example and HTTP REST example exercise both standalone and server
use. The issue is delivered in one PR as requested by the maintainer.
