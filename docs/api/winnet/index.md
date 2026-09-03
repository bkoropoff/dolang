# winnet

Windows NetAPI bindings.

## Types

| Type                                   | Description                        |
| -------------------------------------- | ---------------------------------- |
| [`User`](./user.md)                    | SID-stable local user principal    |
| [`UserInfo`](./user-info.md)           | Fresh account-information snapshot |
| [`UserFlags`](./user-flags.md)         | Native account flag mask           |
| [`Group`](./group.md)                  | SID-stable local group principal   |
| [`GroupInfo`](./group-info.md)         | Fresh local-group snapshot         |
| [`AccountPolicy`](./account-policy.md) | Local password and lockout policy  |

## User Options

[`create_user`](#create_user-name-password-options) and
[`User.update`](./user.md#update-options) accept these keyword options.
`password` is required by `create_user` and optional for `User.update`. Other
omitted options leave their default or current value unchanged. `nil` clears
nullable fields and removes the account expiration.

| Name                     | Type                                        | Description                                |
| ------------------------ | ------------------------------------------- | ------------------------------------------ |
| `name`                   | [`Str`](../std/str.md)?                     | New account name; applied last             |
| `password`               | [`Str`](../std/str.md)                      | Password to set                            |
| `full_name`              | [`Str`](../std/str.md)?                     | Display name                               |
| `comment`                | [`Str`](../std/str.md)?                     | Administrative account comment             |
| `user_comment`           | [`Str`](../std/str.md)?                     | User-facing account comment                |
| `home_dir`               | [`fs.windows.Path`](../fs/windows/path.md)? | Home directory                             |
| `home_dir_drive`         | [`Str`](../std/str.md)?                     | Logon drive designator, such as `"Z:"`     |
| `profile`                | [`fs.windows.Path`](../fs/windows/path.md)? | Profile path                               |
| `script_path`            | [`fs.windows.Path`](../fs/windows/path.md)? | Logon script path                          |
| `account_expires`        | [`time.DateTime`](../time/datetime.md)?     | Expiration instant; `nil` means never      |
| `disabled`               | [`Bool`](../std/bool.md)?                   | Whether the account is disabled            |
| `password_never_expires` | [`Bool`](../std/bool.md)?                   | Whether the password is exempt from expiry |
| `password_cannot_change` | [`Bool`](../std/bool.md)?                   | Whether the user can change the password   |

## Functions

### `account_policy()`

Returns the local password and lockout policy.

#### Returns

[`AccountPolicy`](./account-policy.md)

### `update_account_policy ...options`

Updates the supplied local password and lockout settings. Omitted settings are
unchanged. `nil` selects no limit for `max_password_age` and `force_logoff`.

#### Parameters

| Name                         | Type                                     | Description                              |
| ---------------------------- | ---------------------------------------- | ---------------------------------------- |
| `min_password_length`        | [`Int`](../std/int.md)?                  | Minimum password length                  |
| `max_password_age`           | [`time.Duration`](../time/duration.md)?  | Maximum password age; `nil` means never  |
| `min_password_age`           | [`time.Duration`](../time/duration.md)?  | Minimum password age                     |
| `force_logoff`               | [`time.Duration`](../time/duration.md)?  | Forced-logoff delay; `nil` disables it   |
| `password_history_length`    | [`Int`](../std/int.md)?                  | Number of prior passwords retained       |
| `lockout_duration`           | [`time.Duration`](../time/duration.md)?  | Account lockout duration                 |
| `lockout_observation_window` | [`time.Duration`](../time/duration.md)?  | Failed-logon counter observation window  |
| `lockout_threshold`          | [`Int`](../std/int.md)?                  | Failed-logon threshold; zero disables it |

#### Returns

[`AccountPolicy`](./account-policy.md)

### `user principal`

Obtains a user capability from an account name, `security.windows.Sid`, or
[`UserInfo`](./user-info.md) snapshot.

#### Returns

[`User`](./user.md)

### `users()`

Iterates local user accounts.

#### Returns

`Iter` over [`UserInfo`](./user-info.md)

### `create_user :name :password ...options`

Creates a normal enabled local user and returns its SID-stable `User`.

See [User options](#user-options) for supported keyword options.

#### Parameters

| Name       | Type                   | Description      |
| ---------- | ---------------------- | ---------------- |
| `name`     | [`Str`](../std/str.md) | Account name     |
| `password` | [`Str`](../std/str.md) | Initial password |

#### Returns

[`User`](./user.md)

#### Example

```
let user = create_user name: "build-user" password: $password
  full_name: "Build User"
  comment: "Automation account"
  disabled: true
  password_never_expires: true
```

### `group principal`

Obtains a group capability from an account name, `security.windows.Sid`, or
[`GroupInfo`](./group-info.md) snapshot.

#### Returns

[`Group`](./group.md)

### `groups()`

Iterates local groups.

#### Returns

`Iter` over [`GroupInfo`](./group-info.md)

### `create_group name :comment?`

Creates a local group.

#### Parameters

| Name      | Type                    | Description            |
| --------- | ----------------------- | ---------------------- |
| `name`    | [`Str`](../std/str.md)  | Group name             |
| `comment` | [`Str`](../std/str.md)? | Administrative comment |

#### Returns

[`Group`](./group.md)
