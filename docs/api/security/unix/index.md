# security.unix

The `security.unix` module exposes Unix security types and uid/gid identity
lookups.

## Types

| Type                            | Description                        |
| ------------------------------- | ---------------------------------- |
| [`Acl`](./acl.md)               | POSIX.1e access-control list       |
| [`Ace`](./ace.md)               | POSIX.1e access-control entry      |
| [`Permission`](./permission.md) | Read/write/execute permission bits |
| [`Identity`](./identity.md)     | Unix process identity information  |

## Functions

### `id()`

Returns Unix security information captured for the active VFS context.

#### Returns

[`Identity`](./identity.md)

**Errors:**

- Raises `UnsupportedError` when the active VFS target is not Unix.

```
let info = id()
echo "uid=$info.uid euid=$info.euid"
```

### `user_name uid`

Resolves a Unix user ID in the active VFS target. On macOS, `uid` may also
be a [`uuid.Uuid`](../../uuid/uuid.md) principal, resolved to a uid first via
[`security.macos.id_for_uuid`](../macos/index.md#id_for_uuid-uuid).

#### Parameters

| Name  | Type               | Description                |
| ----- | ------------------ | -------------------------- |
| `uid` | `Int`\|`uuid.Uuid` | Unix user ID or macOS UUID |

#### Returns

[`Str`](../../std/str.md)

**Errors:**

- Raises [`sys.NotFoundError`](../../sys/not-found-error.md) when the ID is
  unknown.
- Raises `UnsupportedError` when the active VFS target is not Unix, or when
  a `uuid.Uuid` is passed and the target is not macOS.

### `user_id name`

Resolves a Unix user name in the active VFS target.

#### Returns

[`Int`](../../std/int.md)

**Errors:**

- Raises [`sys.NotFoundError`](../../sys/not-found-error.md) when the name is
  unknown.
- Raises `UnsupportedError` when the active VFS target is not Unix.

### `group_name gid`

Resolves a Unix group ID in the active VFS target. On macOS, `gid` may also
be a [`uuid.Uuid`](../../uuid/uuid.md) principal, resolved to a gid first via
[`security.macos.id_for_uuid`](../macos/index.md#id_for_uuid-uuid).

#### Parameters

| Name  | Type               | Description                 |
| ----- | ------------------ | --------------------------- |
| `gid` | `Int`\|`uuid.Uuid` | Unix group ID or macOS UUID |

#### Returns

[`Str`](../../std/str.md)

**Errors:**

- Raises [`sys.NotFoundError`](../../sys/not-found-error.md) when the ID is
  unknown.
- Raises `UnsupportedError` when the active VFS target is not Unix, or when
  a `uuid.Uuid` is passed and the target is not macOS.

### `group_id name`

Resolves a Unix group name in the active VFS target.

#### Returns

[`Int`](../../std/int.md)

**Errors:**

- Raises [`sys.NotFoundError`](../../sys/not-found-error.md) when the name is
  unknown.
- Raises `UnsupportedError` when the active VFS target is not Unix.
