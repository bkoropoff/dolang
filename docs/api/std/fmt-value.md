# FmtValue

Binds a value to reusable formatting options.

`FmtValue` exposes the same formatting fields and `pad` method as
[`FmtSpec`](./fmt-spec.md), but is a distinct type rather than a subtype of
it. Calling it with new keyword options returns a new `FmtValue` retaining
the original value.

## Fields

### `value`

The bound value, one level down. It is itself a `FmtValue` when this one
binds another.

### `source`

Reserved source-expression text. This is currently always nil.

## Methods

### `call :fill? :align? :sign? :width? :precision? :alt? :kind?`

Returns a new `FmtValue` with the supplied options merged. Positional
arguments are not accepted.

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
let money = fmt precision: 2 kind: :FIXED:
let bound = money 1.5
assert_eq (str $ fmt bound width: 8) "1.50    "

# `value` descends exactly one level.
let outer = fmt bound width: 8
assert_eq (str outer.value) "1.50"
```

## Example

```
let amount = 12.5
let money = fmt precision: 2 kind: :FIXED:
echo "total: $(money $amount)"

let column = fmt (money amount) width: 10 align: :RIGHT:
assert_eq (str column) "     12.50"
```

When `kind` is nil, interpolation in an ordinary quoted string or here string
uses display formatting. `str` and `dbg` supply display and debug formatting,
respectively. An explicit `kind` overrides the surrounding operation.
