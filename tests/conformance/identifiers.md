# Identifier conformance cases

Authority: spec §3.5 and §4.1. These cases are specified expectations, not
executable tests or passing coverage. Implement them with M2 (lexical rules),
M3–M4 (declarations), and M9 (resolution/visibility).

| Input / scenario | Expected result |
| --- | --- |
| `user`, `user2`, `user_name`, `User`, `_internal`, `_User`, `a0`, `Z9`, `__` | Valid identifier spellings |
| `2user` used as a declaration name | Reject: a name cannot start with a digit |
| `café`, `用户`, `α`, `user١` used as names | Reject: non-ASCII identifier characters |
| `user` and `User` declared in the same scope | Distinct names, not a duplicate due to case folding |
| `User` as a package declaration name; `Name` as a field name | Exported |
| `user`, `_internal`, `_User` as package declaration/field names | Package-private |
| `User` as a local binding name | Does not create a package export |
| `_` used as a name or read as a value | Reject; binding/assignment discard targets are allowed by §5.5 and create no name |
| `func` used as a declaration name | Reject: keyword cannot be an identifier |
| Unicode text inside a supported string or comment form | Do not reject because of the ASCII identifier restriction |

For invalid names, assert a source-aware diagnostic at the offending name or
character. Lexer recovery/token splitting for invalid input remains an internal
choice; it must not cause an invalid declaration to be silently accepted.
