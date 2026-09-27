# Program entry-point conformance cases

Authority: spec §3.19, with §15.2, §15.4, §15.6, §17.8, §18.9–18.11. These are
pending resolution, code-generation, and runtime cases, not passing coverage.

| Source / scenario | Expected result |
| --- | --- |
| `package main` with `func main() {}` | Valid program entry point |
| `package main` with no `main` function | Reject when checked/built as a program; diagnose at the package clause |
| `func main(args Array<string>) {}` or `func main(x int) {}` | Reject: entry point takes no parameters |
| `func main() int { return 0 }` or `func main() error { return nil }` | Reject: entry point returns no results |
| `async func main() {}` | Reject: entry point is not `async` |
| Two package-level `func main()` declarations | Reject as duplicate functions (§7.8) |
| Method `func (s Server) main() {}` in `package main` without a function `main` | Still missing the entry point; the method does not satisfy it |
| Method named `main` alongside a valid `func main()` | Valid; no conflict |
| `func main()` in a package not named `main` | Ordinary function; no entry-point diagnostics |
| `main` using `?` on an error-returning call | Reject: `main` has no error result (§15.2) |
| `main` ignoring an error result | Reject per §15.6; `_ = save()` is valid |
| `await` directly in `main` | Reject: `main` is not `async` (§17.8) |
| `main` spawns with `go` and retrieves with `.wait()` | Valid (§18.9) |
| `main` returns normally | Locals dropped in order, then exit status 0 |
| Initial task panics | Unwind with drops, panic report on stderr, nonzero exit status |
| Drop panics while the initial task is unwinding | Abort with nonzero status |
| Detached task still running when `main` returns | Process exits; task abandoned (§18.11) |

Native cases need bounded execution and must compare stdout, stderr, and exit
status. The exact nonzero status is implementation-defined; assert only that it
is nonzero.
