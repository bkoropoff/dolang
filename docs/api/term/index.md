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

### `console`

The host [`Console`](./console.md), which may be taken over by an extension
such a `progress.`

Unlike [`output()`](#output), it is not intercepted by an enclosing
[`capture`](#capture-console-func-args).

### `default`

A [`Console`](./console.md) that forwards every operation to whatever
[`output()`](#output) currently resolves to, resolved fresh on each call
rather than once. Bound as the main strand's implicit output when stdout is a
terminal, so unnamed program output keeps following capture and `progress`
takeover for the life of the process — see
[Terminal output](../../shell/terminal-output.md#child-process-output).

## Functions

### `output()`

Returns the current output console: the one installed by an enclosing
[`capture`](#capture-console-func-args), or [`console`](#console) if there is
none. This is where `echo`, `print`, diagnostics, and unredirected child
process output go.

#### Returns

[`Console`](./console.md)

### `capture console func ...args`

Runs a callable with `console` installed as the ambient console, then flushes
it and restores the previous one.

`console` may be any [`Console`](./console.md), or any
[`Sink`](../std/sink.md) — a plain sink is wrapped in a
[`SinkConsole`](./sink-console.md), which frames per the ambient
[I/O mode](../shell/index.md#with_io_mode-mode-func).

The override is inherited by all strands spawned inside the call.

#### Parameters

| Name      | Type                            | Description                           |
| --------- | ------------------------------- | ------------------------------------- |
| `console` | [`Console`](./console.md)\|sink | Destination to install                |
| `func`    | callable                        | Block to run                          |
| `...`     |                                 | Additional arguments passed to `func` |

#### Returns

Return value of `func`.

```
let lines = []
term.capture $lines do
  echo "Hello, Alice!"
assert_eq $lines ["Hello, Alice!"]
```

The scope always ends with a flush, so an unterminated `print` still arrives:

```
let out = []
term.capture $out do print hi
assert_eq $out ["hi"]
```

### `sub func :trim? :can_style? ...args`

Runs a callable and returns its console output as a string. The console
counterpart to [`proc.sub`](../proc/index.md#sub-func-trim), which captures a
strand's implicit output stream.

Verbatim output is captured, which must be valid UTF-8. One final line ending
(LF or CRLF) is removed unless `trim: false`.

#### Parameters

| Name        | Type                      | Description                                     |
| ----------- | ------------------------- | ----------------------------------------------- |
| `func`      | callable                  | Block to run                                    |
| `trim`      | [`Bool`](../std/bool.md)? | Strip one trailing line ending (default `true`) |
| `can_style` | [`Bool`](../std/bool.md)? | Keep ANSI styling (default `false`)             |
| `...`       |                           | Additional arguments passed to `func`           |

#### Returns

[`Str`](../std/str.md)

```
let greeting = term.sub do greet Alice
assert_eq $greeting "Hello, Alice!"
```

### `mute func ...args`

Runs a function with default console-bound output silenced: `echo`, `print`,
unredirected program `stderr`, etc.

#### Parameters

| Name      | Type     | Description                           |
| --------- | -------- | ------------------------------------- |
| `func`    | callable | Block to run                          |
| `...args` |          | Additional arguments passed to `func` |

#### Returns

Return value of `func`.

```
# Nothing from this reaches the terminal.
mute do run printf "this will not be printed"
```

### `echo ...args`

Prints arguments separated by spaces, followed by a newline. Ordinary values
are sanitized; direct [`Text`](./text.md) arguments retain their styling.

#### Parameters

| Name      | Type | Description                                    |
| --------- | ---- | ---------------------------------------------- |
| `...args` | *    | Values converted with `arg` and written safely |

#### Returns

`nil`

```
echo status: ready count: 3
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

#### Returns

`nil`

```
print "status: " ready fg: :GREEN: bold: true
```

### `style :...options ...args`

Constructs styled terminal text from concatenated values. With no positional
arguments, it returns a reusable [`Style`](./style.md) instead.

#### Parameters

| Name      | Type | Description                         |
| --------- | ---- | ----------------------------------- |
| `...args` | *    | Values converted to display strings |

Also accepts the module's [style options](#style-options). `:INHERIT:` leaves
a setting to the surrounding style. This is normally the default, but clears
a saved setting when deriving a [`Style`](./style.md).

#### Returns

[`Text`](./text.md) when positional arguments are provided;
otherwise [`Style`](./style.md)

```
let warning = style Warning fg: :YELLOW: bold: true
echo $warning
```

### `preformat text`

Validates existing ANSI-styled text. SGR styling is canonicalized; other
terminal controls, including hyperlinks, are removed.

#### Parameters

| Name   | Type                   | Description              |
| ------ | ---------------------- | ------------------------ |
| `text` | [`Str`](../std/str.md) | ANSI-formatted input     |

#### Returns

[`Text`](./text.md)

```
let formatted = preformat input
echo $formatted
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
