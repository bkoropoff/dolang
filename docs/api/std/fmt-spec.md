# FmtSpec

Stores reusable formatting options.

`FmtSpec` values are immutable. Calling one returns a new specification with
the supplied options merged, or a bound [`FmtValue`](./fmt-value.md) when
given a
positional value.

## Fields

### `fill`

The fill character, `:ZERO:` for numeric zero padding, or nil for spaces.

### `align`

`:LEFT:`, `:RIGHT:`, `:CENTER:`, or nil for the formatted value's default.

### `sign`

`:PLUS:`, `:SPACE:`, or nil to show only negative signs.

### `width`

The minimum width in extended grapheme clusters, or nil when unset. A value
may measure itself differently: [`term.Text`](../term/text.md) measures
terminal cells.

### `precision`

The numeric precision or maximum number of extended grapheme clusters,
depending on the formatted value, or nil when unset.

### `alt`

Whether alternate formatting is enabled.

### `kind`

The requested representation, or nil to inherit the surrounding conversion.
Supported values are `:STR:`, `:DBG:`, `:VERBATIM:`, `:HEX:`, `:OCT:`,
`:BIN:`, `:DEC:`, `:EXP:`, and `:FIXED:`.

## Methods

### `call value? :fill? :align? :sign? :width? :precision? :alt? :kind?`

Returns a new specification, or a bound [`FmtValue`](./fmt-value.md) when
`value` is provided. Omitted options retain their current values; nil resets
an option.

Binding a [`FmtValue`](./fmt-value.md) is a `TypeError`: a bound value stays
one level deep, so a consumer never has to unwrap a chain of specifications.
Bind [`value`](./fmt-value.md#value) and state the combined options
explicitly instead.

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
