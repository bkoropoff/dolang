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

The capitalized constructor accepts only built `Ace` values. Use
[`security.macos.acl`](./index.md#acl-aces) to mix built entries with
declarative dictionaries or to pass entries variadically.

## Fields

### `aces`

Immutable array-like view of [`Ace`](./ace.md) values, in evaluation order.
Use `.len` for the entry count.
