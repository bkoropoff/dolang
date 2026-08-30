# `TokenInfo`

Windows access token information.

## Fields

### `groups`

A lazy array-like view of the token's [`TokenGroup`](./tokengroup.md) objects.

### `is_elevated`

Whether the token is elevated.

### `logon_sid`

The group SID marked as the token's logon SID, or `nil` if none is present.

### `owner_sid`

Default owner SID for objects created by the token.

### `primary_group_sid`

Primary group SID for objects created by the token.

### `user_sid`

SID of the token's user.
