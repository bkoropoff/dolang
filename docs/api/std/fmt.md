# Fmt

Binds a value to reusable formatting options.

`Fmt` is a nominal subtype of [`FmtSpec`](./fmt-spec.md) and exposes the same
formatting fields and `pad` method. Calling it with new keyword options returns
a new `Fmt` retaining the original value.

## Fields

### `value`

The bound value. Never a `Fmt`: binding one is a `TypeError`.

### `source`

Reserved source-expression text. This is currently always nil.

## Methods

### `call :fill? :align? :sign? :width? :precision? :alt? :kind?`

Returns a new `Fmt` with the supplied options merged. Positional arguments are
not accepted.

## Example

```
let money = fmt precision: 2 kind: :FIXED:
echo "total: $(money $amount)"

# Laying a bound value out again restates the options over its `value`,
# rather than binding the `Fmt` itself.
let bound = money amount
let column = fmt bound.value width: 10 align: :RIGHT: precision: 2 kind: :FIXED:
```

When `kind` is nil, interpolation in an ordinary quoted string or here string
uses display formatting. `str` and `dbg` supply display and debug formatting,
respectively. An explicit `kind` overrides the surrounding operation.
