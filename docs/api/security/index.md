# security

The `security` module reports the security identity of the active VFS target.

Platform-specific types and functions are exposed by
[`security.unix`](./unix/index.md), [`security.nfs4`](./nfs4/index.md),
[`security.macos`](./macos/index.md), and
[`security.windows`](./windows/index.md).

## Functions

### `user_name()`

Returns the current target user's name, on both platform families: the real
user ID's name on Unix, or the access token's user SID's name on Windows.

#### Returns

[`Str`](../std/str.md)

```
echo "running as $(user_name())"
```
