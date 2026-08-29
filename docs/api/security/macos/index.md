# security.macos

The `security.macos` module exposes macOS extended access-control-list
types and principal identity resolution between Unix uid/gid values and
macOS `guid_t` UUIDs.

## Types

| Type                  | Description                          |
| --------------------- | ------------------------------------ |
| [`Acl`](./acl.md)     | macOS extended access-control list   |
| [`Ace`](./ace.md)     | macOS extended access-control entry  |
| [`Mask`](./mask.md)   | macOS extended ACE permission mask   |
| [`Flags`](./flags.md) | macOS extended ACE inheritance flags |

## Functions

### `ace :allow? :deny? mask: :flags?`

Constructs a macOS entry from declarative arguments. Pass exactly one of
`allow:` or `deny:` with a principal UUID. Declarative principals accept a
[`uuid.Uuid`](../../uuid/uuid.md), UUID string, or 16-byte UUID binary value.
`mask:` is required; `flags:` defaults to empty. Masks and flags accept their
built type, one flag symbol, or an iterable of flag symbols.

```
ace allow: "00112233-4455-6677-8899-aabbccddeeff" mask: [:READ_DATA:]
```

### `acl ...aces`

Constructs a macOS ACL from [`Ace`](./ace.md) values and declarative ACE
dictionaries. Pass collections with `...` to spread their entries. An empty
ACL is valid.

### `uuid_for_uid uid`

Resolves a Unix user ID to its macOS principal UUID.

#### Parameters

| Name  | Type  | Description  |
| ----- | ----- | ------------ |
| `uid` | `Int` | Unix user ID |

#### Returns

[`uuid.Uuid`](../../uuid/uuid.md)

#### Errors

- Raises `UnsupportedError` when the active VFS target is not macOS.

#### Example

```
let owner = security.macos.uuid_for_uid 501
```

### `uuid_for_gid gid`

Resolves a Unix group ID to its macOS principal UUID.

#### Parameters

| Name  | Type  | Description   |
| ----- | ----- | ------------- |
| `gid` | `Int` | Unix group ID |

#### Returns

[`uuid.Uuid`](../../uuid/uuid.md)

#### Errors

- Raises `UnsupportedError` when the active VFS target is not macOS.

### `id_for_uuid uuid`

Resolves a macOS principal UUID back to the Unix uid or gid it identifies.
The Membership framework reports which kind the UUID resolved to, so the
result carries both the id and whether it's a user or group.

#### Parameters

| Name   | Type                      | Description          |
| ------ | ------------------------- | -------------------- |
| `uuid` | `uuid.Uuid`\|`Str`\|`Bin` | macOS principal UUID |

#### Returns

A two-element [`Tuple`](../../std/tuple.md) of `(kind, id)`, where `kind` is
`:UID:` or `:GID:` and `id` is an `Int`.

#### Errors

- Raises `UnsupportedError` when the active VFS target is not macOS.

#### Example

```
let kind id = security.macos.id_for_uuid owner
if (kind == :UID:)
  echo "uid: $id"
else
  echo "gid: $id"
```
