# security.windows

The `security.windows` module exposes Windows security types.

## Types

| Type                                                  | Description                       |
| ----------------------------------------------------- | --------------------------------- |
| [`AccessMask`](./access-mask.md)                      | Generic Windows object rights     |
| [`AceFlags`](./ace-flags.md)                          | ACE header flags                  |
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

## Declarative forms

A descriptor, an ACL, and an ACE can each be written as data rather than built
by calling constructors. [`SecDesc`](./secdesc.md) and [`Acl`](./acl.md) stay
strict and accept only built values. [`Ace`](./ace.md) takes the same named
fields as an ACE spec but requires built `Sid`, `AccessMask`, `AceFlags`, and
`uuid.Guid` values. The `sec_desc`, `acl`, and `ace` functions below, and every
parameter that takes a descriptor, ACL, or ACE, also accept the declarative
forms. This is the [`Int`](../../std/int.md) versus `int` distinction: the
capitalized form constructs, the lowercase form coerces.

Unrecognized keys, repeated keys, and entries out of order are errors, not
guesses. Every error names its position in the spec, so a mistake several
levels down reports as `dacl[2].mask`.

Symbols are the declarative surface throughout: any parameter taking a flags
value also takes a symbol or an iterable of symbols, and any parameter taking
a [`Sid`](./sid.md) also takes its canonical string or a symbol naming a
[well-known SID](./sid.md#well-known-sids). The flags types exist for
structural inspection of a value once you have one.

### ACE specs

An ACE spec is a dictionary naming its trustee under exactly one of `allow:`,
`deny:`, or `audit:`. The remaining keys are the options the
[`Ace`](./ace.md) class methods take.

```
ace $
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

### ACL specs

An ACL spec is any iterable of ACE specs or [`Ace`](./ace.md) values, in
packet order. A dictionary is the same sequence with named options alongside
it: entries take the implicit integer keys, and `revision` selects the ACL
revision as it does for the [`Acl`](./acl.md) constructor.

```
acl $
  - allow: :LOCAL_SYSTEM:
    mask: :GENERIC_ALL:
  - allow: :BUILTIN_ADMINISTRATORS:
    mask: :GENERIC_ALL:
```

```
acl $
  revision: :DIRECTORY_SERVICE:
  - allow: $group.sid
    mask: :GENERIC_ALL:
    object_type: $schema_guid
```

### Descriptor specs

A descriptor spec is a dictionary of [`SecDesc`'s component
options](./secdesc.md#component-options), where `dacl` and `sacl` accept ACL
specs and `owner` and `group` accept any trustee form.

```
sec_desc
  owner: :BUILTIN_ADMINISTRATORS:
  dacl_protected: true
  dacl:
    - allow: :LOCAL_SYSTEM:
      mask: :GENERIC_ALL:
```

## Functions

### `ace spec`

Coerces an ACE spec into an [`Ace`](./ace.md).

#### Parameters

| Name   | Type                                           | Description                           |
| ------ | ---------------------------------------------- | ------------------------------------- |
| `spec` | [`Ace`](./ace.md)\|[`Dict`](../../std/dict.md) | ACE spec, or an entry to pass through |

#### Returns

[`Ace`](./ace.md)

#### Example

```
let entry = ace $
  deny: :EVERYONE:
  mask: :GENERIC_WRITE:
```

### `acl spec`

Coerces an ACL spec into an [`Acl`](./acl.md).

#### Parameters

| Name   | Type                                                     | Description                         |
| ------ | -------------------------------------------------------- | ----------------------------------- |
| `spec` | [`Acl`](./acl.md)\|iterable\|[`Dict`](../../std/dict.md) | ACL spec, or a list to pass through |

#### Returns

[`Acl`](./acl.md)

#### Example

```
let dacl = acl $
  - allow: :LOCAL_SYSTEM:
    mask: :GENERIC_ALL:
  - allow: :BUILTIN_ADMINISTRATORS:
    mask: :GENERIC_ALL:
```

### `sec_desc desc? ...options`

Coerces a descriptor spec into a [`SecDesc`](./secdesc.md).

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
