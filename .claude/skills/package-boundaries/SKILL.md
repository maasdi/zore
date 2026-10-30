---
name: package-boundaries
description: Decide which standard package owns a function, type, or rule before adding or moving library code. Use whenever you add a public name to std/, change what a std package imports, or review such a change, including work delegated to other agents.
---

# Package boundaries

Every public name in `std/` belongs to the one package whose job it is. Check
these rules before adding a name, moving one, or adding an import.

1. Put a name where its meaning lives, not where it is first needed.
   - Calendar math, durations, deadlines, and RFC 3339 belong to `std/time`.
   - Protocol syntax and limits belong to the protocol package: HTTP dates
     and headers to `std/net/http`, URL rules to `std/net/url`.
   - Text formatting belongs to `std/fmt` or `std/strconv`, byte and string
     helpers to `std/strings` and `std/bytes`.
   - A consumer such as `std/log` uses those packages; it does not grow its
     own copy of their rules.
2. Imports point downward only. Low-level packages (`strconv`, `strings`,
   `unicode`, `math`, `errors`, `time`, `io`, `sort`) never import higher ones
   (`os`, `net`, `context`, `log`, `encoding/json`, `net/http`). Adding an
   import must not create a cycle or make a low-level package depend on a
   higher one; when it would, move the shared code down instead.
3. Do not forward. A package does not re-export or wrap another package's
   function under its own name just to keep an old spelling. Move the
   callers and remove the old name, and record the change in the
   specification's migration notes and `CHANGELOG.md`.
4. Share primitives, not policy. When two packages need the same building
   block, put the primitive in the lower package and keep each package's
   own policy (formats, limits, error text) at home.
5. Keep internals private. Helpers needed by only one package stay
   unexported there. Expose a new public primitive only when the specification
   locks it.

Before finishing:

1. List the public names the change adds, moves, or removes, and the package
   each one lives in. Check each against rule 1.
2. Compare the `import` lines before and after. Check each new import
   against rule 2.
3. Search for a second copy of the logic you added in other packages. Remove
   it or note why it is different policy.
