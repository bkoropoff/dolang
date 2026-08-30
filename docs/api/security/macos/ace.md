# `Ace`

Immutable macOS extended access-control entry.

## Class Methods

### `allow principal mask: :flags?`

Constructs an entry that grants the permissions in `mask`.

### `deny principal mask: :flags?`

Constructs an entry that denies the permissions in `mask`.

#### Parameters

| Name        | Type                              | Description           |
| ----------- | --------------------------------- | --------------------- |
| `principal` | [`uuid.Uuid`](../../uuid/uuid.md) | The entry's principal |

Every constructor takes `mask:` (a [`Mask`](./mask.md)) as a required
key argument, and `flags:` (a [`Flags`](./flags.md)) as an optional
one, defaulting to empty.

Capitalized class methods require a built `uuid.Uuid`, [`Mask`](./mask.md),
and [`Flags`](./flags.md). Use
[`security.macos.ace`](./index.md#ace-allow-deny-mask-flags) for UUID string or
binary values and symbolic mask or flag coercion.

Unlike NFSv4 or POSIX.1e ACL entries, macOS resolves every principal (owning
user, owning group, well-known accounts, or an arbitrary user/group) to a
UUID before it reaches the file's ACL, so there is no separate qualifier
enum: `principal` is always a [`uuid.Uuid`](../../uuid/uuid.md).

#### Example

```
let read = macos.Mask(:READ_DATA:, :READ_ATTRIBUTES:)
macos.Ace.allow $owner mask: $read flags: (macos.Flags(:FILE_INHERIT:))
```

## Fields

### `flags`

The entry's inheritance flags, as a [`Flags`](./flags.md).

### `mask`

The entry's permission mask, as a [`Mask`](./mask.md).

### `principal`

The entry's principal, as a [`uuid.Uuid`](../../uuid/uuid.md).

### `type`

Entry type: `:ALLOW:` or `:DENY:`.
