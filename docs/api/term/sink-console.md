# `SinkConsole`

A [`Console`](./console.md) over an ordinary [sink](../std/sink.md), supplying
the rest of the console interface.

[`capture`](./index.md#capture-console-func-args) wraps a plain sink in one of
these automatically, so callers pass an array or a pipeline end and never name
this type.

## Framing

This is a bytestream-to-value boundary, and that conversion is exactly what the
[I/O mode](../shell/index.md#with_io_mode-mode-func) governs — the same way an
external process's output is framed when it crosses into a sink.

- `:LINE:` — one [`Str`](../std/str.md) per complete line, with the ending
  stripped (LF or CRLF).
- `:CHUNK:` — arbitrary [`Bin`](../std/bin.md) chunks, line endings left in
  place. Chunk boundaries are unspecified.

`writeln` materializes its line ending into the byte stream *first*, so the
terminator survives either mode rather than depending on one.

```
let lines = []
term.capture $lines do echo hello
assert_eq $lines ["hello"]

let chunks = []
with_io_mode :CHUNK: do term.capture $chunks do echo hello
# [b"hello\n"] — the terminator is still there
```

Its line ending is the host platform's. A console has no VFS target to consult,
so it has to pick one.

## Constructor

### `SinkConsole sink :can_style?`

Wraps `sink`. [`capture`](./index.md#capture-console-func-args) does this for
you; construct one directly to pass options.

**Parameters:**

| Name        | Type                     | Description                       |
| ----------- | ------------------------ | --------------------------------- |
| `sink`      | [`Sink`](../std/sink.md) | Where framed values are written   |
| `can_style` | `bool?`                  | Emit ANSI styling (default false) |

**Returns:** `SinkConsole`

```
# Off by default, so assertions compare against plain text.
let plain = []
term.capture $plain do echo $warning

# Opt in when the test is specifically about styling.
let styled = []
term.capture (term.SinkConsole(styled, can_style: true)) do echo $warning
```

## Methods

Implements the [`Console`](./console.md) interface: `write`, `writeln`, and
`flush`. `flush` emits any partial final line, which is what makes an
unterminated `print` visible when a capture scope ends.

[`geometry()`](./console.md#geometry) is `nil` — a sink has no layout.
