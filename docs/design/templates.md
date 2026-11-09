# Template follow-up decision

Status: deferred, application-driven; no template API, syntax or implementation
is selected. Tracked by [#226](https://github.com/maasdi/zore/issues/226), parent
[#210](https://github.com/maasdi/zore/issues/210), and Q16 in the
[specification questions](../spec-questions.md). The implemented initial
contract is [HTML escaping](../html.md), spec §37.31.

## Decision

Ship the basic HTML escaping primitive independently. Require a concrete
application, its template examples and its data/output requirements before
choosing a template language or rendering API. A web framework is outside this
decision. HTML auto-escaped templates and plain-text templates need separate
contracts; choosing one does not implicitly select the other.

For an HTML template proposal, demonstrate how trusted literal markup and
untrusted values remain distinct through parsing, compilation and rendering.
Escaping must follow the insertion context and its transitions, including
single/double-quoted attributes, URLs, embedded JavaScript/CSS and raw-text
sections. A blanket call to EscapeString cannot implement contextual escaping.
Reject unsupported or ambiguous contexts rather than silently treating them as
ordinary text. Define URL validation and scheme restrictions, embedded-language
serialization, composition and any trusted-markup mechanism before exposing it.
No trusted-markup bypass type is introduced by the current escaping package.

For plain-text templates, decide the expression/data model and formatting rules
independently. Plain-text output has no automatic HTML escaping and must not be
presented as suitable for HTML insertion. The application must establish the
need for either renderer, caching, includes or composition before these become
public features.

## Ownership and rendering gates

A future specification must choose and test the output model explicitly:

- A returned immutable string must be independent of borrowed render inputs and
  remain usable after the renderer/input bindings are dropped.
- A writer-based operation must borrow the writer only for rendering, keep its
  ownership with the caller, and define counts, partial output and explicit
  errors. It must not silently close or flush a caller-owned sink.
- Compiled template storage, captured values, sharing across tasks and borrowing
  across async writes require explicit ownership rules. No view may outlive its
  backing owner; plain/async behavior must be specified separately from CPU cost.
- Input, parse-depth, expansion, iteration and output budgets must be selected
  from application requirements, with cleanup and adversarial tests.

These are design gates for a later spec §53 decision, not promised signatures,
package paths or an implementation-ready template contract. Keep Q16 open until
an application-driven proposal resolves them.
