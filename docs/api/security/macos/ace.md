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
keyword argument, and `flags:` (a [`Flags`](./flags.md)) as an optional
one, defaulting to empty.

Unlike NFSv4 or POSIX.1e ACL entries, macOS resolves every principal (owning
user, owning group, well-known accounts, or an arbitrary user/group) to a
UUID before it reaches the file's ACL, so there is no separate qualifier
enum: `principal` is always a [`uuid.Uuid`](../../uuid/uuid.md).

```
let read = macos.Mask(:READ_DATA:, :READ_ATTRIBUTES:)
macos.Ace.allow $owner mask: $read flags: (macos.Flags(:FILE_INHERIT:))
```

## Fields

### `type`

Entry type: `:ALLOW:` or `:DENY:`.

### `principal`

The entry's principal, as a [`uuid.Uuid`](../../uuid/uuid.md).

### `mask`

The entry's permission mask, as a [`Mask`](./mask.md).

### `flags`

The entry's inheritance flags, as a [`Flags`](./flags.md).
