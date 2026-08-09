# `Guid`

Windows globally unique identifier.

## Constructor

### `Guid value`

Parses a canonical GUID string or native Windows GUID packet.

#### Parameters

| Name    | Type                                                 | Description                        |
| ------- | ---------------------------------------------------- | ---------------------------------- |
| `value` | [`Str`](../../std/str.md)\|[`Bin`](../../std/bin.md) | GUID text or 16-byte native packet |

#### Errors

- Raises `ValueError` when the text or packet is malformed.

#### Example

```
let id = Guid 00112233-4455-6677-8899-aabbccddeeff
echo $id
```

## Methods

### `to_bin()`

Returns the 16-byte native Windows GUID representation.

#### Returns

[`Bin`](../../std/bin.md)

## Operators

GUIDs support equality and hashing.
