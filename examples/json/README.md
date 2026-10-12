# JSON

Run `zore run examples/json/main.ore` from the repository root.

The example encodes exported struct fields with lower-camel names, decodes a
request with strict field checking, and applies a JSON tree patch that separates
missing fields from explicit null. Optional email fields disappear when unset.
It also demonstrates indented snake-case output and rejection of a misspelled
request field. The private password field is never encoded.
