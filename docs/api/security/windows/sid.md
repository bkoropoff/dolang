# `Sid`

Windows security identifier.

## Constructor

### `Sid value`

Constructs a SID from its canonical string or native binary representation.

**Parameters:**

| Name    | Type                                                 | Description        |
| ------- | ---------------------------------------------------- | ------------------ |
| `value` | [`Str`](../../std/str.md)\|[`Bin`](../../std/bin.md) | SID representation |

## Fields

### `revision`

SID revision number.

### `identifier_authority`

The 48-bit identifier authority as an integer.

### `sub_authority_count`

Number of sub-authorities.

### `sub_authorities`

Sub-authorities as an immutable [`Tuple`](../../std/tuple.md).

## Methods

### `lookup()`

Resolves the SID in the active Windows VFS target.

**Returns:** [`SidName`](./sidname.md)

**Errors:**

- Raises [`sys.NotFoundError`](../../sys/not-found-error.md) when the SID is
  unmapped.
- Raises `UnsupportedError` for Unix targets.

### `to_bin()`

Returns the native Windows packet representation.

**Returns:** [`Bin`](../../std/bin.md)

```
let sid = Sid S-1-5-32-544
echo $sid.identifier_authority
let encoded = sid.to_bin()
```
