# Fmt

An immutable sequence of literal text, bound interpolations, and unbound
parameters, produced by a
[`t"..."` string](../../language/strings.md#formatted-sequences).

Every segment is a [`Str`](./str.md) of literal text, a
[`FmtValue`](./fmt-value.md) interpolation, or a [`FmtParam`](./fmt-param.md)
still waiting to be filled. This type behaves like an immutable array:
indexable, iterable, spreadable, and destructurable.

## Trust

A sequence written as `t"..."` carries a guarantee: its `Str` segments are
program text, and everything interpolated into it is a `FmtValue` or
`FmtParam`. That is what lets a consumer treat the literal segments as the
trusted skeleton of a command and each interpolation as data to be bound or
quoted. [`sqlite`](../sqlite/index.md) is a prime example, as it relies on
this guarantee to exclude SQL injection vulnerabilities.

Toward that end, **no conversion implicitly expands a sequence.** `str` and
plain interpolation (`"$seq"`, `"${seq}"`, `"${seq:s}"`) raise a
[`TypeError`](./type-error.md); `verbatim` and `dbg` give the source form
This avoids accidental expansion before the sequences reaches its consumer
for safe structural quoting or binding of interpolations.
[`format()`](#format-bindings) must be used to explicitly expand a sequence.

**Building a sequence yourself must uphold this guarantee.** The
[constructor](#fmt-segments) accepts whatever segments it is given, so a
`Str` built from untrusted input would subsequently be treated as trusted.
Programmatic construction of `Fmt` must be done with care.

```
# Fine: the query text is literal, the value is bound.
let query = t"select * from users where name = $name"

# Fine: assembled at runtime, but the untrusted value is still bound.
let assembled = Fmt ["select * from users where name = ", (FmtValue name)]

# Danger: untrusted input is treated as a trusted string literal
let injected = Fmt ["select * from users where name = ", name]
```

## Constructor

### `Fmt segments`

Builds a sequence from an iterable of segments.

Writing a `t"..."` is the usual way to get one; the constructor is for
assembling a sequence at runtime.

#### Parameters

| Name       | Type | Description                                            |
| ---------- | ---- | ------------------------------------------------------ |
| `segments` |      | Iterable of `Str`, `FmtValue`, and `FmtParam` segments |

#### Errors

| Exception                      | Condition                                           |
| ------------------------------ | --------------------------------------------------- |
| [`TypeError`](./type-error.md) | A segment is not a `Str`, `FmtValue`, or `FmtParam` |

#### Example

```
let built = Fmt ["a=", (FmtValue 42), ";"]
assert_eq $built.len 3
assert_eq $built.format() "a=42;"
```

## Fields

### `len`

The number of segments.

```
let greeting = t"hello $name!"
assert_eq $greeting.len 3
```

## Methods

### `format ...bindings`

Expands the sequence and returns the result: literal text as it stands, and
each interpolation through its own specification. This is the only way to get
the expansion — see [Trust](#trust).

Given arguments, it fills the [parameters](./fmt-param.md) first, on the same
exhaustive terms as [`(call)`](#call-bindings). Use `(call)` when the filled
sequence is what a consumer wants.

#### Returns

[`Str`](./str.md)

#### Errors

| Exception                                         | Condition                     |
| ------------------------------------------------- | ----------------------------- |
| [`MissingPosError`](./missing-pos-error.md)       | A numbered parameter unfilled |
| [`MissingKeyError`](./missing-key-error.md)       | A named parameter unfilled    |
| [`UnexpectedPosError`](./unexpected-pos-error.md) | A positional argument unused  |
| [`UnexpectedKeyError`](./unexpected-key-error.md) | A keyword argument unused     |

#### Example

```
let count = 3
assert_eq $(t"n=${count:03d}").format() "n=003"

let stmt = t"select * from t where a = ${#0} and c = ${#name}"
assert_eq $stmt.format(1, name: "n") "select * from t where a = 1 and c = n"
```

### `params()`

The unbound [parameter](./fmt-param.md) names, in depth-first, parent-first
order.

#### Returns

[`Set`](./set.md)

#### Example

```
let stmt = t"select * from t where a = ${#0} and c = ${#name}"
assert_eq $stmt.params() (Set([0, :name:]))
assert_eq $(t"no holes").params() (Set([]))
```

### `bind bindings`

Fills the [parameters](./fmt-param.md) `bindings` names and returns the
result. A parameter it does not name stays a parameter, so a template can be
filled in stages.

Binding is keyed lookup: a parameter written `${#0}` is filled by key `0`, and
`${#name}` by key `name`. A dict is what `bind` takes because only a dict can
name a sparse set of positions.

A binding descends into a sequence bound inside the sequence, so a template
pasted into another is filled along with it.

#### Parameters

| Name       | Type                | Description                    |
| ---------- | ------------------- | ------------------------------ |
| `bindings` | [`dict`](./dict.md) | Values keyed by parameter name |

#### Returns

`Fmt`

#### Errors

| Exception                                         | Condition                        |
| ------------------------------------------------- | -------------------------------- |
| [`TypeError`](./type-error.md)                    | `bindings` is not a dict         |
| [`UnexpectedKeyError`](./unexpected-key-error.md) | A named key no parameter uses    |
| [`UnexpectedPosError`](./unexpected-pos-error.md) | An integer key no parameter uses |

#### Example

```
let stmt = t"select * from t where a = ${#0} and c = ${#name}"
let partial = stmt.bind {name: "n"}
assert_eq $(partial.bind {0: 1}).format() "select * from t where a = 1 and c = n"
```

## Operators

### `(call) ...bindings`

Fills every parameter at once and insists the two sides match exactly: each
parameter is filled, and each argument is used.

Positional arguments are sugar for integer keys — argument *i* fills parameter
`i` — so `(call)` and [`bind`](#bind-bindings) substitute identically and differ
only in their checks.

#### Returns

`Fmt`

#### Errors

| Exception                                         | Condition                     |
| ------------------------------------------------- | ----------------------------- |
| [`MissingPosError`](./missing-pos-error.md)       | A numbered parameter unfilled |
| [`MissingKeyError`](./missing-key-error.md)       | A named parameter unfilled    |
| [`UnexpectedPosError`](./unexpected-pos-error.md) | A positional argument unused  |
| [`UnexpectedKeyError`](./unexpected-key-error.md) | A keyword argument unused     |

#### Example

```
let stmt = t"select * from t where a = ${#0} and c = ${#name}"
assert_eq $(stmt 1 name: "n").format() "select * from t where a = 1 and c = n"
```

### Indexing

`seq[i]` returns segment `i`: a [`Str`](./str.md) of literal text, a
[`FmtValue`](./fmt-value.md), or a [`FmtParam`](./fmt-param.md).

```
let greeting = t"hello $name!"
assert_eq $greeting[0] "hello "
assert_eq $greeting[1].source r"$name"
```

Assigning to a segment raises an [`ImmutableError`](./immutable-error.md).

### Iteration

Iterating yields the segments in order, so a consumer can decide what to do
with each.

```
for segment = greeting
  echo $ type $segment
```

The console is the worked example of such a consumer:
[`term`](../term/text.md#sequences) walks a sequence segment by segment rather
than converting it, which is what lets styling interpolated into one survive
to the terminal.

### Equality

Two sequences are equal when their segments are. How a sequence was assembled
does not show. The text each interpolation records does: a segment carries its
[`source`](./fmt-value.md#source), so `t"$name"` and `t"${name}"` are not
equal.

## Formatting

`dbg` gives the source form, reconstructed from the literal text and each
interpolation's [`source`](./fmt-value.md#source). An interpolation built at
runtime has no source, so its bound value's debug form stands in.

`verbatim` — and with it the `!` conversion and command-argument position —
gives the same source form, since that is what the sequence was written as.
`str` and the display conversion raise a [`TypeError`](./type-error.md).
[`format()`](#format-bindings) is the only expansion. See [Trust](#trust).

```
let count = 3
assert_eq (dbg t"n=${count:03d}") r#"t"n=${count:03d}""#
assert_eq (verbatim t"n=${count:03d}") (dbg t"n=${count:03d}")
```

## Example

```
let user = "root"
let seq = t"user: ${user:>8}"

# Expanding gives what the equivalent `"..."` would have.
assert_eq $seq.format() "user:     root"

# The interpolated value is still there to be inspected.
assert_eq $seq[1].value $user
assert_eq $seq[1].width 8
```
