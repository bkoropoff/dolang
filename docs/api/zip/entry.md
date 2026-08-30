# Entry

A single archive entry's metadata, with a method to open it for reading.

Entries are only available for archives opened in read mode, via
[`Archive.entries`](./archive.md#entries).

## Fields

### `comment`

Entry comment.

### `compressed_size`

Compressed size in bytes.

### `compression`

Compression method as `:STORED:`, `:DEFLATE:`, `:ZSTD:`, or `:UNKNOWN:`.

### `crc32`

CRC-32 checksum of the uncompressed data.

### `last_modified`

Last modification time as [`DateTime`](../time/datetime.md), or `nil` when
the stored timestamp cannot be represented.

### `mode`

Unix permission bits, or `nil` when the archive does not provide Unix
metadata.

### `name`

Entry name as an [`fs.unix.Path`](../fs/unix/path.md).

### `size`

Uncompressed size in bytes.

### `type`

Entry type as `:FILE:`, `:DIR:`, `:SYMLINK:`, `:FIFO:`, `:CHAR_DEVICE:`,
`:BLOCK_DEVICE:`, or `:UNKNOWN:`.

## Methods

### `open block?`

Opens the entry for reading.

#### Parameters

| Name    | Type   | Description                                          |
| ------- | ------ | ---------------------------------------------------- |
| `block` | `Func` | Function to run with the file; auto-closes when done |

#### Returns

[`File`](./file.md) when no block is provided, otherwise the
result of calling `block`.

#### Errors

- Raises a concurrency error if another file is already open in the archive.

#### Example

```
for entry = archive.entries
  entry.open do |file|
    echo (str (file.read entry.size))
```
