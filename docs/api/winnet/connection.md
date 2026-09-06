# Connection

A connection to a remote SMB resource.

## Fields

### `name`

The local device the connection redirects, or its remote name when it
redirects none.

### `connected`

Whether this capability is still connected. Becomes `false` after
[`disconnect`](#disconnect-force-forget_credentials).

## Methods

### `info()`

Returns a fresh [`ConnectionInfo`](./connection-info.md) snapshot.

### `path()`

Returns the connection's root as an
[`fs.windows.Path`](../fs/windows/path.md): the drive root when the connection
redirects a local device, and the remote name otherwise.

```
connect \\build\artifacts local: Z: do |conn|
  fs.copy $release $conn.path()
```

### `disconnect :force? :forget_credentials?`

Disconnects and invalidates this capability. Disconnecting an already
disconnected connection succeeds and does nothing, so a connection may be
disconnected explicitly inside a scoped
[`connect`](./index.md#connect-remote-options-func) block.

A persistent connection's profile entry is removed, so it is not restored at
the next logon.

#### Parameters

| Name                 | Type                      | Description                                                                                     |
| -------------------- | ------------------------- | ----------------------------------------------------------------------------------------------- |
| `force`              | [`Bool`](../std/bool.md)? | Disconnects even with open files or directories on the connection; defaults to `false`          |
| `forget_credentials` | [`Bool`](../std/bool.md)? | Removes credentials saved for the server; defaults to removing them for a persistent connection |

#### Errors

| Exception                                            | Condition                                                              |
| ---------------------------------------------------- | ---------------------------------------------------------------------- |
| [`ResourceBusyError`](../sys/resource-busy-error.md) | Files or directories are open on the connection and `force` is not set |
