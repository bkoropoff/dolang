# `SidName`

Resolved Windows account identity.

## Class Methods

### `lookup value`

Resolves an account name or SID in the active VFS target.

A [`Str`](../../std/str.md) is always an account name, never a SID in its
canonical string form: a SID is given as a [`Sid`](./sid.md), its native
binary representation, or a symbol naming a
[well-known SID](./sid.md#well-known-sids).

#### Parameters

| Name    | Type                                                                                               | Description         |
| ------- | -------------------------------------------------------------------------------------------------- | ------------------- |
| `value` | [`Str`](../../std/str.md)\|[`Sid`](./sid.md)\|[`Bin`](../../std/bin.md)\|[`Sym`](../../std/sym.md) | Account name or SID |

#### Returns

`SidName`

#### Errors

| Exception                                            | Condition                     |
| ---------------------------------------------------- | ----------------------------- |
| [`sys.NotFoundError`](../../sys/not-found-error.md)  | The identity is unmapped      |
| [`UnsupportedError`](../../std/unsupported-error.md) | The active VFS target is Unix |

## Fields

### `domain`

Account domain returned by Windows.

### `kind`

Windows SID name-use classification as an uppercase symbol.

For recognized values, see
[SID name-use values](./index.md#sid-name-use-values).

#### Example

```
let account = SidName.lookup BUILTIN\Users
echo "$account.qualified_name ($account.kind)"
```

### `name`

Unqualified account name.

### `qualified_name`

The `domain\name` form, or `name` when the domain is empty.

### `sid`

Resolved [`Sid`](./sid.md).
