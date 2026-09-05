# FmtParam

An unbound position in a [`Fmt`](./fmt.md) — a hole waiting to be filled.

A [`FmtValue`](./fmt-value.md) already carries a value. A `FmtParam` carries
only a name and the options that will apply once something is bound to it, so
a template can be written once and filled in later: a prepared statement, a
reusable message.

`FmtParam` exposes the same formatting fields and `pad` method as
[`FmtSpec`](./fmt-spec.md). Calling it with new keyword options returns a new
`FmtParam` naming the same parameter.

## Names, not positions

A name is an [`Int`](./int.md) or a [`sym`](./sym.md), and filling a hole is
keyed lookup either way. An integer name is a name that happens to be a
number: it is never renumbered, so `${#0}` means parameter `0` wherever it
appears, including inside a sequence pasted into another one.

Filling is [`Fmt.call`](./fmt.md#call-bindings), which fills every hole at
once, and [`Fmt.bind`](./fmt.md#bind-bindings), which fills some and leaves
the rest.

## Constructor

### `FmtParam name :fill? :align? :sign? :width? :precision? :alt? :kind? :source?`

#### Parameters

| Name     | Type                                 | Description                       |
| -------- | ------------------------------------ | --------------------------------- |
| `name`   | [`Int`](./int.md)\|[`sym`](./sym.md) | The parameter this position names |
| `source` | [`Str`](./str.md)?                   | Text this was written as          |

The formatting options are those of [`FmtSpec`](./fmt-spec.md), and are
applied to whatever value is eventually bound.

#### Returns

`FmtParam`

#### Errors

| Exception                      | Condition                              |
| ------------------------------ | -------------------------------------- |
| [`TypeError`](./type-error.md) | `name` is neither an `Int` nor a `sym` |

#### Example

```
let hole = FmtParam :user: width: 8
assert_eq $hole.name :user:
assert_eq $hole.width 8
```

## Fields

### `name`

The parameter named by this position, as an `Int` or a `sym`.

### `source`

The text the parameter was written as, or nil. A `${#...}` in a
[`t"..."`](../../language/expressions.md#formatted-sequences) records the
whole interpolation, sigil and delimiters included.

```
let seq = t"a=${#0:>3}"
assert_eq $seq[1].source r"${#0:>3}"
```

## Methods

### `call :fill? :align? :sign? :width? :precision? :alt? :kind?`

Returns a new `FmtParam` naming the same parameter with the supplied options
merged. Positional arguments are not accepted. The result is synthetic rather
than source-derived, so its [`source`](#source) is nil.

## Operators

### Equality

Two parameters are equal when they name the same parameter under the same
specification and record the same [`source`](#source).

## Formatting

A parameter converts to the text it was written as — `str`, `dbg`, and
`verbatim` alike. Built at runtime with no [`source`](#source), it shows the
form it would have been written in.

```
assert_eq (str (FmtParam 0)) r"${#0}"
assert_eq (str (FmtParam :user:)) r"${#user}"
```

Unlike a [`Fmt`](./fmt.md), a parameter does not refuse conversion. A sequence
refuses because flattening it produces text with its interpolations already
substituted and nothing left to tell them from the literal skeleton. A
parameter has no bound value at all, so showing it gives nothing away.

What it cannot do is stand in for a value: expanding a sequence that still
contains one raises a [`ValueError`](./value-error.md), rather than emitting
something hole-shaped where a value was meant to go. See
[`Fmt.format`](./fmt.md#format).
