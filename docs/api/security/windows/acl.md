# `Acl`

Immutable view of a native Windows access-control list.

A list can also be written as an [ACL spec](./index.md#acl-specs) wherever one
is accepted, including the `dacl:` and `sacl:` options of
[`sec_desc`](./index.md#sec_desc-desc-options). The constructor below stays
strict: its entries must already be [`Ace`](./ace.md) values.

## Constructor

### `Acl aces :revision = nil`

Constructs an ACL from an iterable of [`Ace`](./ace.md) values.

#### Parameters

| Name       | Type                                                  | Description                               |
| ---------- | ----------------------------------------------------- | ----------------------------------------- |
| `aces`     | iterable                                              | Entries in packet order                   |
| `revision` | [`Sym`](../../std/sym.md)\|[`Int`](../../std/int.md)? | `:BASIC:`, `:DIRECTORY_SERVICE:`, 2, or 4 |

Revision 4 is selected when an object ACE is present; otherwise revision 2 is
selected. Supplying revision 2 with an object ACE raises `ValueError`.

## Fields

### `revision`

`:BASIC:` or `:DIRECTORY_SERVICE:` for supported revisions, or the native
integer for an unknown parsed revision.

### `size`

Declared ACL packet size.

### `aces`

Immutable array-like view of [`Ace`](./ace.md) values. Use `.len` for the
entry count.

## Methods

### `to_bin()`

Returns the exact native ACL packet.

#### Returns

[`Bin`](../../std/bin.md)
