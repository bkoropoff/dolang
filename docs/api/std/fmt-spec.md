# FmtSpec

Stores reusable formatting options.

`FmtSpec` values are immutable. Calling one returns a new specification with
the supplied options overridden, or a bound [`FmtValue`](./fmt-value.md) when
given a positional value.

## Constructor

### `FmtSpec :fill? :align? :sign? :width? :precision? :alt? :kind?`

#### Parameters

| Name        | Type  | Description                            |
| ----------- | ----- | -------------------------------------- |
| `fill`      | Str?  | Fill character, `:ZERO:`, or nil       |
| `align`     | Sym?  | `:LEFT:`, `:RIGHT:`, or `:CENTER:`     |
| `sign`      | Sym?  | `:PLUS:` or `:SPACE:`                  |
| `width`     | Int?  | Minimum width in grapheme clusters     |
| `precision` | Int?  | Numeric precision or maximum graphemes |
| `alt`       | Bool? | Enables alternate formatting           |
| `kind`      | Sym?  | Representation kind                    |

#### Example

```
let money = FmtSpec precision: 2 kind: :FIXED:
assert_eq "$(money 1.5)/$(money 2.25)" "1.50/2.25"
```

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

| Kind         | Representation     | Conversion |
| ------------ | ------------------ | ---------- |
| `:STR:`      | string             | `s`        |
| `:DBG:`      | debug              | `?`        |
| `:VERBATIM:` | verbatim           | `!`        |
| `:HEX:`      | integer (hex)      | `x`        |
| `:OCT:`      | integer (octal)    | `o`        |
| `:BIN:`      | integer (binary)   | `b`        |
| `:DEC:`      | integer (decimal)  | `d`        |
| `:EXP:`      | float (scientific) | `e`        |
| `:FIXED:`    | float (fixed)      | `f`        |

The Conversion column gives the equivalent character in a [formatted
interpolation](../../language/strings.md#formatted-interpolation).

## Operators

### `(call) value? :fill? :align? :sign? :width? :precision? :alt? :kind?`

Returns a new specification, or a bound [`FmtValue`](./fmt-value.md) when
`value` is provided. Omitted options retain their current values; nil resets
an option.

Binding a [`FmtValue`](./fmt-value.md) nests it: the inner specification
renders the value and this one lays that rendering out. See
[Sequencing](./fmt-value.md#sequencing).

## Methods

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
let column = FmtSpec fill: . align: :CENTER: width: 8
echo $column.pad("name")
```
