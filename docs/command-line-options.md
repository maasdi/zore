# Command-line options

`std/flag` reads the options and arguments a program is started with. You
describe them once, hand the words to `Parse`, and get back a value you can ask
questions of. Parsing never prints and never ends the program; you decide what
happens next. The full rules are in the
[language specification, §37.21](../spec/language-spec.md#3721-command-line-options--locked).

## A small program

```ore
package main

import "std/flag"
import "std/os"

func main() {
    var set = flag.New("greet", "Print a greeting.")
    set.String("name", "n", "world", "who to greet")
    set.Int("count", "c", 1, "how many times")
    set.Bool("shout", "s", "use capital letters")
    set.Rest("file", "extra files")

    let args = os.Args()
    let result, err = set.Parse(args[1:])
    if err != nil {
        var stdout = os.Stdout()
        var stderr = os.Stderr()
        os.Exit(set.Report(err, stdout, stderr))
    }
    for var i = 0; i < result.Int("count"); i += 1 {
        println("hello " + result.String("name"))
    }
}
```

`greet --help` prints:

```text
Usage: greet [options] [FILE...]

Print a greeting.

Arguments:
  FILE...  extra files

Options:
  -n, --name NAME    who to greet (default "world")
  -c, --count COUNT  how many times (default 1)
  -s, --shout        use capital letters
  -h, --help         show this help
```

and `greet --nmae bob` prints this to standard error and exits with status 2:

```text
greet: unknown option --nmae
Usage: greet [options] [FILE...]
Run 'greet --help' for more information.
```

## What you can declare

| Call | Typed as | You get back |
| --- | --- | --- |
| `Bool("verbose", "v", help)` | `--verbose` or `-v` | `true` if given |
| `String("name", "n", "world", help)` | `--name bob`, `--name=bob`, `-n bob`, `-n=bob` | The value, or `"world"` |
| `Int("count", "c", 1, help)` | Same forms, a whole number | The number, or `1` |
| `Strings("tag", "t", help)` | Given any number of times | Every value, in order |
| `Arg("file", help)` | A required word | That word |
| `Rest("file", help)` | Any number of words after the required ones | All of them |

Pass `""` as the short name when an option has none. `--help` and `-h` are
always there for you. An option and an argument can't share a name, such as
`--file` and `FILE`, so the help text stays clear.

## Rules worth knowing

- Options and arguments can be mixed: `greet a -n bob b` works.
- A value can't be a separate word starting with `-`, so a forgotten value is
  caught: `--name --shout` says `option --name needs a value`. Number options
  still accept negative numbers (`--count -5`), and `-` alone is a value. For
  any other value starting with `-`, use `=`: `--name=-x`.
- `--help` or `-h` before `--` always shows the help, even if something else
  on the line is wrong.
- `--` stops option reading. Use it for arguments that start with `-`:
  `greet -- -weird-file`.
- Giving `--name` twice is an error; giving a `Strings` option twice adds both.
- Short options cannot be grouped: write `-a -b`, not `-ab`.
- `result.Has("count")` tells you whether `--count` was typed, even when its
  value equals the default.
- If `Parse` fails, the result it returns still answers with the defaults, so
  nothing is ever missing or null.

## Choosing your own output

`Report` is a helper, not a rule. It prints the usage to standard output for
`--help` (status 0), or a short message to standard error for a mistake
(status 2), and returns that status without exiting. To print something else,
skip it:

```ore
if err == flag.Help {
    println(set.Usage())
    return
}
if err != nil {
    println("bad input: " + errors.Message(err))
    os.Exit(64)
}
```

`Report` takes any two `io.Writer` values, so a test can capture what it would
print. [`examples/cli`](../examples/cli) is a complete program built this way.

## Not included

Grouped short options (`-abc`), `--no-` booleans, abbreviations, subcommands,
other value types, environment variables, configuration files and shell-style
splitting of a command string are left out on purpose. Each would need its own
agreed design first.
