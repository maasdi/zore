# `std/flag`

These cases cover spec §37.21. Native cases are in `tests/codegen/flag.rs`;
checking cases are in `tests/packages/packages.rs`; `examples/cli` is a small
word-counting program built on the package.

## Reading options and arguments

| Scenario | Expected result |
| --- | --- |
| No options given | Declared `String` and `Int` defaults, `Bool` false, empty `Strings` and `Rest`; `Has` is false for every option |
| `--name bob --count=3 --shout --tag a --tag=b plain x y` | Each value set, tags `a,b` in order, `Rest` is `x,y`, `Has` true |
| `-n al -c=2 -s plain` | Short forms with a separate value and with `=` |
| `x -n al y --count 1 z` | Options between arguments; `Has("count")` is true even though `1` is the default |
| `--name -s` and `--count -5` | The next word is the value, even when it starts with `-` |
| `- -- --help -n --tag=x` after the required argument | `-` is an argument, and every word after `--` is an argument, `--help` included |
| Empty words and `--name=` | Empty words are arguments; `--name=` sets an empty value that counts as given |
| `--name=a=b` and `-c=+7` | The value is everything after the first `=`; `Int` reads like `strconv.Atoi` |
| A set with no arguments given no words | Parses with defaults |

## Rejected input

| Scenario | Expected result |
| --- | --- |
| `--nmae`, `-x`, `-sn`, `-n5`, `--bogus` | `flag: unknown option` with the word as typed |
| `--pasword=hunter2`, `-p=hunter2` | `flag: unknown option --pasword` and `-p`: the value is never repeated |
| `--shout=yes`, `-s=` | `flag: option --shout takes no value` |
| `--name` or `-c` as the last word | `flag: option --name needs a value`, named by its long form |
| `--count x`, `-c=1.5`, a value past the largest `int` | `flag: option --count: invalid integer "x"` |
| `--name a -n b`, `-s --shout`, `-c 1 -c 1` | `flag: option --name given more than once` |
| No words, or only options | `flag: missing argument STYLE`; with two required arguments, the first missing one, `TO_PATH` for `to-path` |
| Four words for two required arguments | `flag: unexpected argument "c"`, the first extra one |
| `--bogus --help` | The unknown option: problems are found from left to right |
| `--help --bogus`, `plain -h` | `Help`, equal to `flag.Help` with `==` and `errors.Is` |
| `-h=x`, `--help=` | `flag: option --help takes no value` |
| `a b c --bogus` against two required arguments | The unknown option: options are read before counts are checked |
| Every rejected case | The returned `Result` answers with defaults, empty lists, and empty argument text |

## Usage text and reporting

| Scenario | Expected result |
| --- | --- |
| A set with a summary, every option kind, `Arg` and `Rest` | Exact text: usage line, summary, `Arguments:`, `Options:` aligned two spaces past the widest entry, defaults and `(repeatable)` notes, `--help` last |
| A set with no summary or arguments, empty help texts, zero defaults | No summary or `Arguments:` section, no trailing spaces, no default notes |
| A set with only `Rest` | `[ITEM...]` on the usage line |
| A built program run with `--help --bogus` | Usage on stdout, nothing on stderr, exit status 0 |
| A built program run with `--name=secret-value --nmae=secret-value` | Three lines on stderr, nothing on stdout, exit status 2, the value not repeated |
| `Report(nil, ...)` | 0, nothing written |
| `Report` with `Help` wrapped by `errors.Wrap` | Usage on stdout, 0 |
| `Report` with a failing writer | 1 |
| `Report` with an error from elsewhere | `t: other failure`, the usage line, and the hint; status 2 |

## Mistakes in the program

| Scenario | Expected result |
| --- | --- |
| Declaring `help` or `h` | Panics with `flag: reserved option --help` or `-h` |
| Long names `x`, `9lives`, `dry run`, `--name` | Panics with `flag: invalid option name` |
| Short names `qq`, `-` | Panics with `flag: invalid short name` |
| A long or short name used twice | Panics with `flag: duplicate option` |
| Argument names `""`, `1st`, a repeated name, any argument after `Rest` | Panics with the listed message |
| Asking a `Result` for a missing name or the wrong kind | Panics, such as `flag: no String option --count` and `flag: no Rest argument` |
| Declaring on a `let` set, using a moved set, ignoring `Parse`, wrong value types, private fields, writers that are not `mut` | Rejected by the checker |

## Ownership and tasks

| Scenario | Expected result |
| --- | --- |
| Parsing inside an `async` task and in a plain helper that uses `?` | The same results; the error propagates; no leaks under `ZORE_CHECK_LEAKS` |
