# ShareInfo

A fresh local SMB share snapshot.

## Fields

### `name`

Share name as a `Str`.

### `kind`

Resource kind: `:DISKTREE:`, `:PRINTQ:`, `:DEVICE:`, or `:IPC:`.

### `special`

Whether the native special-share flag is set.

### `temporary`

Whether the native temporary-share flag is set.

### `comment`

Comment as a `Str`, or `nil`.

### `max_uses`

Maximum simultaneous connections as an `Int`, or `nil` for unlimited use.

### `current_uses`

Current connection count as an `Int`.

### `path`

Shared [`fs.windows.Path`](../fs/windows/path.md).

### `sec_desc`

Share [`security.windows.SecDesc`](../security/windows/secdesc.md), or `nil`
when the share is left to the server service's default security.
