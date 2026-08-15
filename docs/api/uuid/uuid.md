# Uuid

Parsed or generated UUID value.

## Constructor

### `Uuid value`

Parses a UUID from text or binary, or copies an existing `Uuid`.

Text is accepted in hyphenated, simple (no hyphens), braced, or URN form.
Binary values must be exactly 16 bytes.

#### Parameters

| Name    | Type                                                                | Description           |
| ------- | ------------------------------------------------------------------- | --------------------- |
| `value` | [`Str`](../std/str.md)\|[`Bin`](../std/bin.md)\|[`Uuid`](./uuid.md) | UUID to parse or copy |

#### Errors

| Exception    | Condition                                                      |
| ------------ | -------------------------------------------------------------- |
| `ValueError` | The text is not a valid UUID, or bytes are not exactly 16 long |

#### Example

```
let id = Uuid "67e55044-10b1-426f-9247-bb680e5fe0c8"
let copy = Uuid $id
```

## Class Fields

### `NIL`

The nil `Uuid`, `00000000-0000-0000-0000-000000000000`.

### `MAX`

The max `Uuid`, `ffffffff-ffff-ffff-ffff-ffffffffffff`.

## Class Methods

### `generate()`

Generates a random (version 4) UUID.

#### Returns

`Uuid`

```
let id = Uuid.generate()
assert_eq $id.version 4
assert_eq $id.variant :RFC4122:
```

## Fields

### `bytes`

The raw 16-byte binary form.

#### Returns

[`Bin`](../std/bin.md)

### `hex`

The simple (no hyphens) lowercase hexadecimal text form.

### `version`

The UUID version number, such as `4` for a randomly generated UUID.

### `variant`

The UUID variant, one of `:NCS:`, `:RFC4122:`, `:MICROSOFT:`, or `:FUTURE:`.

## Example

```
let id = Uuid "67e55044-10b1-426f-9247-bb680e5fe0c8"
assert_eq (str id) "67e55044-10b1-426f-9247-bb680e5fe0c8"
assert_eq $id.hex "67e5504410b1426f9247bb680e5fe0c8"
assert_eq $id.version 4
assert_eq $id.variant :RFC4122:
```
