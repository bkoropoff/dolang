# Fmt

Binds a value to reusable formatting options.

`Fmt` is a nominal subtype of [`FmtSpec`](./fmt-spec.md) and exposes the same
formatting fields and `pad` method. Calling it with new keyword options returns
a new `Fmt` retaining the original value.

## Fields

### `value`

The bound value.

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
```

When `kind` is nil, string interpolation uses verbatim formatting, `str` uses
display formatting, and `dbg` uses debug formatting. An explicit `kind`
overrides the surrounding operation.
