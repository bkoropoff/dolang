# Guid

Windows globally unique identifier, mirroring the native `GUID` struct
(`Data1`/`Data2`/`Data3`/`Data4`).

## Constructor

### `Guid value`

Parses a canonical hyphenated GUID string, or copies an existing `Guid` or
16-byte native packet.

#### Parameters

| Name    | Type                                                                | Description                         |
| ------- | ------------------------------------------------------------------- | ----------------------------------- |
| `value` | [`Str`](../std/str.md)\|[`Bin`](../std/bin.md)\|[`Guid`](./guid.md) | GUID text, native packet, or `Guid` |

#### Errors

| Exception    | Condition                                                          |
| ------------ | ------------------------------------------------------------------ |
| `ValueError` | The text is not a canonical GUID, or bytes are not exactly 16 long |

#### Example

```
let id = Guid "00112233-4455-6677-8899-aabbccddeeff"
let copy = Guid $id
```

## Class Fields

### `NIL`

The nil `Guid`, `00000000-0000-0000-0000-000000000000`.

## Class Methods

### `generate()`

Generates a random `Guid`.

#### Returns

`Guid`

## Fields

### `bytes`

The raw 16-byte native Windows in-memory GUID packet.

#### Returns

[`Bin`](../std/bin.md)

### `data1`

The `Data1` field.

### `data2`

The `Data2` field.

### `data3`

The `Data3` field.

### `data4`

The 8-byte `Data4` field.

#### Returns

[`Bin`](../std/bin.md)

## Example

```
let id = Guid "00112233-4455-6677-8899-aabbccddeeff"
assert_eq (str id) "00112233-4455-6677-8899-aabbccddeeff"
assert_eq $id.data1 0x00112233
assert_eq $id.data2 0x4455
assert_eq $id.data3 0x6677
assert_eq $id.data4 b"\x88\x99\xaa\xbb\xcc\xdd\xee\xff"
```
