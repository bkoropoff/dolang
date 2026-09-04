# Style

Stores reusable terminal style settings.

Calling a `Style` applies its saved settings. Supplied options override saved
colors or enable additional attributes. Omitted options retain saved settings;
`:INHERIT:` clears them.

## Constructor

### `Style :...options`

Creates a reusable style. Accepts the
[`term` style options](./index.md#style-options) and nothing else: a
positional argument is text to style, which is
[`term.text`](./index.md#text-options-args)'s job.

#### Returns

`Style`

#### Example

```
let warning = term.Style fg: :YELLOW: bold: true
```

## Operators

### `style :...options ...args`

Applies or derives the saved style.

#### Parameters

| Name      | Type | Description                         |
| --------- | ---- | ----------------------------------- |
| `...args` | *    | Values converted to display strings |

Also accepts the [`term` style options](./index.md#style-options).

#### Returns

[`Text`](./text.md) when positional arguments are provided;
otherwise a derived `Style`

#### Example

```
let warning = term.Style fg: :YELLOW: bold: true
let urgent = warning fg: :RED: underline: true
let uncolored = urgent fg: :INHERIT:

echo $warning("Warning")
echo $urgent("Failure")
echo $uncolored("Notice")
```
