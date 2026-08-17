# security.windows

The `security.windows` module exposes Windows security types.

## Types

| Type                                                  | Description                       |
| ----------------------------------------------------- | --------------------------------- |
| [`AccessMask`](./access-mask.md)                      | Generic Windows object rights     |
| [`SecDescControl`](./secdesc-control.md)              | Security descriptor control flags |
| [`SecInfo`](./sec-info.md)                            | Loaded descriptor components      |
| [`TokenGroupAttributes`](./token-group-attributes.md) | Token group attributes            |
| [`Acl`](./acl.md)                                     | Windows access-control list       |
| [`Ace`](./ace.md)                                     | Windows access-control entry      |
| [`SecDesc`](./secdesc.md)                             | Windows security descriptor       |
| [`Sid`](./sid.md)                                     | Windows security identifier       |
| [`SidName`](./sidname.md)                             | Resolved Windows account identity |
| [`TokenGroup`](./tokengroup.md)                       | Windows token group membership    |
| [`TokenInfo`](./tokeninfo.md)                         | Windows access token information  |

## Enumeration values

### SID name-use values

| Value                 | Meaning                       |
| --------------------- | ----------------------------- |
| `:USER:`              | User SID                      |
| `:GROUP:`             | Group SID                     |
| `:DOMAIN:`            | Domain SID                    |
| `:ALIAS:`             | Alias SID                     |
| `:WELL_KNOWN_GROUP:`  | Well-known group SID          |
| `:DELETED_ACCOUNT:`   | Deleted account SID           |
| `:INVALID:`           | Invalid SID                   |
| `:UNKNOWN:`           | SID of an unknown type        |
| `:COMPUTER:`          | Computer SID                  |
| `:LABEL:`             | Mandatory integrity label SID |
| `:LOGON_SESSION:`     | Logon session SID             |

### ACE type values

| Code | Symbol                             |
| ---- | ---------------------------------- |
| 0    | `:ACCESS_ALLOWED:`                 |
| 1    | `:ACCESS_DENIED:`                  |
| 2    | `:SYSTEM_AUDIT:`                   |
| 3    | `:SYSTEM_ALARM:`                   |
| 4    | `:ACCESS_ALLOWED_COMPOUND:`        |
| 5    | `:ACCESS_ALLOWED_OBJECT:`          |
| 6    | `:ACCESS_DENIED_OBJECT:`           |
| 7    | `:SYSTEM_AUDIT_OBJECT:`            |
| 8    | `:SYSTEM_ALARM_OBJECT:`            |
| 9    | `:ACCESS_ALLOWED_CALLBACK:`        |
| 10   | `:ACCESS_DENIED_CALLBACK:`         |
| 11   | `:ACCESS_ALLOWED_CALLBACK_OBJECT:` |
| 12   | `:ACCESS_DENIED_CALLBACK_OBJECT:`  |
| 13   | `:SYSTEM_AUDIT_CALLBACK:`          |
| 14   | `:SYSTEM_ALARM_CALLBACK:`          |
| 15   | `:SYSTEM_AUDIT_CALLBACK_OBJECT:`   |
| 16   | `:SYSTEM_ALARM_CALLBACK_OBJECT:`   |
| 17   | `:SYSTEM_MANDATORY_LABEL:`         |
| 18   | `:SYSTEM_RESOURCE_ATTRIBUTE:`      |
| 19   | `:SYSTEM_SCOPED_POLICY_ID:`        |
| 20   | `:SYSTEM_PROCESS_TRUST_LABEL:`     |
| 21   | `:SYSTEM_ACCESS_FILTER:`           |

## Functions

### `token_info()`

Returns Windows token information captured for the active VFS context.

#### Returns

[`TokenInfo`](./tokeninfo.md)

#### Errors

- Raises `UnsupportedError` when the active VFS target is Unix.

#### Example

```
if token_info().is_elevated
  echo elevated
```
