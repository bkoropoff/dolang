# ManagerAccessMask

Service Control Manager access rights.

## Constructor

### `ManagerAccessMask ...rights`

Constructs a mask from symbols or one iterable of symbols.

#### Supported symbols

| Symbol                            | Meaning                                     |
| --------------------------------- | ------------------------------------------- |
| `:SC_MANAGER_CONNECT:`            | Connect to the Service Control Manager      |
| `:SC_MANAGER_CREATE_SERVICE:`     | Create services                             |
| `:SC_MANAGER_ENUMERATE_SERVICE:`  | Enumerate services                          |
| `:SC_MANAGER_LOCK:`               | Lock the service database                   |
| `:SC_MANAGER_QUERY_LOCK_STATUS:`  | Query the service database lock status      |
| `:SC_MANAGER_MODIFY_BOOT_CONFIG:` | Modify boot configuration information       |
| `:SC_MANAGER_ALL_ACCESS:`         | All Service Control Manager-specific rights |

The generic object rights in
[`security.windows.AccessMask`](../security/windows/access-mask.md) are also
supported.

## Fields

### `int`

Returns the complete native mask as an integer, including unknown bits.

## Class Methods

### `from_int value`

Constructs a mask from a native integer while preserving every bit.

## Methods

### `contains right`

Tests whether all bits represented by a symbol are set.

#### Returns

[`Bool`](../std/bool.md)

## Operators

`|`, `&`, and `^` combine masks. `~` complements a mask within the supported
bit set. Iteration yields the symbols represented by a mask.
