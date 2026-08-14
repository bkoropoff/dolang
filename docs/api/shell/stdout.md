# `Stdout`

Handle for the process's standard output, obtained as
[`shell.stdout`](./index.md#stdout).

## Methods

### `write data`

Writes bytes verbatim and reports how many were written.

#### Parameters

| Name   | Type                                           | Description    |
| ------ | ---------------------------------------------- | -------------- |
| `data` | [`Str`](../std/str.md)\|[`Bin`](../std/bin.md) | Bytes to write |

#### Returns

[`Int`](../std/int.md) — bytes written. For a `Str`, this is the
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
of [`strand.put`](../strand/index.md). A value contributes exactly its own bytes
and nothing else — no line ending is appended and none is translated. `put`
differs from `write` only in stringifying values that are neither a `Str` nor a
`Bin` rather than rejecting them, so `put 42` writes `42` the way `echo` would.

To terminate values, say so with
[`precrimp`](../std/sink.md#precrimp-terminator):

```
shell.stdout.put "hello"          # writes "hello"

let lines = shell.stdout.precrimp()
lines.put "hello"                 # writes "hello\n"
```
