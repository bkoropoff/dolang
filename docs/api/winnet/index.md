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
| [`Share`](./share.md)                  | Local SMB share capability         |
| [`ShareInfo`](./share-info.md)         | Fresh local SMB share snapshot     |
| [`JoinStatus`](./join-status.md)       | Workgroup or domain membership     |
| [`MachineInfo`](./machine-info.md)     | Machine identity and server role   |
| [`ServerType`](./server-type.md)       | Native server role mask            |

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

## Join Credentials

[`join_domain`](#join_domain-domain-options),
[`unjoin_domain`](#unjoin_domain-options) and
[`rename_machine`](#rename_machine-name-options) accept domain credentials
authorized for the operation. `account` and `password` must be supplied
together; supplying one without the other is an error. Omit both to use the
caller's own credentials.

| Name       | Type                    | Description                                        |
| ---------- | ----------------------- | -------------------------------------------------- |
| `account`  | [`Str`](../std/str.md)? | Domain account, such as `"CORP\\joiner"`           |
| `password` | [`Str`](../std/str.md)? | Password for `account`                             |

## Domain Join

These operations change how the machine is identified on the network. **Every
one of them takes effect only after the machine is restarted**, so a
provisioning script must sequence a reboot before the change is usable. None
of them return a status; use [`join_status`](#join_status) to read the
recorded membership.

[`provision_computer`](#provision_computer-domain-machine-options) and
[`apply_offline_join`](#apply_offline_join-blob-windows_path-options) split a
join in two: the first runs where a domain controller is reachable and mints a
blob, the second applies that blob to a Windows installation that has never
talked to the domain. This is the path for image-based and first-boot
provisioning, where the online join would need network, a reachable domain
controller and credentials at exactly the wrong moment.

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

### `share name_or_info`

Obtains a share capability from a share name or [`ShareInfo`](./share-info.md).

#### Parameters

| Name           | Type                                                   | Description                  |
| -------------- | ------------------------------------------------------ | ---------------------------- |
| `name_or_info` | [`Str`](../std/str.md)\|[`ShareInfo`](./share-info.md) | Share name or prior snapshot |

#### Returns

[`Share`](./share.md)

### `shares()`

Iterates every local SMB share, including administrative and non-disk shares.

#### Returns

`Iter` over [`ShareInfo`](./share-info.md)

### `create_share :name :path ...options`

Creates a local SMB share. The default kind is `:DISKTREE:`, usage is
unlimited, and Windows supplies the default security descriptor. `kind`
accepts `:DISKTREE:`, `:PRINTQ:`, `:DEVICE:`, or `:IPC:`.

#### Parameters

| Name        | Type                                                          | Description                                                          |
| ----------- | ------------------------------------------------------------- | -------------------------------------------------------------------- |
| `name`      | [`Str`](../std/str.md)                                        | Share name                                                           |
| `path`      | [`fs.windows.Path`](../fs/windows/path.md)                    | Shared local path                                                    |
| `kind`      | [`Sym`](../std/sym.md)?                                       | Resource kind                                                        |
| `comment`   | [`Str`](../std/str.md)?                                       | Comment                                                              |
| `max_uses`  | [`Int`](../std/int.md)?                                       | Connection limit; `nil` is unlimited                                 |
| `special`   | [`Bool`](../std/bool.md)?                                     | Marks a special share                                                |
| `temporary` | [`Bool`](../std/bool.md)?                                     | Marks a temporary share                                              |
| `sec_desc`  | [`security.windows.SecDesc`](../security/windows/secdesc.md)? | Security descriptor, self-relative packet, or declarative descriptor |

#### Returns

[`Share`](./share.md)

### `join_status()`

Returns the machine's current workgroup or domain membership.

#### Returns

[`JoinStatus`](./join-status.md)

### `machine_info()`

Returns the computer name, domain membership, OS level and server role.

#### Returns

[`MachineInfo`](./machine-info.md)

### `join_domain :domain ...options`

Joins the machine to a domain. **Takes effect only after a restart.**

Supply either `account` and `password` (see
[Join credentials](#join-credentials)) or `machine_password`; the two are
mutually exclusive, because joining with a pre-created computer account
password leaves no room for a user account.

#### Parameters

| Name                | Type                      | Description                                                   |
| ------------------- | ------------------------- | ------------------------------------------------------------- |
| `domain`            | [`Str`](../std/str.md)    | Domain to join                                                |
| `ou`                | [`Str`](../std/str.md)?   | Organizational unit for the computer account                  |
| `account`           | [`Str`](../std/str.md)?   | Domain account authorized to join                             |
| `password`          | [`Str`](../std/str.md)?   | Password for `account`                                        |
| `machine_password`  | [`Str`](../std/str.md)?   | Pre-created computer account password, instead of an account  |
| `create_account`    | [`Bool`](../std/bool.md)? | Create the computer account as part of the join               |
| `join_if_joined`    | [`Bool`](../std/bool.md)? | Proceed even if already joined to a domain                    |
| `unsecure`          | [`Bool`](../std/bool.md)? | Perform an unsecured join                                     |
| `defer_spn`         | [`Bool`](../std/bool.md)? | Defer setting the service principal name until a later rename |
| `force_spn`         | [`Bool`](../std/bool.md)? | Force the service principal name to be set                    |
| `dc_account`        | [`Bool`](../std/bool.md)? | Join using a domain controller account                        |
| `with_new_name`     | [`Bool`](../std/bool.md)? | Join under a rename that has not taken effect yet             |
| `readonly`          | [`Bool`](../std/bool.md)? | Join against a read-only domain controller                    |
| `ambiguous_dc`      | [`Bool`](../std/bool.md)? | Allow an ambiguous domain controller name                     |
| `no_netlogon_cache` | [`Bool`](../std/bool.md)? | Do not write the netlogon cache                               |
| `no_account_reuse`  | [`Bool`](../std/bool.md)? | Fail rather than reuse an existing computer account           |

#### Returns

`nil`

#### Errors

| Exception                                  | Condition                                                  |
| ------------------------------------------ | ---------------------------------------------------------- |
| [`AlreadyExistsError`](../sys/index.md)    | Already joined and `join_if_joined` was not set            |
| [`InvalidInputError`](../sys/index.md)     | Credentials are incomplete, or both credential forms given |
| [`PermissionDeniedError`](../sys/index.md) | Caller may not join the machine to the domain              |

#### Example

```
winnet.join_domain
  domain: corp.example.com
  ou: "OU=Servers,DC=corp,DC=example,DC=com"
  account: r"CORP\joiner"
  password: $password
  create_account: true
echo "restart to complete the join"
```

### `unjoin_domain ...options`

Removes the machine from its domain. **Takes effect only after a restart.**

#### Parameters

| Name             | Type                      | Description                                     |
| ---------------- | ------------------------- | ----------------------------------------------- |
| `account`        | [`Str`](../std/str.md)?   | Domain account authorized to unjoin             |
| `password`       | [`Str`](../std/str.md)?   | Password for `account`                          |
| `delete_account` | [`Bool`](../std/bool.md)? | Disable the computer account in the domain      |

#### Returns

`nil`

#### Errors

| Exception                                  | Condition                                    |
| ------------------------------------------ | -------------------------------------------- |
| [`NotFoundError`](../sys/index.md)         | The machine is not joined to a domain        |
| [`InvalidInputError`](../sys/index.md)     | The machine is a domain controller           |
| [`PermissionDeniedError`](../sys/index.md) | Caller may not unjoin the machine            |

### `rename_machine name ...options`

Renames the machine within its domain. **Takes effect only after a restart.**

#### Parameters

| Name             | Type                      | Description                                    |
| ---------------- | ------------------------- | ---------------------------------------------- |
| `name`           | [`Str`](../std/str.md)    | New computer name                              |
| `account`        | [`Str`](../std/str.md)?   | Domain account authorized to rename            |
| `password`       | [`Str`](../std/str.md)?   | Password for `account`                         |
| `create_account` | [`Bool`](../std/bool.md)? | Create the renamed computer account if missing |

#### Returns

`nil`

### `provision_computer :domain :machine ...options`

Creates a computer account in the domain and returns the blob that joins a
machine to it. Run this where a domain controller is reachable, then apply the
blob with
[`apply_offline_join`](#apply_offline_join-blob-windows_path-options).

The blob contains the computer account password. Treat it as a secret: anyone
holding it can join a machine as that account.

#### Parameters

| Name                     | Type                      | Description                                          |
| ------------------------ | ------------------------- | ---------------------------------------------------- |
| `domain`                 | [`Str`](../std/str.md)    | Domain to create the computer account in             |
| `machine`                | [`Str`](../std/str.md)    | Computer name to provision                           |
| `ou`                     | [`Str`](../std/str.md)?   | Organizational unit for the computer account         |
| `dc`                     | [`Str`](../std/str.md)?   | Domain controller to provision against               |
| `reuse`                  | [`Bool`](../std/bool.md)? | Reuse an existing computer account                   |
| `default_password`       | [`Bool`](../std/bool.md)? | Use the default computer account password            |
| `skip_account_search`    | [`Bool`](../std/bool.md)? | Skip searching for an existing account               |
| `root_ca_certs`          | [`Bool`](../std/bool.md)? | Include root CA certificates in the blob             |
| `downlevel_priv_support` | [`Bool`](../std/bool.md)? | Support provisioning by a down-level privileged user |

#### Returns

[`Bin`](../std/bin.md)

#### Example

```
let blob = winnet.provision_computer
  domain: corp.example.com
  machine: WS01
  ou: "OU=Workstations,DC=corp,DC=example,DC=com"
fs.Path("ws01.blob").write $blob
```

### `apply_offline_join blob :windows_path ...options`

Applies a provisioning blob to a Windows installation, joining it to the
domain without contacting a domain controller. **Takes effect only after the
target installation is started or restarted.**

`windows_path` is the Windows directory of the installation to modify —
the mounted image's `Windows` directory for an offline image, or the running
system's own when `online` is set.

#### Parameters

| Name           | Type                                       | Description                                        |
| -------------- | ------------------------------------------ | -------------------------------------------------- |
| `blob`         | [`Bin`](../std/bin.md)                     | Blob from `provision_computer`                     |
| `windows_path` | [`fs.windows.Path`](../fs/windows/path.md) | Windows directory of the installation to modify    |
| `online`       | [`Bool`](../std/bool.md)?                  | The target installation is the running system      |

#### Returns

`nil`

#### Example

```
let blob = fs.Path("ws01.blob").read "b"
winnet.apply_offline_join $blob
  windows_path: (fs.windows.Path r"D:\Windows")
```
