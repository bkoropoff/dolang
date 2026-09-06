# FmtParam

An unbound interpolation in a [`Fmt`](./fmt.md).

A [`FmtValue`](./fmt-value.md) already carries a value. A `FmtParam` carries
only a name and the options that will apply once something is bound to it, so
a template can be written once and filled in later: a prepared statement, a
reusable message.

`FmtParam` exposes the same formatting fields and `pad` method as
[`FmtSpec`](./fmt-spec.md). Calling it with new keyword options returns a new
`FmtParam` naming the same parameter.

## Names

A name is an [`Int`](./int.md) or a [`Sym`](./sym.md). Integer names are
never renumbered when `Fmt`s are combined or nested.

A hole is written `${#name}`, or `$#name` when it states no specification.

[`Fmt.(call)`](./fmt.md#call-bindings) fills every parameter at
once; [`Fmt.bind`](./fmt.md#bind-bindings) can partially fill parameters.

## Constructor

### `FmtParam name :fill? :align? :sign? :width? :precision? :alt? :kind? :source?`

#### Parameters

| Name     | Type                                 | Description                        |
| -------- | ------------------------------------ | ---------------------------------- |
| `name`   | [`Int`](./int.md)\|[`sym`](./sym.md) | The parameter this position names  |
| `source` | [`Str`](./str.md)?                   | Interpolation as written in source |

The formatting options are those of [`FmtSpec`](./fmt-spec.md), and are
applied to whatever value is eventually bound.

#### Errors

| Exception                      | Condition                              |
| ------------------------------ | -------------------------------------- |
| [`TypeError`](./type-error.md) | `name` is neither an `Int` nor a `Sym` |

#### Example

```
let hole = FmtParam :user: width: 8
assert_eq $hole.name :user:
assert_eq $hole.width 8
```

## Fields

### `name`

The parameter named by this position, as an `Int` or a `Sym`.

### `source`

The text the parameter was written as, or nil. A `${#...}` in a
[`t"..."`](../../language/strings.md#formatted-sequences) records the
whole interpolation, sigil and delimiters included.

```
let seq = t"a=${#0:>3}"
assert_eq $seq[1].source r"${#0:>3}"
```

## Operators

### `(call) :fill? :align? :sign? :width? :precision? :alt? :kind?`

Returns a new `FmtParam` naming the same parameter with the supplied options
merged. Positional arguments are not accepted. The result is synthetic rather
than source-derived, so its [`source`](#source) is nil.

### Equality

Two parameters are equal when they name the same parameter under the same
specification and record the same [`source`](#source).

## Coercion

A `FmtParam` coerces to string form as its source text. If no
[`source`](#source) exists, the syntax is synthesized.

```
assert_eq (str (FmtParam 0)) r"${#0}"
assert_eq (str (FmtParam :user:)) r"${#user}"
```
