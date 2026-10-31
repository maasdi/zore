# `std/encoding/hex`, `std/encoding/base64`, and `std/encoding/binary`

These cases cover spec §37.14. Native cases are in `tests/codegen/encoding.rs`;
checking cases are in `tests/packages/packages.rs`; `examples/encoding` packs
a session into a URL-safe token and reads it back.

## `std/encoding/hex`

| Scenario | Expected result |
| --- | --- |
| All 256 byte values | 512 lower-case digits from `000102` to `fdfeff`; they decode back, also in upper case |
| `DeAdBeEf` | Mixed case decodes to `DE AD BE EF` |
| Empty input | Encodes to `""`; decodes to an empty array without error |
| `abc`, and `zzz` | `hex.DecodeString: odd length`, and `hex.Decode: odd length`: length is checked before bytes |
| `0x00`, `g0`, `0g`, `00 1`, `12\n4`, `-1`, `ab+c` | `invalid byte at offset N` naming the first bad byte, and an empty array |
| `EncodedLen(-1)`, `DecodedLen(-1)`, `EncodedLen` past the largest `int` | Panics naming the function |

## `std/encoding/base64`

| Scenario | Expected result |
| --- | --- |
| RFC 4648 §10 vectors `""` through `foobar` | The listed text in `Standard` and `URL`, without `=` in the raw encodings, and back |
| Bytes `FB EF FF` | `++//` in `Standard`, `--__` in `URL` |
| Every length from 0 to 70 in all four encodings | Output length equals `EncodedLen`; `DecodedLen` is at least the input length and less than 3 more; round trip |
| `Zg=`, `Zm9vY` and `Zg` padded, `Zm9vY` and `Z` raw, white space that changes the length | `invalid length` |
| The other alphabet's characters, a space, `\r\n`, `Z===`, `Zm=v`, `Z=m=`, `====`, `Zg==Zg==`, `=` in a raw encoding | `invalid byte at offset N` at the first rejected byte |
| `Zh==`, `Zm9=`, raw `Zh` and `Zm9vYmF` | `nonzero padding bits at offset N` at the last character |
| `Decode` with zero bytes | The message starts `base64.Encoding.Decode:` |
| `Encoding(4)`, `Encoding(-1)` | Panics with `base64: unknown Encoding N` |
| Negative lengths, and `EncodedLen` past the largest `int` | Panics; the largest raw length that fits, `9223372036854775807`, does not |

## `std/encoding/binary`

| Scenario | Expected result |
| --- | --- |
| Bytes `01`–`08` read at every width | Big endian `258`, `16909060`, `72623859790382856`; little endian `513`, `67305985`, `578437695752307201` |
| All bits set | Unsigned maxima, and `-1` for every signed width |
| Minimum and maximum of `int16`, `int32`, `int64` appended and read back | The exact two's complement bytes, and the same values |
| `Append` and `Put` at every width in both orders | The exact bytes; `Put` writes only its slice |
| Slices one byte short, one byte long, and empty | `binary.ByteOrder.Name: need W bytes, got N`, a zero value, and the data unchanged |
| `ByteOrder(2)` and `ByteOrder(-1)` | Panics with `binary: unknown ByteOrder N` |

## Together

| Scenario | Expected result |
| --- | --- |
| A function that decodes base64, then hex, then a big-endian `Uint32` with `?` | `256` for a good token; the first decoder's error otherwise, with no storage left behind |
| A string passed as bytes, an unread decode error, `base64.URL` as an `int`, a `base64.Encoding` as a `binary.ByteOrder`, an unexported name, `Append` to a `let` array, an extra argument | Reject: mismatched types, may be unused, not exported, mutable, takes 1 argument |
