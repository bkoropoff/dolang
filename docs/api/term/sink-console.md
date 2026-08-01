# `SinkConsole`

A [`Console`](./console.md) over an ordinary [sink](../std/sink.md), supplying
the rest of the console interface.

[`capture`](./index.md#capture-console-func-args) wraps a plain sink in one of
these automatically.

## Framing

The current [I/O mode](../shell/index.md#with_io_mode-mode-func) governs how
written data is divided into values.

- `:LINE:` — one [`Str`](../std/str.md) per complete line, with the ending
  stripped (LF or CRLF).
- `:CHUNK:` — arbitrary [`Bin`](../std/bin.md) chunks, line endings left in
  place. Chunk boundaries are unspecified.

```
let lines = []
term.capture $lines do echo hello
assert_eq $lines ["hello"]

let chunks = []
with_io_mode :CHUNK: do term.capture $chunks do echo hello
# [b"hello\n"] — the terminator is still there
```

The line ending is arbitarily chosen to be the interpreter's host platform's.

## Constructor

### `SinkConsole sink :can_style?`

Wraps `sink`.

**Parameters:**

| Name        | Type                     | Description                       |
| ----------- | ------------------------ | --------------------------------- |
| `sink`      | [`Sink`](../std/sink.md) | Where framed values are written   |
| `can_style` | `bool?`                  | Emit ANSI styling (default false) |

```
# Style is off by default
let plain = []
term.capture $plain do echo $warning

# Opt in to SGR styling
let styled = []
term.capture (term.SinkConsole styled can_style: true) do echo $warning
```

## Methods

Implements the [`Console`](./console.md) interface: `write`, `writeln`, and
`flush`. `flush` emits any partial final line, which is what makes an
unterminated `print` visible when a capture scope ends.

[`geometry()`](./console.md#geometry) is `nil` — a sink has no layout.
