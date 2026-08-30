# `Stderr`

Handle for the process's standard error, obtained as
[`shell.stderr`](./index.md#stderr).

Stateless and interchangeable, as [`Stdout`](./stdout.md) is.

Unlike [`term.console`](../term/console.md), this bypasses extensions that have
taken over the terminal shell-wide — it is the error stream itself.

## Methods

### `flush()`

Flushes buffered output to the stream.

### `write data`

Writes bytes verbatim and reports how many were written. Identical in behavior
to [`Stdout.write`](./stdout.md#write-data).

```
shell.stderr.write "fatal: no such target\n"
```

## Operators

### Sink

`Stderr` is a [sink](../std/sink.md). Values put into it are written verbatim,
as with [`Stdout`](./stdout.md#sink).
