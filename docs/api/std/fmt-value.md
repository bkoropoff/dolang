# FmtValue

Binds a value to reusable formatting options.

`FmtValue` exposes the same formatting fields and `pad` method as
[`FmtSpec`](./fmt-spec.md), but is a distinct type rather than a subtype of
it. Calling it with new keyword options returns a new `FmtValue` retaining
the original value.

## Fields

### `value`

The bound value. Never a `FmtValue`: binding one is a `TypeError`.

### `source`

Reserved source-expression text. This is currently always nil.

## Methods

### `call :fill? :align? :sign? :width? :precision? :alt? :kind?`

Returns a new `FmtValue` with the supplied options merged. Positional
arguments are not accepted.

## Example

```
let money = fmt precision: 2 kind: :FIXED:
echo "total: $(money $amount)"

# Laying a bound value out again restates the options over its `value`,
# rather than binding the `FmtValue` itself.
let bound = money amount
let column = fmt bound.value width: 10 align: :RIGHT: precision: 2 kind: :FIXED:
```

When `kind` is nil, interpolation in an ordinary quoted string or here string
uses display formatting. `str` and `dbg` supply display and debug formatting,
respectively. An explicit `kind` overrides the surrounding operation.
