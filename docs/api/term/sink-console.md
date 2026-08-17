# `SinkConsole`

A [`Console`](./console.md) over an ordinary [sink](../std/sink.md), supplying
the rest of the console interface.

[`capture`](./index.md#capture-console-func-args-mode) wraps a plain sink in one
of these automatically.

## Output mode

The `mode:` given at construction fixes how written data is divided into
values for the console's lifetime. In either mode, the values concatenate back
to exactly the bytes that were written.

- `:LINE:` — one [`Str`](../std/str.md) per complete line, **terminator
  included**. The default.
- `:CHUNK:` — arbitrary [`Bin`](../std/bin.md) chunks. Chunk boundaries are
  unspecified.

```
let lines = []
term.capture $lines do echo hello
assert_eq $lines ["hello\n"]

let chomped = []
term.capture (chomped.prechomp()) do echo hello
assert_eq $chomped ["hello"]

let chunks = []
term.capture $chunks mode: :CHUNK: do echo hello
# [b"hello\n"]
```

Nothing is translated on the way through: a line ending that arrives is the
line ending that is stored.

## Constructor

### `SinkConsole sink :can_style? :mode?`

Wraps `sink`.

#### Parameters

| Name        | Type                     | Description                        |
| ----------- | ------------------------ | ---------------------------------- |
| `sink`      | [`Sink`](../std/sink.md) | Where output values are written    |
| `can_style` | `bool?`                  | Emit ANSI styling (default false)  |
| `mode`      | [`sym`](../std/sym.md)?  | `:LINE:` (default) or `:CHUNK:`    |

#### Example

```
# Style is off by default
let plain = []
term.capture $plain do echo $warning

# Opt in to SGR styling
let styled = []
term.capture (term.SinkConsole styled can_style: true) do echo $warning
```

## Methods

Implements the [`Console`](./console.md) interface: `write` and `flush`.
`flush` emits any partial final line, which is what makes an unterminated
`print` visible when a capture scope ends.

[`line_ending`](./console.md#line_ending) is the interpreter host's, chosen
arbitrarily — a sink has no platform of its own.

[`is_tty`](./console.md#is_tty) is always `false` and
[`geometry()`](./console.md#geometry) is `nil` — a sink is never a terminal.
