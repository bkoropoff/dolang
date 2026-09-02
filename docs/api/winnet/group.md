# Group

Identifies a Windows local group by immutable SID.

## Fields

### `sid`

The stable `security.windows.Sid`.

## Methods

### `info()`

Returns a fresh [`GroupInfo`](./group-info.md) snapshot.

### `update :name? :comment?`

Updates the comment before applying a requested rename and returns a fresh
[`GroupInfo`](./group-info.md). `comment: nil` clears the comment. If renaming
fails, the comment change remains applied.

### `members()`

Iterates members as
[`security.windows.SidName`](../security/windows/sidname.md).

### `add_member principal`

Adds an account name or `security.windows.Sid` to the group. Adding an existing
member raises `sys.AlreadyExistsError`.

### `remove_member principal`

Removes an account name or `security.windows.Sid` from the group. Removing an
absent member raises `sys.NotFoundError`.

### `rights()`

Returns the account rights assigned through the local security policy.

### `grant_right name`

Grants an account right such as `SeServiceLogonRight`.

### `revoke_right name`

Revokes an account right. Revoking an unassigned right has no effect.

### `delete()`

Deletes the group.
