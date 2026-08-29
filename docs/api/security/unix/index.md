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

### `ace :user_obj? :user? :group_obj? :group? :mask? :other? :permissions?`

Constructs a POSIX entry from declarative arguments. Pass exactly one
qualifier. `user_obj:`, `group_obj:`, `mask:`, and `other:` take permissions;
`user:` and `group:` take a numeric ID and accept a separate optional
`permissions:` value. Named entries default to empty permissions.

Permissions may be a [`Permission`](./permission.md), a permission symbol, or
an iterable of permission symbols.

#### Example

```
let owner = ace user_obj: [:READ:, :WRITE:]
let named = ace user: 1000 permissions: :READ:
```

### `acl ...aces`

Constructs a POSIX ACL from [`Ace`](./ace.md) values and declarative ACE
dictionaries. Pass collections with `...` to spread their entries.

```
acl
  $ace(user_obj: [:READ:, :WRITE:])
  {group_obj: [:READ:]}
  {other: []}
```

The resulting ACL must satisfy the POSIX completeness rules documented by
[`Acl`](./acl.md).

### `id()`

Returns Unix security information captured for the active VFS context.

#### Returns

[`Identity`](./identity.md)

#### Errors

- Raises `UnsupportedError` when the active VFS target is not Unix.

#### Example

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

#### Errors

| Exception                                            | Condition                                                      |
| ---------------------------------------------------- | -------------------------------------------------------------- |
| [`sys.NotFoundError`](../../sys/not-found-error.md)  | The ID is unknown                                              |
| [`UnsupportedError`](../../std/unsupported-error.md) | The active VFS target is not Unix                              |
| [`UnsupportedError`](../../std/unsupported-error.md) | A `uuid.Uuid` is passed and the active VFS target is not macOS |

### `user_id name`

Resolves a Unix user name in the active VFS target.

#### Returns

[`Int`](../../std/int.md)

#### Errors

| Exception                                            | Condition                         |
| ---------------------------------------------------- | --------------------------------- |
| [`sys.NotFoundError`](../../sys/not-found-error.md)  | The name is unknown               |
| [`UnsupportedError`](../../std/unsupported-error.md) | The active VFS target is not Unix |

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

#### Errors

| Exception                                            | Condition                                                      |
| ---------------------------------------------------- | -------------------------------------------------------------- |
| [`sys.NotFoundError`](../../sys/not-found-error.md)  | The ID is unknown                                              |
| [`UnsupportedError`](../../std/unsupported-error.md) | The active VFS target is not Unix                              |
| [`UnsupportedError`](../../std/unsupported-error.md) | A `uuid.Uuid` is passed and the active VFS target is not macOS |

### `group_id name`

Resolves a Unix group name in the active VFS target.

#### Returns

[`Int`](../../std/int.md)

#### Errors

| Exception                                            | Condition                         |
| ---------------------------------------------------- | --------------------------------- |
| [`sys.NotFoundError`](../../sys/not-found-error.md)  | The name is unknown               |
| [`UnsupportedError`](../../std/unsupported-error.md) | The active VFS target is not Unix |
