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
| `rand.Bytes(16)` unused; `let token = rand.Text()`; `rand.secureRead`; a string size; arrays instead of slices | Rejected |

Runtime unit tests also fill a 1 MiB request to its last chunk and, on Linux,
inject a failing source: the call returns
`crypto/rand.Bytes: injected failure` and no bytes, and the next call works.

## Hashes and HMAC

The contract is locked in specification §37.20. Executable tests are in
`tests/codegen/crypto.rs`, with compiler acceptance and rejection in
`tests/packages/packages.rs::crypto_hashes_and_hmac_check_their_calls`.

| Scenario | Expected result |
| --- | --- |
| SHA-1, SHA-256, and SHA-512 of the empty input, `abc`, the 448- and 896-bit NIST messages, and one million `a` bytes | The FIPS 180-4 example values |
| Every input length from 0 to 300 bytes, written whole, byte by byte, and in two parts | The same hash each way |
| `Sum` after the first part, then writing the rest | The hash of the first part, then the hash of the whole |
| `Reset`, then writing the input again | The same hash as a new `Digest` |
| `Write` | Returns the input length and `nil` |
| `Size` and `BlockSize` | 20/64 for SHA-1, 32/64 for SHA-256, 64/128 for SHA-512 |
| HMAC-SHA-256 and HMAC-SHA-512 of RFC 4231 cases 1, 2, 3, 4, 6, and 7 | The RFC values |
| HMAC with an empty key and data; keys of exactly the block size and one byte longer | The RFC 2104 result |
| Two HMACs compared with `subtle.ConstantTimeEqual` | `true` for the same key, `false` for another key |
| A 360,000-byte file hashed through `io.Copy` into a `Digest` | The same hash as `Sum256` of its bytes |
| A `Digest` built with a struct literal; reading its fields; `sha256.Sum256` given an array; `hmac.SHA256` with one argument; an unused result; a call to a private helper | Rejected |
