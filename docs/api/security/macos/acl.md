# `Acl`

Immutable macOS extended access-control list.

## Constructor

### `Acl aces`

Constructs an ACL from an iterable of [`Ace`](./ace.md) values, in
evaluation order.

#### Parameters

| Name   | Type     | Description            |
| ------ | -------- | ---------------------- |
| `aces` | iterable | Access-control entries |

Like an NFSv4 ACL, a macOS extended ACL has no required entries or
completeness invariant: an empty ACL and any combination of entries
construct without error.

## Fields

### `aces`

Immutable array-like view of [`Ace`](./ace.md) values, in evaluation order.
Use `.len` for the entry count.
