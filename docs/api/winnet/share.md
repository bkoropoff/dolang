# Share

A local SMB share capability.

## Fields

### `name`

The immutable share name.

## Methods

### `info()`

Returns a fresh [`ShareInfo`](./share-info.md) snapshot.

### `update :comment? :max_uses? :sec_desc?`

Updates only the supplied fields and returns fresh information. `nil` clears
the comment or selects unlimited use. A security descriptor accepts a
`security.windows.SecDesc`, self-relative `Bin`, or declarative descriptor.

Updates are applied in comment, maximum-use, descriptor order. If a later
update fails, earlier updates remain applied.

#### Returns

[`ShareInfo`](./share-info.md)

### `delete()`

Deletes the share and invalidates this capability.
