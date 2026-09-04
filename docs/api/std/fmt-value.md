# FmtValue

Binds a value to reusable formatting options.

A [formatted
interpolation](../../language/expressions.md#formatted-interpolation) produces
one for each `${...}` it contains; the constructor is for binding a value
where no interpolation is being written.

`FmtValue` exposes the same formatting fields and `pad` method as
[`FmtSpec`](./fmt-spec.md), but is a distinct type rather than a subtype of
it. Calling it with new keyword options returns a new `FmtValue` retaining
the original value.

## Constructor

### `FmtValue value :fill? :align? :sign? :width? :precision? :alt? :kind? :source?`

#### Parameters

| Name        | Type  | Description                            |
| ----------- | ----- | -------------------------------------- |
| `value`     |       | Value to bind                          |
| `fill`      | ?     | Fill character, `:ZERO:`, or nil       |
| `align`     | Sym?  | `:LEFT:`, `:RIGHT:`, or `:CENTER:`     |
| `sign`      | Sym?  | `:PLUS:` or `:SPACE:`                  |
| `width`     | Int?  | Minimum width in grapheme clusters     |
| `precision` | Int?  | Numeric precision or maximum graphemes |
| `alt`       | Bool? | Enables alternate formatting           |
| `kind`      | Sym?  | Representation kind                    |
| `source`    | Str?  | Text this was written as               |

#### Returns

`FmtValue`

#### Example

```
let money = FmtValue 1.5 precision: 2 kind: :FIXED:
assert_eq (str money) "1.50"
```

## Fields

### `value`

The bound value, one level down. It is itself a `FmtValue` when this one
binds another.

### `source`

The text the value was written as, or nil. Formatted interpolation records
the whole interpolation including its sigil and delimiters, so a consumer can
reproduce the form it was written in.

```
let sourced = FmtValue 42 width: 4 source: r"${x:>4}"
assert_eq $sourced.source r"${x:>4}"
```

## Methods

### `call :fill? :align? :sign? :width? :precision? :alt? :kind?`

Returns a new `FmtValue` with the supplied options merged. Positional
arguments are not accepted. The result is synthetic rather than
source-derived, so its [`source`](#source) is nil.

## Sequencing

Binding a `FmtValue` sequences the two specifications rather than merging
them: the inner one renders the value, and the outer one lays that rendering
out as text. So a width under a width pads what the inner layout produced,
and a precision clips it.

An outer option the inner rendering cannot honor — a numeric `kind`, say — is
a `TypeError`, because a rendering is text. Each level is honored or rejected;
none is skipped.

Nesting is bounded by the ordinary call depth.

```
let money = FmtSpec precision: 2 kind: :FIXED:
let bound = money 1.5
assert_eq "${bound:8}" "1.50    "

# `value` descends exactly one level.
let outer = (FmtValue bound width: 8)
assert_eq (str outer.value) "1.50"
```

## Example

```
let amount = 12.5
let money = FmtSpec precision: 2 kind: :FIXED:
echo "total: $(money $amount)"

# A bound value carries its options into an interpolation that adds more.
assert_eq "${money(amount):>10}" "     12.50"
```

When `kind` is nil, interpolation in an ordinary quoted string or here string
uses display formatting. `str` and `dbg` supply display and debug formatting,
respectively. An explicit `kind` overrides the surrounding operation.
