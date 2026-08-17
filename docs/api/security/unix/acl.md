# `Acl`

Immutable POSIX.1e access-control list.

## Constructor

### `Acl aces`

Constructs an ACL from an iterable of [`Ace`](./ace.md) values.

#### Parameters

| Name   | Type     | Description            |
| ------ | -------- | ---------------------- |
| `aces` | iterable | Access-control entries |

#### Errors

| Exception    | Condition                                                     |
| ------------ | ------------------------------------------------------------- |
| `ValueError` | A required owner-user, owner-group, or other entry is missing |
| `ValueError` | Entry qualifiers are duplicated                               |
| `ValueError` | Named user or group entries are present without a mask entry  |

The constructor preserves the supplied mask. It does not calculate one.

## Fields

### `aces`

Immutable array-like view of [`Ace`](./ace.md) values. Use `.len` for the
entry count.
