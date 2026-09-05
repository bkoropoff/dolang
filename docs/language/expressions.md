# Expressions

Do has three expression contexts: full expressions (inside delimiters), compact
expressions (after `$`), and string interpolation (inside quoted strings).

## Full Expressions

Within parentheses `()`, brackets `[]`, and braces `{}`, Do uses C-like syntax
where whitespace is insignificant and operators are available:

```
let result = (1 + 2 * 3)     # 7
let arr = [1, 2, 3]
let dict = {name: "Alice", age: 30}
```

Full expressions support:

- Arithmetic operators: `+`, `-` (including unary negation), `*`, `/`, `//`, `%`
- Comparison operators: `==`, `!=`, `<`, `<=`, `>`, `>=`
- Logical operators: `&&`, `||`, `!`
- Bitwise operators: `&`, `|`, `^`, `~`, `<<`, `>>`
- Range expressions: `a..b`, `..b`, `a..`, `..`
- Function calls:
    - Juxtaposition: `echo "hello" "world"`
    - C-style: `echo("hello", "world")`, `func()`
- Indexing: `arr[0]`, `dict["key"]`
- Field access: `obj.field`

Full expressions can span multiple lines:

```
let value = (
  some_long_computation(x, y) +
  another_value * factor
)
```

### Operator Precedence

From lowest to highest:

1. `$` (low-precedence call)
2. `||`
3. `&&`
4. `==`, `!=`, `<`, `<=`, `>`, `>=`
5. Range: `a..b`, `..b`, `a..`, `..`
6. `|`
7. `^`
8. `&`
9. `<<`, `>>`
10. `+`, `-`
11. `*`, `/`, `//`, `%`
12. Unary `-`, `!`, `~`
13. Call, index, field access

### Ternary-Style Expressions

There is no dedicated ternary operator, but `&&` and `||` short-circuit and
return their operands (as in Lua or Python), so you can write:

```
let label = (condition && "yes" || "no")
```

### Range Expressions

`..` constructs a [`Range`](../api/std/range.md) value.

```
let bounded = 1..5
let from_start = ..5
let to_end = 1..
let all = ..
```

`a..b` is half-open: it includes `a` and excludes `b`.

Open-ended forms are primarily used for slicing:

```
let arr = [0, 1, 2, 3]
assert_eq $arr[1..3] [1, 2]
assert_eq $arr[..2] [0, 1]
assert_eq $arr[2..] [2, 3]
assert_eq $arr[..] [0, 1, 2, 3]
```

Bounded ranges are iterable. `a..` is also iterable and unbounded. `..b` and
`..` are not iterable because they have no starting value.

## Compact Expressions

The `$` prefix introduces a compact expression at statement level. It supports:

- Variable access: `$name`
- Field access: `$person.name`
- Indexing: `$arr[0]`
- Range expressions: `$start..end`, `$start..`, `$..end`, `$..`
- C-style calls: `$func(arg1, arg2)`
- Chaining: `$obj.method(arg).field[0]`
- Boolean not: `$!flag`

```
let person = {name: "Alice", age: 30}
echo $person.name
echo $person["age"]
echo $Str(person.age)
```

### Implicit

Some special statement forms expect compact expression without `$` to avoid
needing to write it in those cases:

- The right-hand side of a `let` or assignment
- The condition of an `if` or `while`
- The iteratee of a `for`
- The scrutinee of a `bind`
- After `return` or `throw`

In fact, using `$` unnecessarily in these contexts is a syntax error.

## Quoted Strings

Double-quoted strings support interpolation with `$`. `$` behavior is more
conservative than in compact expressions:

- Simple variable substitution works: `"hello $name"`
- Anything beyond basic variable access must use `$()`: `"result: $(1 + 2)"`

```
let name = Alice
let age = 30

echo "Hello, $name!"
echo "$name is $age years old"
echo "In 10 years: $(age + 10)"
echo "Type: $(type name)"
```

