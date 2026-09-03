# User

Identifies a Windows local user by immutable SID.

## Fields

### `sid`

The stable `security.windows.Sid`.

## Methods

### `info()`

Returns a fresh [`UserInfo`](./user-info.md) snapshot.

### `update ...options`

Updates account state and returns a fresh [`UserInfo`](./user-info.md).

Accepts the shared [User options](./index.md#user-options). Passwords cannot be
cleared with `nil`.

Requested attributes are applied before `name`. If an earlier update fails,
the rename does not occur. If the rename fails, earlier changes remain applied.
A successful rename keeps the handle usable under the new name.

### `rights()`

Returns the account rights assigned through the local security policy.

### `grant_right name`

Grants an account right such as `SeServiceLogonRight`.

### `revoke_right name`

Revokes an account right. Revoking an unassigned right has no effect.

### `delete()`

Deletes the account.
