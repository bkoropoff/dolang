# `Ace`

Immutable view of a native Windows access-control entry.

The constructor takes the same fields as an [ACE spec](./index.md#ace-specs). An
entry can also be written directly as a spec wherever one is accepted, including
[`acl`](./index.md#acl-aces-revision) and the `dacl:` and `sacl:` options of
[`sec_desc`](./index.md#sec_desc-desc-options).

## Constructor

### `Ace :allow? :deny? :audit? :mask ...options`

Constructs an access-allowed, access-denied, or system-audit ACE. Exactly one
of `allow`, `deny`, or `audit` names the trustee.

#### Parameters

| Name                    | Type                                                        | Description                        |
| ----------------------- | ----------------------------------------------------------- | ---------------------------------- |
| `allow`                 | [`Sid`](./sid.md)?                                          | Trustee for an access-allowed ACE  |
| `deny`                  | [`Sid`](./sid.md)?                                          | Trustee for an access-denied ACE   |
| `audit`                 | [`Sid`](./sid.md)?                                          | Trustee for a system-audit ACE     |
| `mask`                  | [`AccessMask`](./access-mask.md)                            | Access mask                        |
| `flags`                 | [`AceFlags`](./ace-flags.md)?                               | ACE header flags                   |
| `object_type`           | [`uuid.Guid`](../../uuid/guid.md)?                          | Object type                        |
| `inherited_object_type` | [`uuid.Guid`](../../uuid/guid.md)?                          | Inherited object type              |
| `callback`              | [`Bool`](../../std/bool.md)?                                | Build a callback ACE               |
| `application_data`      | [`Bin`](../../std/bin.md)?                                  | Trailing application data          |
| `successful`            | [`Bool`](../../std/bool.md)?                                | Audit successful access            |
| `failed`                | [`Bool`](../../std/bool.md)?                                | Audit failed access                |

#### Returns

`Ace`

The constructor is strict: trustees must already be `Sid` values, `mask` must
be an `AccessMask`, `flags` must be `AceFlags`, and object types must be
`uuid.Guid` values. Use the lowercase
[`ace`](./index.md#ace-allow-deny-audit-mask-options) coercion function for
declarative values such as SID strings and symbolic flags.
Application data is zero-padded to DWORD (32-bit) alignment.

#### Errors

| Exception    | Condition                                                                |
| ------------ | ------------------------------------------------------------------------ |
| `ValueError` | Zero or multiple trustee arguments are present                           |
| `ValueError` | An audit has no outcome, both outcomes are false, or flags name outcomes |
| `ValueError` | `successful` or `failed` is supplied for an allow or deny ACE            |

#### Example

```
let entry = Ace
  allow: (Sid :EVERYONE:)
  mask: (AccessMask :GENERIC_READ:)
```

## Fields

### `type`

Symbolic native ACE type, or `:UNKNOWN:` for an unrecognized type code.

For recognized values, see [ACE type values](./index.md#ace-type-values).

### `type_code`

Native numeric ACE type code.

### `flags`

[`AceFlags`](./ace-flags.md) from the ACE header.

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
