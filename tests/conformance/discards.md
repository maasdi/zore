# Discard-target conformance cases

Authority: spec §5.5, §10, §11, §14, and §18.6. These are pending parser,
resolution, ownership, and runtime cases, not executable coverage. Calls and
resource names below are conceptual fixtures to define when those stages exist.
Unless stated otherwise, results are non-error values. Error-discard cases are
specified in `errors.md` under the policy in §15.6.

| Input / scenario | Expected result |
| --- | --- |
| `let value, _ = pair()` | Bind first result; discard second; call once |
| `let _ = calculate()` and `var _ = calculate()` | Evaluate once, discard result, create no binding |
| `_ = calculate()` | Discard assignment needs no prior `_` declaration |
| `let _, _ = pair()` | Repeated discard allowed; consume both result positions |
| Two discard targets applied to a one-result call | Reject result-count mismatch |
| `println(_)`, `let x = _` | Reject reading `_` |
| `_` as function/type/field/parameter/receiver name | Reject invalid name context |
| `_unused` as a local name | Ordinary binding, not discard |
| Copy local assigned to `_`, then read again | Valid; source remains available |
| Move local assigned to `_`, then read again | Reject use after move |
| Move local assigned to `_` while borrowed | Reject move while borrowed |
| Owned result assigned to `_` | Normal deterministic cleanup exactly once |
| Owned result discarded alongside a retained result | Discarded value cleaned up; retained result remains owned normally |
| Borrowed view assigned to `_` | Do not destroy the backing owner; enforce ordinary borrow rules |
| Discard an effectful call's result | Preserve call side effects |
| Discard result of `?` or `await` expression | Preserve explicit propagation/suspension semantics |
| Discard a Task handle | Detach handle; do not cancel running task |

Use non-Copy resource fixtures to observe ownership transfer and cleanup. Do not
invent special drop ordering or bypass lifetime checks to satisfy discard cases.
Use `errors.md` for explicit versus silently ignored error results.
