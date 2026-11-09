# HTML escaping

`std/encoding/html.EscapeString(text)` escapes data for ordinary HTML element
text and fixed single- or double-quoted values of ordinary text attributes, such
as `title` and `alt`. Markup and attribute names stay application-controlled.

```ore
package main

import "std/encoding/html"

func greeting(name string) string {
    return "<p title=\"" + html.EscapeString(name) + "\">Hello " + html.EscapeString(name) + "</p>"
}

func main() {
    println(greeting("Ada & <friends>"))
}
```

The replacements are `&` → `&amp;`, `<` → `&lt;`, `>` → `&gt;`,
`"` → `&quot;`, and `'` → `&#39;`. All other bytes stay unchanged, including
Unicode, NUL, whitespace and controls. Empty input stays empty. Existing entity
spellings are data: `&amp;` becomes `&amp;amp;`. Escape the original input once
where it enters markup; escaping an escaped result again adds another layer.

Strings are already valid UTF-8. For raw bytes, call `strings.FromBytes(data)`
and handle its validation error first. This package has no bytes overload,
normalization or invalid-UTF-8 replacement behavior. HTML parsers may normalize
newlines or controls; byte preservation describes the output string.

Input stays usable, and the returned immutable Copy string outlives the call
and its input binding. The function performs linear CPU work, produces at most
six output bytes per input byte, and follows ordinary string allocation behavior.
It opens no resources, returns no error for valid strings, and does not suspend
or use helper workers. Async code calls it normally.

## Choose the context before escaping

This helper does not sanitize HTML or detect where a value will be inserted.
It is insufficient for script/style contents, event-handler attributes,
`style`, `srcdoc`, comments, tag/attribute names, doctypes, unquoted attributes,
or embedded languages. Keep attribute delimiters fixed in trusted markup.

URL values need a separate validation and encoding policy. For example,
`EscapeString("javascript:alert(1)")` leaves that string unchanged. HTML escaping
does not constrain URL schemes or make that value safe in an `href` or `src`.
JavaScript and CSS require their own context-aware serializers and policies.

The separate [template follow-up decision](design/templates.md) records the
requirements for application-driven HTML and plain-text templates. Issue
[#226](https://github.com/maasdi/zore/issues/226) implements basic escaping only.
