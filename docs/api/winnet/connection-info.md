# ConnectionInfo

A fresh SMB client connection snapshot.

## Fields

### `local`

Redirected local device such as `"Z:"` as a `Str`, or `nil` for a deviceless
connection.

### `remote`

Remote resource in UNC form as a `Str`.

### `provider`

Network provider name as a `Str`, or `nil` when the provider reports none.

### `user`

Account the connection authenticated as, as a `Str`, or `nil`. A connection
saved in the profile but not currently established has no account to report.

### `kind`

Resource kind: `:DISK:`, `:PRINT:`, or `:ANY:`. A provider resource type this
binding does not model reports as `:ANY:`.

### `state`

`:CONNECTED:` when the connection is established, `:REMEMBERED:` when it is
saved in the profile but not currently connected — what `net use` shows as
`Unavailable`.

### `persistent`

Whether the connection is recorded in the profile and restored at logon.
Independent of [`state`](#state): a restored mapping whose server is reachable
is both `:CONNECTED:` and persistent.
