# UserInfo

Reports a fresh snapshot of mutable Windows user-account state.

Passwords are write-only and are never included in snapshots.

## Fields

### `sid`

The stable `security.windows.Sid`.

### `name`

Account name. **Type:** [`Str`](../std/str.md)

### `full_name`

Display name. **Type:** [`Str`](../std/str.md)|`nil`

### `comment`

Administrative account comment. **Type:** [`Str`](../std/str.md)|`nil`

### `user_comment`

User-facing account comment. **Type:** [`Str`](../std/str.md)|`nil`

### `home_dir`

Home directory. **Type:** [`fs.windows.Path`](../fs/windows/path.md)|`nil`

### `home_dir_drive`

Drive designator assigned to the home directory during logon, such as `"Z:"`.
**Type:** [`Str`](../std/str.md)|`nil`

### `profile`

Profile path. **Type:** [`fs.windows.Path`](../fs/windows/path.md)|`nil`

### `script_path`

Logon script path. **Type:** [`fs.windows.Path`](../fs/windows/path.md)|`nil`

### `flags`

Complete native account flags. **Type:** [`UserFlags`](./user-flags.md)

### `disabled`

Whether the account is disabled. **Type:** [`Bool`](../std/bool.md)

### `password_never_expires`

Whether the password is exempt from expiry. **Type:**
[`Bool`](../std/bool.md)

### `password_cannot_change`

Whether the user can change the password. **Type:** [`Bool`](../std/bool.md)

### `password_age`

Time since the password was last changed. **Type:**
[`time.Duration`](../time/duration.md)

### `password_expired`

Whether the password has expired. **Type:** [`Bool`](../std/bool.md)

### `last_logon`

Last recorded logon time. **Type:** [`time.DateTime`](../time/datetime.md)|`nil`

### `account_expires`

Account expiration time. **Type:**
[`time.DateTime`](../time/datetime.md)|`nil`

### `bad_password_count`

Recorded failed password attempts. **Type:** [`Int`](../std/int.md)

### `logon_count`

Recorded successful logons. **Type:** [`Int`](../std/int.md)
