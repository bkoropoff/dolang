# FmtSpec

Stores reusable formatting options.

`FmtSpec` values are immutable. Calling one returns a new specification with
the supplied options merged, or a bound [`Fmt`](./fmt.md) when given a
positional value.

## Fields

### `fill`

The fill character, `:ZERO:` for numeric zero padding, or nil for spaces.

### `align`

`:LEFT:`, `:RIGHT:`, `:CENTER:`, or nil for the formatted value's default.

### `sign`

`:PLUS:`, `:SPACE:`, or nil to show only negative signs.

### `width`

The minimum display width, or nil when no minimum is set.

### `precision`

The precision or maximum display width, depending on the formatted value, or
nil when unset.

### `alt`

Whether alternate formatting is enabled.

### `kind`

The requested representation, or nil to inherit the surrounding conversion.
Supported values are `:STR:`, `:DBG:`, `:VERBATIM:`, `:HEX:`, `:OCT:`,
`:BIN:`, `:DEC:`, `:EXP:`, and `:FIXED:`.

## Methods

### `call value? :fill? :align? :sign? :width? :precision? :alt? :kind?`

Returns a new specification, or a bound [`Fmt`](./fmt.md) when `value` is
provided. Omitted options retain their current values; nil resets an option.

### `pad value`

Applies fill, alignment, width, and truncation to a string.

#### Parameters

| Name    | Type                | Description          |
| ------- | ------------------- | -------------------- |
| `value` | [`Str`](./str.md)   | String to lay out    |

#### Returns

[`Str`](./str.md)

#### Example

```
let column = fmt fill: . align: :CENTER: width: 8
echo $column.pad("name")
```
