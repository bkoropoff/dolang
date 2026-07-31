# `Stdin`

Handle for the process's standard input, obtained as
[`shell.stdin`](./index.md#stdin).

The handle is stateless — the buffered reader lives on the interpreter — so
every `Stdin` value reads the same stream through the same buffer. That is what
makes mixing iteration with [`read`](#read-size) safe: a second reader would
buffer ahead and swallow bytes the first was going to see.

## Methods

### `read size?`

Reads raw bytes.

No framing: nothing is split into lines and nothing is required to be valid
UTF-8. This is the escape hatch below iteration, which applies the ambient
[I/O mode](./index.md#with_io_mode-mode-func).

**Parameters:**

| Name   | Type                    | Description                                   |
| ------ | ----------------------- | --------------------------------------------- |
| `size` | [`Int`](../std/int.md)? | Maximum bytes to read; unbounded when omitted |

**Returns:** [`Bin`](../std/bin.md). Empty means end of stream.

With a `size`, this is a single read and may return fewer bytes than requested,
as [`fs.File.read`](../fs/file.md) does. Without one, it reads to end of stream.

```
let rest = shell.stdin.read()
let head = shell.stdin.read 64
```

## Operators

### Iteration

`Stdin` is an [iterator](../std/iter.md). Iteration is framed per the ambient
[I/O mode](./index.md#with_io_mode-mode-func): `:LINE:` yields
[`Str`](../std/str.md) values with the line ending removed, `:CHUNK:` yields
[`Bin`](../std/bin.md) values.

Iteration and `read` share the same buffer, so they can be interleaved:

```
for line = shell.stdin
  if (line == "--")
    break
let body = shell.stdin.read()
```
