# `Ace`

Immutable POSIX.1e access-control entry.

## Class Methods

### `user_obj permissions?`

Constructs the file-owner entry.

### `user id permissions?`

Constructs a named-user entry.

#### Parameters

| Name | Type                      | Description |
| ---- | ------------------------- | ----------- |
| `id` | [`Int`](../../std/int.md) | User ID     |

### `group_obj permissions?`

Constructs the file-group entry.

### `group id permissions?`

Constructs a named-group entry.

#### Parameters

| Name | Type                      | Description |
| ---- | ------------------------- | ----------- |
| `id` | [`Int`](../../std/int.md) | Group ID    |

### `mask permissions?`

Constructs the effective-rights mask entry.

### `other permissions?`

Constructs the entry for all other users.

Every constructor takes an optional trailing
[`Permission`](./permission.md) argument, defaulting to empty.

## Fields

### `type`

Entry qualifier: `:USER_OBJ:`, `:USER:`, `:GROUP_OBJ:`, `:GROUP:`, `:MASK:`,
or `:OTHER:`.

### `id`

Numeric user or group ID.

Raises `FieldError` unless `type` is `:USER:` or `:GROUP:`.

### `permissions`

The entry's permissions, as a [`Permission`](./permission.md).
