# `Sid`

Windows security identifier.

## Constructor

### `Sid value`

Constructs a SID from its canonical string or native binary representation.

#### Parameters

| Name    | Type                                                 | Description        |
| ------- | ---------------------------------------------------- | ------------------ |
| `value` | [`Str`](../../std/str.md)\|[`Bin`](../../std/bin.md) | SID representation |

## Fields

### `revision`

SID revision number.

### `identifier_authority`

The identifier authority as a symbol when it is well known, or its 48-bit
integer value otherwise.

| Symbol               | Authority           |
| -------------------- | ------------------- |
| `:NULL:`             | Null                |
| `:WORLD:`            | World               |
| `:LOCAL:`            | Local               |
| `:CREATOR:`          | Creator             |
| `:NON_UNIQUE:`       | Non-unique          |
| `:NT:`               | NT                  |
| `:RESOURCE_MANAGER:` | Resource manager    |
| `:APP_PACKAGE:`      | Application package |
| `:MANDATORY_LABEL:`  | Mandatory label     |
| `:SCOPED_POLICY:`    | Scoped policy ID    |
| `:AUTHENTICATION:`   | Authentication      |
| `:PROCESS_TRUST:`    | Process trust       |

### `sub_authority_count`

Number of sub-authorities.

### `sub_authorities`

Sub-authorities as an immutable [`Tuple`](../../std/tuple.md).

## Methods

### `lookup()`

Resolves the SID in the active Windows VFS target.

#### Returns

[`SidName`](./sidname.md)

#### Errors

- Raises [`sys.NotFoundError`](../../sys/not-found-error.md) when the SID is
  unmapped.
- Raises `UnsupportedError` for Unix targets.

### `to_bin()`

Returns the native Windows packet representation.

#### Returns

[`Bin`](../../std/bin.md)

#### Example

```
let sid = Sid S-1-5-32-544
echo $sid.identifier_authority
let encoded = sid.to_bin()
```
