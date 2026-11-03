# Secure random bytes and constant-time comparison

The contract is locked in specification §37.19. Executable tests are in
`tests/codegen/crypto.rs`, with runtime unit tests in `runtime/src/crypto.rs`
and compiler acceptance and rejection in
`tests/packages/packages.rs::crypto_rand_and_subtle_check_their_calls`.

| Scenario | Expected result |
| --- | --- |
| `rand.Bytes` of 1, 7, 255, 256, 4097, and `MaxBytes` | That many bytes and `nil` |
| `rand.Bytes(0)` | An empty array and `nil` |
| `rand.Bytes` of -1, `MaxBytes + 1`, and the smallest integer | `crypto/rand.Bytes: size outside 0..1048576` and an empty array |
| 65,536 random bytes | Every byte value appears, none more than 511 times |
| Two 32-byte requests | Different bytes |
| 2,000 `rand.Text` tokens | Each 43 characters from `A`–`Z`, `a`–`z`, `0`–`9`, `-`, `_`; all different |
| 32 async tasks calling `Text` and `Bytes` | All succeed with different tokens |
| `ConstantTimeEqual` of two empty slices; a slice with itself; with a copy | `true` |
| A one-bit change at every position; a shorter slice; empty against nonempty; `{0, 255, 128}` against `{0, 255, 127}` | `false` |
| `rand.Bytes(16)` unused; `let token = rand.Text()`; `rand.read`; a string size; arrays instead of slices | Rejected |

A failing system random source is not injected in these tests; the runtime
returns its error unchanged, as the specification states.
