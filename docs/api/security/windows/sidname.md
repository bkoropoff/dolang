# `SidName`

Resolved Windows account identity.

## Class Methods

### `lookup value`

Resolves an account name or [`Sid`](./sid.md) in the active VFS target.

#### Parameters

| Name    | Type                                         | Description         |
| ------- | -------------------------------------------- | ------------------- |
| `value` | [`Str`](../../std/str.md)\|[`Sid`](./sid.md) | Account name or SID |

#### Returns

`SidName`

#### Errors

| Exception                                            | Condition                     |
| ---------------------------------------------------- | ----------------------------- |
| [`sys.NotFoundError`](../../sys/not-found-error.md)  | The identity is unmapped      |
| [`UnsupportedError`](../../std/unsupported-error.md) | The active VFS target is Unix |

## Fields

### `sid`

Resolved [`Sid`](./sid.md).

### `name`

Unqualified account name.

### `domain`

Account domain returned by Windows.

### `qualified_name`

The `domain\name` form, or `name` when the domain is empty.

### `kind`

Windows SID name-use classification as an uppercase symbol.

For recognized values, see
[SID name-use values](./index.md#sid-name-use-values).

#### Example

```
let account = SidName.lookup "BUILTIN\\Users"
echo "$account.qualified_name ($account.kind)"
```
