# `Acl`

Immutable NFSv4 access-control list.

## Constructor

### `Acl aces`

Constructs an ACL from an iterable of [`Ace`](./ace.md) values, in
evaluation order.

#### Parameters

| Name   | Type     | Description            |
| ------ | -------- | ---------------------- |
| `aces` | iterable | Access-control entries |

Unlike a POSIX.1e ACL, an NFSv4 ACL has no required entries or completeness
invariant: an empty ACL and any combination of entries construct without
error.

The capitalized constructor accepts only built `Ace` values. Use
[`security.nfs4.acl`](./index.md#acl-aces) to mix built entries with
declarative dictionaries or to pass entries variadically.

## Fields

### `aces`

Immutable array-like view of [`Ace`](./ace.md) values, in evaluation order.
Use `.len` for the entry count.
