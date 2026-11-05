# String equality and bounded search

Use `==` and `!=` to compare two `str` values by content:

```rill
fn main() {
    "one" == "one" && "one" != "two"
}
```

This prints `true`. Equality compares exact UTF-8 bytes and lengths, including
embedded NUL bytes. It is case-sensitive, with no case folding or Unicode
normalization.
It does not compare string addresses. Both operands evaluate once, left to
right. `str` still denotes program-lifetime text; equality adds no runtime
string construction, slicing or implicit allocation.

Both operators return `bool`. Ordinary string ordering with `<`, `<=`, `>` or
`>=` remains unsupported; use `std.text.compare` for explicit byte ordering.
There is no generic equality trait or inferred equality constraint. Constrain
string-comparison helpers with closed `str` parameter types instead of assuming
an unconstrained generic equality function is supported.

The ordinary library helper remains available:

```rill
import std.text as text;

fn main() {
    text.equal("one", "one")
}
```

Use [text helpers](standard-library.md) for lexicographic ordering, prefix/suffix
checks or comparing a byte span without allocating a substring. Whole records
and enums still reject ordinary equality. Compare relevant fields or match
variants and compare their admitted payloads explicitly; matching `Some(index)`
does not require an equality operation on Option itself.

An explicit helper makes the comparison policy visible. This
[complete example](../examples/search/compare-values.rill) compares both record
fields, then uses nested matches for enum variants. It prints `true`:

```rill
type Label = { name: str; code: i32 }
type Entry = Absent | Present(Label)

fn same_label(read left: Label, read right: Label) -> bool {
    left.name == right.name && left.code == right.code
}

fn same_entry(left: Entry, right: Entry) -> bool {
    match left {
        Absent => match right { Absent => true | Present(_) => false }
        | Present(a) => match right {
            Absent => false
            | Present(b) => same_label(a, b)
        }
    }
}

fn main() {
    let first = record Label { name = "one"; code = 1 };
    let second = record Label { name = "one"; code = 1 };
    let different = record Label { name = "two"; code = 2 };
    same_entry(Present(first), Present(second)) &&
    !same_entry(Present(first), Present(different)) &&
    same_entry(Absent, Absent) && !same_entry(Absent, Present(first))
}
```

Different variants compare false here; matching `Present` values compare both
payload fields. This is an ordinary function over these finite Copy types,
not a derived equality rule for arbitrary records, enums or owned payloads.
Wider equality needs a separate payload and ownership design. The
[language proposal queue](proposals/rust-migration.md) records the process for
considering new forms without treating them as implemented features.

## Search a string with the library

For one substring, prefer `std.text.find`. It returns the first byte offset as
an Option, and returns `None` when there is no match:

```rill
import std.text as text;
import std.option as option;

fn main() {
    match text.find("stone", "one") {
        option.Some(offset) => offset
        | option.None => -1
    }
}
```

This [complete example](../examples/search/substring.rill) prints `2`.
Offsets count bytes, not Unicode characters. An empty needle yields `Some(0)`.
Use `rfind` for the last matching byte offset, or `contains` for a boolean.
These functions inspect the supplied string without allocating another one.

## Stop a manual token scan

Rill has no `break`, `continue` or early `return` statement yet. A search can
stop through its `while` condition, but setting `found` does not jump out of
the current body. Guard any suffix work that must be skipped after a match.

This [complete program](../examples/search/first-token.rill) searches five
supplied tokens and prints `1`:

```rill
type Search = { found: i32; visited: i32; unmatched: i32; next_index: i32 }

fn scan(limit: i32, token_at, target: str) -> Search {
    let mut index = 0;
    let mut found = -1;
    let mut visited = 0;
    let mut unmatched = 0;
    while index < limit && found < 0 {
        let token = token_at(index);
        visited <- visited + 1;
        if token == target {
            found <- index;
        };
        if found < 0 {
            unmatched <- unmatched + 1;
        };
        index <- index + 1;
    };
    record Search {
        found = found;
        visited = visited;
        unmatched = unmatched;
        next_index = index
    }
}

fn tokens(index: i32) -> str {
    if index == 0 { "zero" } else {
        if index == 1 { "one" } else {
            if index == 2 { "two" } else {
                if index == 3 { "one" } else { "end" }
            }
        }
    }
}

fn main() {
    let result = scan(5, tokens, "one");
    result.found
}
```

`scan` takes an ordinary concrete callback. The fixed `tokens` function keeps
this example independent of an array, iterator or argv interface. The caller
supplies a valid token count for its provider; a nonpositive limit visits no
tokens. `found` is `-1` for no match and otherwise the first matching token index.
An empty target matches an empty token, not every token.

For this input, the result fields are:

| Field | Value | Reason |
| --- | ---: | --- |
| `found` | 1 | The first `"one"` is at token index 1 |
| `visited` | 2 | The scan inspected `"zero"` and `"one"` |
| `unmatched` | 1 | The guarded suffix ran only for `"zero"` |
| `next_index` | 2 | The current body's final increment still ran after the match |

The next condition sees `found >= 0` and stops. The later duplicate at index 3
is never visited. If the increment must also be skipped, move it into a suitable
guard, but ensure every continuing path still advances. Nested scans need
their own stop conditions.

See the [control-flow migration patterns](rust-to-rill.md#control-flow) for
filtering and helper-function alternatives. [Loop exit proposals](proposals/rust-migration.md#control-flow)
record the cleanup and loan rules needed before adding abrupt exits. These
examples do not assume the contents of any particular puzzle or user function.
