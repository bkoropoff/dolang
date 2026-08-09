# `Ace`

Immutable POSIX.1e access-control entry.

## Class Methods

### `user_obj :read? :write? :execute?`

Constructs the file-owner entry.

### `user id :read? :write? :execute?`

Constructs a named-user entry.

#### Parameters

| Name | Type                      | Description |
| ---- | ------------------------- | ----------- |
| `id` | [`Int`](../../std/int.md) | User ID     |

### `group_obj :read? :write? :execute?`

Constructs the file-group entry.

### `group id :read? :write? :execute?`

Constructs a named-group entry.

#### Parameters

| Name | Type                      | Description |
| ---- | ------------------------- | ----------- |
| `id` | [`Int`](../../std/int.md) | Group ID    |

### `mask :read? :write? :execute?`

Constructs the effective-rights mask entry.

### `other :read? :write? :execute?`

Constructs the entry for all other users.

The permission options accepted by every constructor default to `false`.

## Fields

### `type`

Entry qualifier: `:USER_OBJ:`, `:USER:`, `:GROUP_OBJ:`, `:GROUP:`, `:MASK:`,
or `:OTHER:`.

### `id`

Numeric user or group ID.

Raises `FieldError` unless `type` is `:USER:` or `:GROUP:`.

### `read`

Whether the entry grants read permission.

### `write`

Whether the entry grants write permission.

### `execute`

Whether the entry grants execute permission.