### Formatted Interpolation

Ordinary quoted strings and non-raw here strings accept
`${value:format-spec}`. The value uses compact-expression syntax; wrap it in
parentheses to use a full expression.

```
let count = 42
echo "count: ${count:05d}"
echo "total: ${(subtotal + tax):8.2f}"
```

The specification may be omitted, along with its `:`, to interpolate with no
options: `"${count}"`.

The format specification is:

```
[[fill]align][sign][#][0][width][.precision][conversion]
```

`align` is `<`, `>`, or `^`. A preceding Unicode scalar sets the fill
character. `sign` is `+` or a space, `#` enables alternate formatting, and
`0` selects numeric-aware zero padding. Width and precision are decimal
counts.

Conversions select the representation:

| Conversion | `kind`       |
| ---------- | ------------ |
| `s`        | `:STR:`      |
| `?`        | `:DBG:`      |
| `!`        | `:VERBATIM:` |
| `x`        | `:HEX:`      |
| `o`        | `:OCT:`      |
| `b`        | `:BIN:`      |
| `d`        | `:DEC:`      |
| `e`        | `:EXP:`      |
| `f`        | `:FIXED:`    |

Without a conversion, quoted strings and here strings use display (`:STR:`)
conversion. The options and validation are the same as
[`FmtValue`](../api/std/fmt-value.md) and
[`FmtSpec`](../api/std/fmt-spec.md).

Width and precision may use `$name` or `$(expression)` instead of a decimal
count:

```
let width = 8
let precision = 2
echo "${amount:$(width).$(precision)f}"
```

A bare substitution consumes its full identifier. Use parentheses when a
conversion immediately follows it: `$(width)f`, not `$widthf`. A format
specification must not contain a newline. Formatted interpolation is not
available in binary or raw strings.

Escape sequences in quoted strings:

| Sequence | Meaning             |
| -------- | ------------------- |
| `\n`     | Newline             |
| `\t`     | Tab                 |
| `\\`     | Backslash           |
| `\"`     | Double quote        |
| `\$`     | Literal dollar sign |

Binary strings (`b"..."`) additionally support hex byte escapes:

| Sequence | Meaning                                |
| -------- | -------------------------------------- |
| `\xNN`   | Byte with hex value `NN` (e.g. `\xff`) |

`\xNN` is only valid inside binary strings; using it in a regular string is a
syntax error.

### Formatted Sequences

Prefixing a quoted string or here string introducer with `t` — `t"..."`,
`t|`, `t|-` — produces a [`Fmt`](../api/std/fmt.md) instead of a `Str`. The
interpolation syntax is exactly the same; what differs is that the segments
are kept apart rather than concatenated, so a consumer sees each interpolated
value instead of only the text it produced.

```
let name = "Alice"
let seq = t"hello ${name:>8}!"

# Expanding gives what the equivalent `"..."` would have.
assert_eq $seq.format() "hello    Alice!"

# But the interpolated value is still there.
assert_eq $seq.len 3
assert_eq $seq[1].value $name
```

Every interpolation is a [`FmtValue`](../api/std/fmt-value.md), whether or not
it states a specification, and each records the text it was written as. The
literal text between them is an ordinary `Str`.

A sequence never expands implicitly: `str` and `"$seq"` raise rather than
flattening it, and expansion has to be asked for with
[`format`](../api/std/fmt.md#format). See
[Trust](../api/std/fmt.md#trust) — the distinction between literal text and
interpolated values is what a consumer such as a query builder acts on.

As with `r"..."`, the prefix shadows a juxtaposition call: `t"x"` is a
sequence, not a call to `t`.

See [Here Strings](basic-types.md#here-strings) for multi-line string literals
that use the same interpolation syntax.

See [Binary Strings](basic-types.md#binary-strings-bin) for details on `Bin`
literals and their methods.
