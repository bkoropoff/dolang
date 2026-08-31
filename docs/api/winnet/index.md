# winnet

Windows NetAPI bindings.

## Types

| Type                           | Description                        |
| ------------------------------ | ---------------------------------- |
| [`User`](./user.md)            | SID-stable local user principal    |
| [`UserInfo`](./user-info.md)   | Fresh account-information snapshot |
| [`UserFlags`](./user-flags.md) | Native account flag mask           |

## User Options

[`create_user`](#create_user-name-password-options) and
[`User.update`](./user.md#update-options) accept these keyword options.
`password` is required by `create_user` and optional for `User.update`. Other
omitted options leave their default or current value unchanged. `nil` clears
nullable fields and removes the account expiration.

| Name                     | Type                                    | Description                                |
| ------------------------ | --------------------------------------- | ------------------------------------------ |
| `password`               | [`Str`](../std/str.md)                  | Password to set                            |
| `full_name`              | [`Str`](../std/str.md)?                 | Display name                               |
| `comment`                | [`Str`](../std/str.md)?                 | Administrative account comment             |
| `user_comment`           | [`Str`](../std/str.md)?                 | User-facing account comment                |
| `home_dir`               | [`Str`](../std/str.md)?                 | Home directory                             |
| `home_dir_drive`         | [`Str`](../std/str.md)?                 | Drive mapped to the home directory         |
| `profile`                | [`Str`](../std/str.md)?                 | Profile path                               |
| `script_path`            | [`Str`](../std/str.md)?                 | Logon script path                          |
| `account_expires`        | [`time.DateTime`](../time/datetime.md)? | Expiration instant; `nil` means never      |
| `disabled`               | [`Bool`](../std/bool.md)?               | Whether the account is disabled            |
| `password_never_expires` | [`Bool`](../std/bool.md)?               | Whether the password is exempt from expiry |
| `password_cannot_change` | [`Bool`](../std/bool.md)?               | Whether the user can change the password   |

## Functions

### `user principal`

Looks up a user by account name or `security.windows.Sid`.

#### Returns

[`User`](./user.md)

### `users()`

Iterates local user accounts.

#### Returns

`Iter` over [`User`](./user.md)

### `create_user name :password ...options`

Creates a normal enabled local user and returns its SID-stable `User`.

See [User options](#user-options) for supported keyword options.

#### Parameters

| Name   | Type                   | Description  |
| ------ | ---------------------- | ------------ |
| `name` | [`Str`](../std/str.md) | Account name |

#### Returns

[`User`](./user.md)

#### Example

```
let user = create_user "build-user" password: $password
  full_name: "Build User"
  comment: "Automation account"
  disabled: true
  password_never_expires: true
```
