# `Stdout`

Handle for the process's standard output, obtained as
[`shell.stdout`](./index.md#stdout).

## Methods

### `write data`

Writes bytes verbatim and reports how many were written.

**Parameters:**

| Name   | Type                                           | Description    |
| ------ | ---------------------------------------------- | -------------- |
| `data` | [`Str`](../std/str.md)\|[`Bin`](../std/bin.md) | Bytes to write |

**Returns:** [`Int`](../std/int.md) — bytes written. For a `Str`, this is the
UTF-8 byte count, not the character count.

**Errors:**

- Raises [`TypeError`](../std/type-error.md) for anything other than a `Str` or
  `Bin`.

```
shell.stdout.write "no newline"
shell.stdout.write b"\x00\x01\xff"
assert_eq (shell.stdout.write "héllo") 6
```

### `flush()`

Flushes buffered output to the stream.

Worth calling when interleaving with unbuffered console output, whose ordering
relative to buffered stdout is otherwise not guaranteed. The interpreter flushes
on exit.

## Operators

### Sink

`Stdout` is a [sink](../std/sink.md), so it can be the target of a pipeline or
of [`strand.put`](../strand/index.md). Unlike `write`, values put into it are
framed per the ambient [I/O mode](./index.md#with_io_mode-mode-func): in
`:LINE:` mode each value is written with a trailing line ending, and `Bin`
values are always written verbatim.

```
shell.stdout.put "hello"
```
