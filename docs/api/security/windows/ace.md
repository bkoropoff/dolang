# `Ace`

Immutable view of a native Windows access-control entry.

## Class Methods

### `allow sid mask ...options`

Constructs an access-allowed ACE.

#### Parameters

| Name                    | Type                                                        | Description               |
| ----------------------- | ----------------------------------------------------------- | ------------------------- |
| `sid`                   | [`Sid`](./sid.md)                                           | Trustee                   |
| `mask`                  | [`AccessMask`](./access-mask.md)\|[`Int`](../../std/int.md) | Access mask               |
| `flags`                 | [`Int`](../../std/int.md)?                                  | Native ACE flags          |
| `object_type`           | [`uuid.Guid`](../../uuid/guid.md)?                          | Object type               |
| `inherited_object_type` | [`uuid.Guid`](../../uuid/guid.md)?                          | Inherited object type     |
| `callback`              | [`Bool`](../../std/bool.md)?                                | Build a callback ACE      |
| `application_data`      | [`Bin`](../../std/bin.md)?                                  | Trailing application data |

#### Returns

`Ace`

Application data is zero-padded to DWORD (32-bit) alignment.

### `deny sid mask ...options`

Constructs an access-denied ACE. Parameters match
[`allow`](#allow-sid-mask-options).

#### Returns

`Ace`

### `audit sid mask :successful :failed ...options`

Constructs a system-audit ACE.

#### Parameters

| Name         | Type                                                        | Description             |
| ------------ | ----------------------------------------------------------- | ----------------------- |
| `sid`        | [`Sid`](./sid.md)                                           | Trustee                 |
| `mask`       | [`AccessMask`](./access-mask.md)\|[`Int`](../../std/int.md) | Access mask             |
| `successful` | [`Bool`](../../std/bool.md)                                 | Audit successful access |
| `failed`     | [`Bool`](../../std/bool.md)                                 | Audit failed access     |

The remaining optional parameters match
[`allow`](#allow-sid-mask-options).

#### Returns

`Ace`

#### Errors

- Raises `ValueError` when both outcomes are false or `flags` contains audit
  outcome bits.

## Fields

### `type`

Symbolic native ACE type, or `:UNKNOWN:` for an unrecognized type code.

For recognized values, see [ACE type values](./index.md#ace-type-values).

### `type_code`

Native numeric ACE type code.

### `flags`

Native ACE flags byte.

### `size`

Declared ACE packet size.

### `mask`

[`AccessMask`](./access-mask.md).

Raises `FieldError` for an ACE layout without a projected mask.

### `sid`

Trustee [`Sid`](./sid.md).

Raises `FieldError` for an ACE layout without a projected SID.

### `object_flags`

Native object ACE flags.

Raises `FieldError` for a non-object ACE.

### `object_type`

Object-type [`uuid.Guid`](../../uuid/guid.md), or `nil` when the
object flag is clear.

Raises `FieldError` for a non-object ACE.

### `inherited_object_type`

Inherited-object-type [`uuid.Guid`](../../uuid/guid.md), or `nil`
when the object flag is clear.

Raises `FieldError` for a non-object ACE.

### `application_data`

Exact bytes after the projected SID. The value can be empty.

Raises `FieldError` when the ACE body is not interpreted.

### `object_inherit`

Whether non-container child objects inherit this ACE.

### `container_inherit`

Whether container child objects inherit this ACE.

### `no_propagate_inherit`

Whether inherited copies stop propagating after one generation.

### `inherit_only`

Whether this ACE applies only through inheritance.

### `inherited`

Whether this ACE was inherited.

### `critical`

Whether the native critical flag is set.

### `successful_access`

Whether an audit or alarm ACE selects successful access.

Raises `FieldError` for other ACE types.

### `failed_access`

Whether an audit or alarm ACE selects failed access.

Raises `FieldError` for other ACE types.

### `trust_protected_filter`

Whether an access-filter ACE has the trust-protected flag.

Raises `FieldError` for other ACE types.

## Methods

### `to_bin()`

Returns the exact native ACE packet, including application or unknown data.

#### Returns

[`Bin`](../../std/bin.md)
