# `Stdin`

Handle for the process's standard input, obtained as
[`shell.stdin`](./index.md#stdin).

## Methods

### `read size?`

Reads raw bytes.

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

```
for line = shell.stdin
  if (line == "--")
    break
let body = shell.stdin.read()
```
