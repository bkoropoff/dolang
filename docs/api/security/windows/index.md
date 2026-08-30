# security.windows

The `security.windows` module exposes Windows security types.

## Types

| Type                                                  | Description                       |
| ----------------------------------------------------- | --------------------------------- |
| [`AccessMask`](./access-mask.md)                      | Generic Windows object rights     |
| [`Ace`](./ace.md)                                     | Windows access-control entry      |
| [`AceFlags`](./ace-flags.md)                          | ACE header flags                  |
| [`Acl`](./acl.md)                                     | Windows access-control list       |
| [`SecDesc`](./secdesc.md)                             | Windows security descriptor       |
| [`SecDescControl`](./secdesc-control.md)              | Security descriptor control flags |
| [`SecInfo`](./sec-info.md)                            | Loaded descriptor components      |
| [`Sid`](./sid.md)                                     | Windows security identifier       |
| [`SidName`](./sidname.md)                             | Resolved Windows account identity |
| [`TokenGroup`](./tokengroup.md)                       | Windows token group membership    |
| [`TokenGroupAttributes`](./token-group-attributes.md) | Token group attributes            |
| [`TokenInfo`](./tokeninfo.md)                         | Windows access token information  |

## Enumeration values

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

## Declarative forms

Security descriptors, ACLs, and ACEs have YAML-like declarative forms as an
manually constructing type instances. Type constructors are strict and require
provided components to already be of the correct type (`Ace`, `Acl`, etc.), but
most other functions and methods will accept a declarative specification in
their place.

Symbols or symbol collections are broadly accepted in lieu of dedicated flag
types. Most parameters taking a [`Sid`](./sid.md) also accept a canonical
string form or a [well-known SID](./sid.md#well-known-sids) symbol.

### ACE Specs

An ACE spec names its trustee under exactly one of `allow:`, `deny:`, or
`audit:`. The remaining fields are the options the [`Ace`](./ace.md) class
methods take. The `ace` function accepts these fields as named arguments; an
ACE nested in an ACL or descriptor uses a dictionary with the same shape.

```
ace
  allow: :BUILTIN_ADMINISTRATORS:
  mask: :GENERIC_ALL:
  flags: [:OBJECT_INHERIT:, :CONTAINER_INHERIT:]
```

| Key                     | Type                                                                    | Description                        |
| ----------------------- | ----------------------------------------------------------------------- | ---------------------------------- |
| `allow`                 | [`Sid`](./sid.md)\|[`Str`](../../std/str.md)\|[`Sym`](../../std/sym.md) | Trustee of an access-allowed entry |
| `deny`                  | [`Sid`](./sid.md)\|[`Str`](../../std/str.md)\|[`Sym`](../../std/sym.md) | Trustee of an access-denied entry  |
| `audit`                 | [`Sid`](./sid.md)\|[`Str`](../../std/str.md)\|[`Sym`](../../std/sym.md) | Trustee of a system-audit entry    |
| `mask`                  | [`AccessMask`](./access-mask.md)                                        | Access mask                        |
| `flags`                 | [`AceFlags`](./ace-flags.md)?                                           | ACE header flags                   |
| `object_type`           | [`uuid.Guid`](../../uuid/guid.md)?                                      | Object type                        |
| `inherited_object_type` | [`uuid.Guid`](../../uuid/guid.md)?                                      | Inherited object type              |
| `callback`              | [`Bool`](../../std/bool.md)?                                            | Build a callback ACE               |
| `application_data`      | [`Bin`](../../std/bin.md)?                                              | Trailing application data          |
| `successful`            | [`Bool`](../../std/bool.md)?                                            | Audit successful access            |
| `failed`                | [`Bool`](../../std/bool.md)?                                            | Audit failed access                |

`mask` is required. `successful` and `failed` apply only to `audit`, which
requires at least one of them.

### ACL Specs

An ACL spec is any iterable of ACE specs or [`Ace`](./ace.md) values, in packet
order. The `acl` function accepts the entries as separate positional
arguments, with `revision:` as an optional key argument.

```
acl
  - allow: :LOCAL_SYSTEM:
    mask: :GENERIC_ALL:
  - allow: :BUILTIN_ADMINISTRATORS:
    mask: :GENERIC_ALL:
```

```
acl
  revision: :DIRECTORY_SERVICE:
  - allow: $group.sid
    mask: :GENERIC_ALL:
    object_type: $schema_guid
```

### Descriptor Specs

A descriptor spec is a dictionary of [`SecDesc`'s component
options](./secdesc.md#component-options), where `dacl` and `sacl` accept ACL
specs and `owner` and `group` accept any trustee form. The `sec_desc` function
accepts these components as key arguments.

```
sec_desc
  owner: :BUILTIN_ADMINISTRATORS:
  dacl_protected: true
  dacl:
    - allow: :LOCAL_SYSTEM:
      mask: :GENERIC_ALL:
```

## Functions

### `ace :allow? :deny? :audit? :mask ...options`

Constructs an [`Ace`](./ace.md). This function will implicitly coerce
arguments that the type constructor would not accept.

#### Parameters

See [ACE specs](#ace-specs) for the accepted arguments.

#### Returns

[`Ace`](./ace.md)

#### Example

```
let entry = ace
  deny: :EVERYONE:
  mask: :GENERIC_WRITE:
```

### `acl ...aces :revision?`

Constructs an [`Acl`](./acl.md). This funtion will implicitly coerce
arguments that the type constructor would not accept.

#### Parameters

| Name       | Type                              | Description                          |
| ---------- | --------------------------------- | ------------------------------------ |
| `aces`     | *                                 | ACE values or specs, in packet order |
| `revision` | `:BASIC:`\|`:DIRECTORY_SERVICE:`? | Native ACL revision                  |

#### Returns

[`Acl`](./acl.md)

#### Example

```
let dacl = acl
  - allow: :LOCAL_SYSTEM:
    mask: :GENERIC_ALL:
  - allow: :BUILTIN_ADMINISTRATORS:
    mask: :GENERIC_ALL:
```

### `sec_desc desc? ...options`

Constructs a [`SecDesc`](./secdesc.md). This function will implicitly coerce
arguments that the type constructor would not accept.

#### Parameters

| Name   | Type                                                                               | Description                 |
| ------ | ---------------------------------------------------------------------------------- | --------------------------- |
| `desc` | [`SecDesc`](./secdesc.md)\|[`Bin`](../../std/bin.md)\|[`Dict`](../../std/dict.md)? | Descriptor, packet, or spec |

The [component options](./secdesc.md#component-options) may be passed as
keyword arguments instead of, or alongside, `desc`. Given both, they amend
`desc` the way [`with`](./secdesc.md#with-options) does.

#### Returns

[`SecDesc`](./secdesc.md)

#### Errors

Raises `ValueError` when neither a descriptor nor any component option is
given.

#### Example

```
let descriptor = sec_desc
  owner: :BUILTIN_ADMINISTRATORS:
  dacl_protected: true
  dacl:
    - allow: :LOCAL_SYSTEM:
      mask: :GENERIC_ALL:

let unprotected = sec_desc $descriptor dacl_protected: false
```

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
