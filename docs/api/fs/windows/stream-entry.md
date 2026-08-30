# StreamEntry

Alternate data stream entry returned by
[`fs.streams`](../index.md#streams-path-resolve).

This is only supported on Windows.

## Fields

### `alloc_size`

Allocated stream size in bytes.

### `name`

Stream name. The unnamed default stream is reported as `""`.

#### Example

```
for stream = streams "data.txt"
  echo $stream.name
```

### `size`

Logical stream size in bytes.

### `type`

Stream type without the leading `$`.

#### Example

```
for stream = streams "data.txt"
  echo $stream.type
```

## Operators

### `/`

`path / stream` returns the stream-qualified [`Path`](./path.md) for that entry.

### Example

```
for stream = streams "data.txt"
  echo (Path "data.txt" / stream)
```
