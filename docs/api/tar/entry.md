# Entry

Exposes metadata and streaming content for the current TAR entry.

The handle remains valid only until its parent [`Reader`](./reader.md)
advances or leaves scope.

## Fields

### `device_major`

Device major number, or `nil` when absent.

### `device_minor`

Device minor number, or `nil` when absent.

### `gid`

Numeric group ID.

### `group_name`

Group name, or `nil` when absent.

### `link_name`

Link target as an [`fs.unix.Path`](../fs/unix/path.md), or `nil` when absent.

### `mode`

Unix permission bits.

### `mtime`

Modification time as [`DateTime`](../time/datetime.md).

### `path`

Entry path as an [`fs.unix.Path`](../fs/unix/path.md).

### `size`

Declared content size in bytes.

### `type`

Entry type as `:FILE:`, `:DIR:`, `:HARDLINK:`, `:SYMLINK:`, `:FIFO:`,
`:CHAR_DEVICE:`, `:BLOCK_DEVICE:`, `:CONTIGUOUS:`, or `:UNKNOWN:`.

### `uid`

Numeric owner ID.

### `user_name`

Owner name, or `nil` when absent.

## Methods

### `read size`

Reads up to `size` bytes from the current content position.

#### Parameters

| Name   | Type                    | Description             |
| ------ | ----------------------- | ----------------------- |
| `size` | [`Int`](../std/int.md)  | Maximum bytes to read   |

#### Returns

[`Bin`](../std/bin.md).

#### Example

```
let prefix = entry.read 512
```

## Operators

Iteration yields arbitrary-sized binary chunks until the entry is exhausted.

```
for chunk = entry
  output.put $chunk
```
