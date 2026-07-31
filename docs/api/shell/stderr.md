# `Stderr`

Handle for the process's standard error, obtained as
[`shell.stderr`](./index.md#stderr).

Stateless and interchangeable, as [`Stdout`](./stdout.md) is.

Unlike [`term.console`](../term/console.md), this bypasses extensions that have
taken over the terminal shell-wide — it is the error stream itself.

## Methods

### `write data`

Writes bytes verbatim and reports how many were written. Identical in behavior
to [`Stdout.write`](./stdout.md#write-data).

```
shell.stderr.write "fatal: no such target\n"
```

### `flush()`

Flushes buffered output to the stream.

## Operators

### Sink

`Stderr` is a [sink](../std/sink.md). Values put into it are framed per the
ambient [I/O mode](./index.md#with_io_mode-mode-func), as with
[`Stdout`](./stdout.md#sink).
