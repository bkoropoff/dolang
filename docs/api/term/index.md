# term

The `term` module interfaces with terminals and consoles.

## Types

| Type                               | Description                             |
| ---------------------------------- | --------------------------------------- |
| [`Console`](./console.md)          | Destination for human-readable output   |
| [`Geometry`](./geometry.md)        | Dimensions of a terminal-backed console |
| [`SinkConsole`](./sink-console.md) | Console over an ordinary sink           |
| [`Style`](./style.md)              | Reusable terminal style                 |
| [`Text`](./text.md)                | Validated terminal presentation         |

## Style options

The terminal styling functions accept these key options:

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

### Errors

- An attribute option is present with a value other than `true` or `:INHERIT:`.
- A color name is unknown, a numeric value is out of range, or an RGB value
  does not contain three integers.

## Values

### `console`

The host [`Console`](./console.md), which may be taken over by an extension
such a `progress.`

Unlike [`output()`](#output), it is not intercepted by an enclosing
[`capture`](#capture-console-func-args-mode).

### `default`

A [`Console`](./console.md) that forwards every operation to whatever
[`output()`](#output) currently resolves to, resolved fresh on each call
rather than once. Bound as the main strand's implicit output when stdout is a
terminal, so unnamed program output keeps following capture and `progress`
takeover for the life of the process — see
[Terminal output](../../shell/terminal-output.md#child-process-output).

## Functions

### `capture console func ...args :mode?`

Runs a function with `console` installed as the ambient console, then flushes
it and restores the previous one.

`console` may be any [`Console`](./console.md), or any
[`Sink`](../std/sink.md) — a plain sink is wrapped in a
[`SinkConsole`](./sink-console.md), using the requested `mode:`.

The override is inherited by all strands spawned inside the call.

#### Parameters

| Name      | Type                            | Description                           |
| --------- | ------------------------------- | ------------------------------------- |
| `console` | [`Console`](./console.md)\|sink | Destination to install                |
| `func`    | `Func`                          | Block to run                          |
| `mode`    | [`sym`](../std/sym.md)?         | `:LINE:` (default) or `:CHUNK:`       |
| `...`     |                                 | Additional arguments passed to `func` |

#### Returns

Return value of `func`.

#### Errors

- Raises [`ValueError`](../std/value-error.md) if `mode:` is given when
  `console` is already a `Console`, which already determines its output mode.

Captured lines keep their terminator, since the capture reproduces what was
written rather than reinterpreting it:

#### Example

```
let lines = []
term.capture $lines do
  echo "Hello, Alice!"
assert_eq $lines ["Hello, Alice!\n"]
```

Put a [`prechomp`](../std/sink.md#prechomp) in front of the sink to strip them:

```
let lines = []
term.capture (lines.prechomp()) do
  echo "Hello, Alice!"
assert_eq $lines ["Hello, Alice!"]
```

The scope always ends with a flush, so an unterminated `print` still arrives —
with no terminator, because none was written:

```
let out = []
term.capture $out do print hi
assert_eq $out ["hi"]
```

### `echo ...args`

Prints arguments separated by spaces, followed by a newline. Ordinary values
are sanitized; [`Text`](./text.md) arguments retain their styling, as does a
[`FmtValue`](../std/fmt-value.md) bound to one — see
[Formatting](./text.md#formatting). A [`Fmt`](../std/fmt.md) is expanded
segment by segment, so styling interpolated into one survives — see
[Sequences](./text.md#sequences).

#### Parameters

| Name      | Type | Description                                         |
| --------- | ---- | --------------------------------------------------- |
| `...args` | *    | Values converted with `verbatim` and written safely |

#### Example

```
echo status: ready count: 3
```

### `mute func ...args`

Runs a function with default console-bound output silenced: `echo`, `print`,
unredirected program `stderr`, etc.

#### Parameters

| Name      | Type   | Description                           |
| --------- | ------ | ------------------------------------- |
| `func`    | `Func` | Block to run                          |
| `...args` |        | Additional arguments passed to `func` |

#### Returns

Return value of `func`.

#### Example

```
# Nothing from this reaches the terminal.
mute do run printf "this will not be printed"
```

### `output()`

Returns the current output console: the one installed by an enclosing
[`capture`](#capture-console-func-args-mode), or [`console`](#console) if there
is none. This is where `echo`, `print`, diagnostics, and unredirected child
process output go.

#### Returns

[`Console`](./console.md)

### `preformat text`

Validates existing ANSI-styled text. SGR styling is canonicalized; other
terminal controls, including hyperlinks, are removed.

#### Parameters

| Name   | Type                   | Description              |
| ------ | ---------------------- | ------------------------ |
| `text` | [`Str`](../std/str.md) | ANSI-formatted input     |

#### Returns

[`Text`](./text.md)

#### Example

```
let formatted = preformat input
echo $formatted
```

### `print :...options ...args`

Prints concatenated values without separators or a trailing newline. Styling
is omitted when stderr is not a terminal.

#### Parameters

| Name      | Type | Description                         |
| --------- | ---- | ----------------------------------- |
| `...args` | *    | Values converted to display strings |

Also accepts the module's [style options](#style-options). `:INHERIT:` is a
no-op for `print`.

#### Example

```
print "status: " ready fg: :GREEN: bold: true
```

### `render_error error :backtrace?`

Formats an error value and backtrace for terminal presentation.
The returned text does not include a final newline.

#### Parameters

| Name        | Type                                      | Description                            |
| ----------- | ----------------------------------------- | -------------------------------------- |
| `error`     |                                           | Error value or message                 |
| `backtrace` | [`strand.Backtrace`](../strand/index.md)? | Explicit backtrace; defaults to active |

#### Returns

[`Text`](./text.md)

#### Errors

| Exception    | Condition                                                  |
| ------------ | ---------------------------------------------------------- |
| `TypeError`  | `backtrace` is present but is not a `strand.Backtrace`     |
| `StateError` | `backtrace` is omitted outside an active exception handler |

#### Example

```
try
  operation()
catch error: e
  print $render_error(e)
```

Ordinary values preserve newlines and tabs but remove other C0/C1 controls and
escape sequences. A [`Text`](./text.md) keeps its styling, and so does a
[`FmtValue`](../std/fmt-value.md) bound to one, whose layout is applied to
the encoded form — see [Formatting](./text.md#formatting). A
[`Fmt`](../std/fmt.md) is expanded segment by segment — see
[Sequences](./text.md#sequences). Raw stdout and
stderr sinks are unchanged and are not sanitized by this module.

### `sub func :chomp? :can_style? ...args`

Runs a function and returns its console output as a string. The console
counterpart to [`proc.sub`](../proc/index.md#sub-func-chomp), which captures a
strand's implicit output stream.

Verbatim output is captured, which must be valid UTF-8. One final line ending
(LF or CRLF) is removed unless `chomp: false`.

#### Parameters

| Name        | Type                      | Description                                     |
| ----------- | ------------------------- | ----------------------------------------------- |
| `func`      | `Func`                    | Block to run                                    |
| `chomp`     | [`Bool`](../std/bool.md)? | Strip one trailing line ending (default `true`) |
| `can_style` | [`Bool`](../std/bool.md)? | Keep ANSI styling (default `false`)             |
| `...`       |                           | Additional arguments passed to `func`           |

#### Returns

[`Str`](../std/str.md)

#### Example

```
let greeting = term.sub do greet Alice
assert_eq $greeting "Hello, Alice!"
```

### `text :...options ...args`

Constructs terminal text from concatenated values, styled or not. This is the
general entry point for [`Text`](./text.md): styling is optional, and a plain
`text` value is still what measures itself in terminal cells.

For a reusable style with no text of its own, construct a
[`Style`](./style.md) directly.

#### Parameters

| Name      | Type | Description                         |
| --------- | ---- | ----------------------------------- |
| `...args` | *    | Values converted to display strings |

Also accepts the module's [style options](#style-options). `:INHERIT:` leaves
a setting to the surrounding style.

#### Returns

[`Text`](./text.md)

#### Example

```
let warning = text Warning fg: :YELLOW: bold: true
echo $warning

# Unstyled, for its measurement and layout methods.
let column = text $name
echo $column.clip(20, suffix: "…")
```
