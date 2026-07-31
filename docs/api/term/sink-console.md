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

### `SinkConsole()`

Constructs an empty adapter. The downstream sink is normally supplied by
`capture`; construct one directly only when subclassing.

## Methods

Implements the [`Console`](./console.md) interface: `write`, `writeln`, and
`flush`. `flush` emits any partial final line, which is what makes an
unterminated `print` visible when a capture scope ends.
