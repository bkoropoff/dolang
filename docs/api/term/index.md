# term

The `term` module writes sanitized terminal output and constructs ANSI-styled
text.

## Types

| Type                  | Description                     |
| --------------------- | ------------------------------- |
| [`Style`](./style.md) | Reusable terminal style         |
| [`Text`](./text.md)   | Validated terminal presentation |

## Style options

The terminal styling functions accept these keyword options:

| Name            | Type                                                                                                    | Description                        |
| --------------- | ------------------------------------------------------------------------------------------------------- | ---------------------------------- |
| `fg`            | [`Sym`](../std/sym.md)\|[`Int`](../std/int.md)\|[`Array`](../std/array.md)\|[`Tuple`](../std/tuple.md)? | Foreground color                   |
| `bg`            | [`Sym`](../std/sym.md)\|[`Int`](../std/int.md)\|[`Array`](../std/array.md)\|[`Tuple`](../std/tuple.md)? | Background color                   |
| `bold`          | [`Bool`](../std/bool.md)\|[`Sym`](../std/sym.md)?                                                       | Enables bold                       |
| `dim`           | [`Bool`](../std/bool.md)\|[`Sym`](../std/sym.md)?                                                       | Enables dim intensity              |
| `italic`        | [`Bool`](../std/bool.md)\|[`Sym`](../std/sym.md)?                                                       | Enables italics                    |
| `underline`     | [`Bool`](../std/bool.md)\|[`Sym`](../std/sym.md)?                                                       | Enables underlining                |
| `blink`         | [`Bool`](../std/bool.md)\|[`Sym`](../std/sym.md)?                                                       | Enables blinking                   |
| `reverse`       | [`Bool`](../std/bool.md)\|[`Sym`](../std/sym.md)?                                                       | Reverses foreground and background |
| `hidden`        | [`Bool`](../std/bool.md)\|[`Sym`](../std/sym.md)?                                                       | Hides text                         |
| `strikethrough` | [`Bool`](../std/bool.md)\|[`Sym`](../std/sym.md)?                                                       | Enables strikethrough              |

Attribute options accept `true` or `:INHERIT:`. `false` is not accepted.

Named colors are `:BLACK:`, `:RED:`, `:GREEN:`, `:YELLOW:`, `:BLUE:`,
`:MAGENTA:`, `:CYAN:`, and `:WHITE:`. Prefix the name with `BRIGHT_` for a
bright color, such as `:BRIGHT_RED:`. An integer selects a 256-color palette
index. A three-integer array or tuple specifies an RGB color. Numeric values
must be between 0 and 255. Color options also accept `:INHERIT:`.

**Errors:**

- An attribute option is present with a value other than `true` or `:INHERIT:`.
- A color name is unknown, a numeric value is out of range, or an RGB value
  does not contain three integers.

## Values

### `have_terminal`

Whether stderr was a terminal when the process started.

## Functions

### `echo ...args`

Prints arguments separated by spaces, followed by a newline. Ordinary values
are sanitized; direct [`Text`](./text.md) arguments retain their styling.

**Parameters:**

| Name      | Type | Description                                    |
| --------- | ---- | ---------------------------------------------- |
| `...args` | *    | Values converted with `arg` and written safely |

**Returns:** `nil`

```
echo status: ready count: 3
```

### `print :...options ...args`

Prints concatenated values without separators or a trailing newline. Styling
is omitted when stderr is not a terminal.

**Parameters:**

| Name      | Type | Description                         |
| --------- | ---- | ----------------------------------- |
| `...args` | *    | Values converted to display strings |

Also accepts the module's [style options](#style-options). `:INHERIT:` is a
no-op for `print`.

**Returns:** `nil`

```
print "status: " ready fg: :GREEN: bold: true
```

### `style :...options ...args`

Constructs styled terminal text from concatenated values. With no positional
arguments, it returns a reusable [`Style`](./style.md) instead.

**Parameters:**

| Name      | Type | Description                         |
| --------- | ---- | ----------------------------------- |
| `...args` | *    | Values converted to display strings |

Also accepts the module's [style options](#style-options). `:INHERIT:` leaves
a setting to the surrounding style. This is normally the default, but clears
a saved setting when deriving a [`Style`](./style.md).

**Returns:** [`Text`](./text.md) when positional arguments are provided;
otherwise [`Style`](./style.md)

```
let warning = style Warning fg: :YELLOW: bold: true
echo $warning
```

### `preformat text`

Validates existing ANSI-styled text. SGR styling is canonicalized; other
terminal controls, including hyperlinks, are removed.

**Parameters:**

| Name   | Type                   | Description              |
| ------ | ---------------------- | ------------------------ |
| `text` | [`Str`](../std/str.md) | ANSI-formatted input     |

**Returns:** [`Text`](./text.md)

```
let formatted = preformat input
echo $formatted
```

### `render_error error :backtrace?`

Formats an error value and backtrace for terminal presentation.
The returned text does not include a final newline.

**Parameters:**

| Name        | Type                                      | Description                            |
| ----------- | ----------------------------------------- | -------------------------------------- |
| `error`     |                                           | Error value or message                 |
| `backtrace` | [`strand.Backtrace`](../strand/index.md)? | Explicit backtrace; defaults to active |

**Returns:** [`Text`](./text.md)

**Errors:**

- `backtrace` is present and is not a `strand.Backtrace`.
- `backtrace` is omitted outside an active exception handler.

```
try
  operation()
catch error: e
  print $render_error(e)
```

Ordinary values preserve newlines and tabs but remove other C0/C1 controls and
escape sequences. Raw stdout and stderr sinks are unchanged and are not
sanitized by this module.
