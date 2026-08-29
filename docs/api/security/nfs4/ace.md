# `Ace`

Immutable NFSv4 access-control entry.

## Class Methods

### `owner type: mask: :flags?`

Constructs the `OWNER@` entry.

### `owning_group type: mask: :flags?`

Constructs the `GROUP@` entry.

### `everyone type: mask: :flags?`

Constructs the `EVERYONE@` entry.

### `user id type: mask: :flags?`

Constructs a named-user entry.

#### Parameters

| Name | Type                      | Description |
| ---- | ------------------------- | ----------- |
| `id` | [`Int`](../../std/int.md) | User ID     |

### `group id type: mask: :flags?`

Constructs a named-group entry.

#### Parameters

| Name | Type                      | Description |
| ---- | ------------------------- | ----------- |
| `id` | [`Int`](../../std/int.md) | Group ID    |

Every constructor takes `type:` (`:ALLOW:`, `:DENY:`, `:AUDIT:`, or
`:ALARM:`) and `mask:` (a [`Mask`](./mask.md)) as required key
arguments, and `flags:` (a [`Flags`](./flags.md)) as an optional one,
defaulting to empty.

Capitalized class methods require built [`Mask`](./mask.md) and
[`Flags`](./flags.md) values. Use
[`security.nfs4.ace`](./index.md#ace-allow-deny-audit-alarm-mask-flags) for
symbolic mask and flag coercion.

#### Example

```
let read = nfs4.Mask(:READ_DATA:, :READ_ATTRIBUTES:)
nfs4.Ace.user 1000 type: :ALLOW: mask: $read flags: (nfs4.Flags(:FILE_INHERIT:))
```

## Fields

### `type`

Entry type: `:ALLOW:`, `:DENY:`, `:AUDIT:`, or `:ALARM:`.

### `principal`

Entry qualifier: `:OWNER:`, `:OWNING_GROUP:`, `:EVERYONE:`, `:USER:`, or
`:GROUP:`.

### `id`

Numeric user or group ID.

Raises `FieldError` unless `principal` is `:USER:` or `:GROUP:`.

### `mask`

The entry's permission mask, as a [`Mask`](./mask.md).

### `flags`

The entry's inheritance/audit flags, as a [`Flags`](./flags.md).
