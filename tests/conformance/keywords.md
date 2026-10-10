# Keyword reservation conformance requirements

Authority: spec §3.15–3.18. Policy, MVP keywords, and future-reserved words are
locked. These are pending requirements and cases, not executable
tests. Implement lexical/grammar cases with the lexer and parser milestones,
and predeclared-name protection cases with resolution.

- Reject every MVP keyword when used as an identifier in a name position.
- Reject every future-reserved word in the same positions, even if no associated
  feature exists. Exercise local, parameter, field, and type names.
- Reject unavailable future syntax; reserving a word must not activate a feature.
- Match keyword spelling exactly, without case folding. Case variants that are
  not themselves reserved follow the ordinary identifier rules.
- Do not reject ordinary names merely because another language reserves them.
- Treat keyword spellings inside strings and comments as contents, not keywords.
- Test keyword boundaries: an identifier containing a reserved word as a prefix
  or substring must remain an identifier unless its complete spelling is reserved.
- Test semicolon-insertion eligibility according to the finalized keyword rules.

Future-feature diagnostics should explain that the word is reserved and the
feature is unavailable. Keep these separate from tests for supported keyword use.

## Future-reserved word cases

For each of `trait`, `impl`, `enum`, `match`, `unsafe`, `macro`, and `defer`,
test rejection in package, type, function, receiver/parameter, local, and
field name positions. Assert that the diagnostic identifies the reserved word.

| Input / scenario | Expected result |
| --- | --- |
| `let match = 1` | Reject future-reserved name |
| `let matchValue = 1` | Valid with respect to reservation |
| `Trait`, `TRAIT`, `_unsafe`, `deferred`, `myenum` | Ordinary identifier spellings |
| `"trait match unsafe"` | String contents, not reserved-word tokens |
| `// trait impl enum macro defer` | Comment contents, not reserved-word tokens |
| Each future-reserved word followed by newline/EOF | No semicolon inserted after the word; source use remains unsupported |
| `unsafe {}` or `defer cleanup()` | Reject unavailable syntax; do not enable features |

Do not use this list to reserve other possible feature names.

## MVP keyword cases

For each word in the exact §3.17 table, test its keyword token and rejection in
name positions. `break`/`continue` grammar and `nil` typing remain separate work.

| Input / scenario | Expected result |
| --- | --- |
| `let func = 1` | Reject keyword as local name |
| `own` used as a field name | Reject keyword as field name |
| `functionName`, `Func`, `True`, `_return` | Ordinary identifiers |
| `int`, `string`, `Array`, `Task`, `error`, `println`, `clone`, `drop` | Identifier tokens; predeclared semantic names, not keywords |
| Every primitive type name from §6.1 | Identifier token |
| `"return true nil"` or `// break continue` | Contents, not keyword tokens |
| `break`, `continue`, `return`, `true`, `false`, `nil` followed by newline/EOF | Insert semicolon |
| Any other keyword from §3.17 followed by newline/EOF | Do not insert semicolon after keyword |

## Predeclared-name protection cases

For every primitive type name in §6.1 and each of `Array`, `Task`, `error`,
`println`, `clone`, and `drop`, verify rejection of declarations introducing that
name into unqualified lookup at package, function, and nested/closure scope.
Exercise variable, constant, type, function, parameter, and receiver binding names
as their grammar becomes available. Test import-introduced names when package
resolution is implemented.

| Input / scenario | Expected result |
| --- | --- |
| `let println = 42`, `var string = 1`, `const int = 1` | Reject predeclared-name shadowing |
| `func f(drop int) {}` | Reject parameter shadowing |
| `func (clone User) f() {}` | Reject receiver binding shadowing |
| `Println` as a binding name | Not rejected for shadowing `println` |
| Field named `string`, selected as `record.string` | Not rejected by predeclared-name protection |
| Method named `clone` | Not rejected solely for matching the predeclared name |
| User-defined `drop` method from §14.3 | Not rejected as shadowing; enforce its cleanup contract |
| Unqualified `drop` after declaring a member named `drop` | Still resolves to the predeclared operation |

Assert source-aware resolution diagnostics, not keyword errors. These cases do
not permit keywords as field names or settle shadowing of ordinary user names.
