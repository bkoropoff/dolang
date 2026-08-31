# User

Identifies a Windows local user by immutable SID and a refreshable cached name.

## Fields

### `sid`

The stable `security.windows.Sid`.

### `name`

The current cached account name.

## Methods

### `info()`

Returns a fresh [`UserInfo`](./user-info.md) snapshot.

### `update ...options`

Updates account state and returns a fresh [`UserInfo`](./user-info.md).

Accepts the shared [User options](./index.md#user-options). Passwords cannot be
cleared with `nil`.

### `delete()`

Deletes the account.
