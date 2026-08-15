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

Resolves a Unix user ID in the active VFS target.

#### Parameters

| Name  | Type  | Description  |
| ----- | ----- | ------------ |
| `uid` | `Int` | Unix user ID |

#### Returns

[`Str`](../../std/str.md)

**Errors:**

- Raises [`sys.NotFoundError`](../../sys/not-found-error.md) when the ID is
  unknown.
- Raises `UnsupportedError` when the active VFS target is not Unix.

### `user_id name`

Resolves a Unix user name in the active VFS target.

#### Returns

[`Int`](../../std/int.md)

**Errors:**

- Raises [`sys.NotFoundError`](../../sys/not-found-error.md) when the name is
  unknown.
- Raises `UnsupportedError` when the active VFS target is not Unix.

### `group_name gid`

Resolves a Unix group ID in the active VFS target.

#### Returns

[`Str`](../../std/str.md)

**Errors:**

- Raises [`sys.NotFoundError`](../../sys/not-found-error.md) when the ID is
  unknown.
- Raises `UnsupportedError` when the active VFS target is not Unix.

### `group_id name`

Resolves a Unix group name in the active VFS target.

#### Returns

[`Int`](../../std/int.md)

**Errors:**

- Raises [`sys.NotFoundError`](../../sys/not-found-error.md) when the name is
  unknown.
- Raises `UnsupportedError` when the active VFS target is not Unix.
