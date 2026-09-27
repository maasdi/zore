# Integer-literal conformance cases

Authority: spec §3.11–3.12, §3.14, §3.7, and §10.2. These are pending lexer/literal-decoding
cases, not executable or passing tests. Expected values below are mathematical
values; they do not settle literal typing or target integer width.

| Source / scenario | Expected result |
| --- | --- |
| `0`, `00`, `000` | Zero |
| `42`, `042`, `0042` | Decimal 42 |
| `0755` | Decimal 755, not octal |
| `08`, `09` | Decimal 8 and 9 |
| `0b101010`, `0B101010` | Decimal 42 |
| `0o755`, `0O755` | Decimal 493 |
| `0xFF`, `0Xff`, `0xFf` | Decimal 255 |
| `0b0`, `0B0`, `0o0`, `0O0`, `0x0`, `0X0` | Zero |
| `0b0010`, `0o007`, `0x00A` | Leading zeros after prefix allowed; values 2, 7, 10 |
| `0b`, `0B`, `0o`, `0O`, `0x`, `0X` | Reject missing digits |
| `0b2`, `0b102` | Reject invalid binary digits; do not silently accept a valid prefix |
| `0o8`, `0o79` | Reject invalid octal digits |
| `0xG` | Reject invalid hexadecimal digit |
| Non-ASCII digits used as an integer spelling | Reject; digits are ASCII only |
| Complete integer literal followed by newline/EOF | Semicolon insertion per §3.7 |

## Digit separator cases

| Source / scenario | Expected result |
| --- | --- |
| `1_000`, `10_00`, `1_0_0_0` | Decimal 1000; no required group size |
| `0xFF_FF`, `0Xff_ff` | Decimal 65535 |
| `0b1010_0101`, `0B1010_0101` | Decimal 165 |
| `0o7_55`, `0O7_55` | Decimal 493 |
| `0_755` | Decimal 755, not octal |
| `0_0`, `0b0_0`, `0o0_0`, `0x0_0` | Zero |
| `1000_`, `0b10_`, `0o75_`, `0xFF_` | Reject trailing separator |
| `1__000`, `0b1__0`, `0o7__5`, `0xF__F` | Reject consecutive separators |
| `0b_10`, `0B_10`, `0o_755`, `0O_755`, `0x_FF`, `0X_FF` | Reject separator immediately after prefix |
| `0_b10`, `0_o755`, `0_xFF` | Reject split prefixes; underscores cannot split a base prefix |
| `0b1_2`, `0o7_8`, `0xF_G` | Reject invalid base digits; underscores do not make them valid |
| `_1000` | Identifier, not an integer literal |

## Suffix rejection cases

| Source / scenario | Expected result |
| --- | --- |
| `42u8`, `42int64`, `42i64`, `42f32`, `42n`, `42_name` | Reject attached suffix/identifier continuation |
| `0b10u8`, `0o755int64`, `0xFFu8` | Reject suffixes on every prefixed base |
| `1_000u64` | Valid separator does not permit a suffix |
| `0xFF`, `0xF32`, `0xdeadBEEF` | Valid hexadecimal integers, not suffix forms |

Assert source spans and useful diagnostics for invalid bases/digits/separators/suffixes.
Float cases are in `floats.md`; add typed-range cases when those rules are locked. At
semantic milestones, verify that base spelling does not change Copy semantics.
