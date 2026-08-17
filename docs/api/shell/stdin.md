# `Stdin`

Handle for the process's standard input, obtained as
[`shell.stdin`](./index.md#stdin).

## Methods

### `read size?`

Reads raw bytes.

#### Parameters

| Name   | Type                    | Description                                   |
| ------ | ----------------------- | --------------------------------------------- |
| `size` | [`Int`](../std/int.md)? | Maximum bytes to read; unbounded when omitted |

#### Returns

[`Bin`](../std/bin.md). Empty means end of stream.

With a `size`, this is a single read and may return fewer bytes than requested,
as [`fs.File.read`](../fs/file.md) does. Without one, it reads to end of stream.

#### Example

```
let rest = shell.stdin.read()
let head = shell.stdin.read 64
```

### `lines()`

Returns a `Stdin` that yields lines. This is the default, so it is only needed
to undo a previous `chunks()`.

#### Returns

`Stdin`.

### `chunks()`

Returns a `Stdin` that yields arbitrary-sized [`Bin`](../std/bin.md) chunks.

#### Returns

`Stdin`.

#### Example

```
for chunk = shell.stdin.chunks()
  hasher.update $chunk
```

## Operators

### Iteration

`Stdin` is an [iterator](../std/iter.md). Iteration yields lines by default;
`lines()` and `chunks()` select the kind of value it yields.
A line **includes its terminator**, so [`chomp`](../std/iter.md#chomp) is how
you ask for one without:

```
for line = shell.stdin.chomp()
  if (line == "--")
    break
let body = shell.stdin.read()
```

All `Stdin` handles read through one shared buffered reader, so mixing
iteration with `read` picks up exactly where the other left off.
