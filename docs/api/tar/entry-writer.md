# EntryWriter

Writes content to one scoped TAR entry.

## Methods

### `write data`

Writes raw string or binary bytes.

**Parameters:**

| Name   | Type                                           | Description    |
| ------ | ---------------------------------------------- | -------------- |
| `data` | [`Str`](../std/str.md)\|[`Bin`](../std/bin.md) | Bytes to write |

**Returns:** `nil`.

```
entry.write header
entry.write b"\x00\x01"
```

## Operators

The sink protocol accepts `Str` and `Bin` values without adding separators.

```
entry.put "first"
entry.put b"second"
```
