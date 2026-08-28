# `Sid`

Windows security identifier.

## Constructor

### `Sid value`

Constructs a SID from its canonical string, its native binary
representation, or a symbol naming a well-known SID.

#### Parameters

| Name    | Type                                                                            | Description        |
| ------- | ------------------------------------------------------------------------------- | ------------------ |
| `value` | [`Str`](../../std/str.md)\|[`Bin`](../../std/bin.md)\|[`Sym`](../../std/sym.md) | SID representation |

#### Errors

Raises `ValueError` for a malformed representation, or for a symbol that is
not one of the well-known SIDs below.

## Well-known SIDs

These SIDs are identical on every Windows installation, so naming one takes
no lookup. Any parameter that accepts a `Sid` accepts these symbols.

SIDs that are relative to a domain or a machine — `Domain Admins`
(`S-1-5-21-<domain>-512`), the local `Administrator` account — have no
symbol: resolving one requires querying the system. The SDDL column is the
two-letter alias the same SID has in an SDDL string, for readers porting one;
those aliases are not accepted as symbols.

| Symbol                                  | SID            | SDDL | Account or group                       |
| --------------------------------------- | -------------- | ---- | -------------------------------------- |
| `:NULL:`                                | `S-1-0-0`      |      | Null SID                               |
| `:EVERYONE:`                            | `S-1-1-0`      | `WD` | Everyone                               |
| `:LOCAL:`                               | `S-1-2-0`      |      | Local logon                            |
| `:CONSOLE_LOGON:`                       | `S-1-2-1`      |      | Console logon                          |
| `:CREATOR_OWNER:`                       | `S-1-3-0`      | `CO` | Creator owner                          |
| `:CREATOR_GROUP:`                       | `S-1-3-1`      | `CG` | Creator group                          |
| `:OWNER_RIGHTS:`                        | `S-1-3-4`      | `OW` | Owner rights                           |
| `:DIALUP:`                              | `S-1-5-1`      |      | Dialup                                 |
| `:NETWORK:`                             | `S-1-5-2`      | `NU` | Network logon                          |
| `:BATCH:`                               | `S-1-5-3`      | `BU` | Batch logon                            |
| `:INTERACTIVE:`                         | `S-1-5-4`      | `IU` | Interactive logon                      |
| `:SERVICE:`                             | `S-1-5-6`      | `SU` | Service logon                          |
| `:ANONYMOUS:`                           | `S-1-5-7`      | `AN` | Anonymous logon                        |
| `:PRINCIPAL_SELF:`                      | `S-1-5-10`     | `PS` | Principal self                         |
| `:AUTHENTICATED_USERS:`                 | `S-1-5-11`     | `AU` | Authenticated users                    |
| `:RESTRICTED_CODE:`                     | `S-1-5-12`     | `RC` | Restricted code                        |
| `:REMOTE_INTERACTIVE_LOGON:`            | `S-1-5-14`     |      | Remote interactive logon               |
| `:THIS_ORGANIZATION:`                   | `S-1-5-15`     |      | This organization                      |
| `:LOCAL_SYSTEM:`                        | `S-1-5-18`     | `SY` | Local system                           |
| `:LOCAL_SERVICE:`                       | `S-1-5-19`     | `LS` | Local service                          |
| `:NETWORK_SERVICE:`                     | `S-1-5-20`     | `NS` | Network service                        |
| `:LOCAL_ACCOUNT:`                       | `S-1-5-113`    |      | Any local account                      |
| `:LOCAL_ACCOUNT_ADMINISTRATOR:`         | `S-1-5-114`    |      | Local account in Administrators        |
| `:BUILTIN_ADMINISTRATORS:`              | `S-1-5-32-544` | `BA` | BUILTIN\Administrators                 |
| `:BUILTIN_USERS:`                       | `S-1-5-32-545` | `BU` | BUILTIN\Users                          |
| `:BUILTIN_GUESTS:`                      | `S-1-5-32-546` | `BG` | BUILTIN\Guests                         |
| `:BUILTIN_POWER_USERS:`                 | `S-1-5-32-547` | `PU` | BUILTIN\Power Users                    |
| `:BUILTIN_BACKUP_OPERATORS:`            | `S-1-5-32-551` | `BO` | BUILTIN\Backup Operators               |
| `:BUILTIN_REMOTE_DESKTOP_USERS:`        | `S-1-5-32-555` | `RD` | BUILTIN\Remote Desktop Users           |
| `:BUILTIN_REMOTE_MANAGEMENT_USERS:`     | `S-1-5-32-580` | `RM` | BUILTIN\Remote Management Users        |
| `:ALL_APPLICATION_PACKAGES:`            | `S-1-15-2-1`   | `AC` | All application packages               |
| `:ALL_RESTRICTED_APPLICATION_PACKAGES:` | `S-1-15-2-2`   |      | All restricted application packages    |
| `:UNTRUSTED_LABEL:`                     | `S-1-16-0`     |      | Untrusted integrity level              |
| `:LOW_LABEL:`                           | `S-1-16-4096`  | `LW` | Low integrity level                    |
| `:MEDIUM_LABEL:`                        | `S-1-16-8192`  | `ME` | Medium integrity level                 |
| `:MEDIUM_PLUS_LABEL:`                   | `S-1-16-8448`  | `MP` | Medium-plus integrity level            |
| `:HIGH_LABEL:`                          | `S-1-16-12288` | `HI` | High integrity level                   |
| `:SYSTEM_LABEL:`                        | `S-1-16-16384` | `SI` | System integrity level                 |

### Example

```
let admins = Sid :BUILTIN_ADMINISTRATORS:
echo $ str $admins
```

## Fields

### `revision`

SID revision number.

### `identifier_authority`

The identifier authority as a symbol when it is well known, or its 48-bit
integer value otherwise.

| Symbol               | Authority           |
| -------------------- | ------------------- |
| `:NULL:`             | Null                |
| `:WORLD:`            | World               |
| `:LOCAL:`            | Local               |
| `:CREATOR:`          | Creator             |
| `:NON_UNIQUE:`       | Non-unique          |
| `:NT:`               | NT                  |
| `:RESOURCE_MANAGER:` | Resource manager    |
| `:APP_PACKAGE:`      | Application package |
| `:MANDATORY_LABEL:`  | Mandatory label     |
| `:SCOPED_POLICY:`    | Scoped policy ID    |
| `:AUTHENTICATION:`   | Authentication      |
| `:PROCESS_TRUST:`    | Process trust       |

### `sub_authority_count`

Number of sub-authorities.

### `sub_authorities`

Sub-authorities as an immutable [`Tuple`](../../std/tuple.md).

## Methods

### `lookup()`

Resolves the SID in the active Windows VFS target.

#### Returns

[`SidName`](./sidname.md)

#### Errors

| Exception                                            | Condition                         |
| ---------------------------------------------------- | --------------------------------- |
| [`sys.NotFoundError`](../../sys/not-found-error.md)  | The SID is unmapped               |
| [`UnsupportedError`](../../std/unsupported-error.md) | The active VFS target is Unix     |

### `to_bin()`

Returns the native Windows packet representation.

#### Returns

[`Bin`](../../std/bin.md)

#### Example

```
let sid = Sid S-1-5-32-544
echo $sid.identifier_authority
let encoded = sid.to_bin()
```
