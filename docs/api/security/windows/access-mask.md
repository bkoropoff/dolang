# AccessMask

Generic Windows object access rights.

## Constructor

### `AccessMask ...rights`

Constructs a mask from symbols or one iterable of symbols.

#### Supported symbols

| Symbol                       | Meaning                                          |
| ---------------------------- | ------------------------------------------------ |
| `:DELETE:`                   | Delete the object                                |
| `:READ_CONTROL:`             | Read its security descriptor, except the SACL    |
| `:WRITE_DAC:`                | Change its discretionary access-control list     |
| `:WRITE_OWNER:`              | Change its owner                                 |
| `:SYNCHRONIZE:`              | Synchronize access to the object                 |
| `:STANDARD_RIGHTS_REQUIRED:` | The standard rights required by most objects     |
| `:STANDARD_RIGHTS_ALL:`      | All standard rights                              |
| `:ACCESS_SYSTEM_SECURITY:`   | Access its system access-control list            |
| `:MAXIMUM_ALLOWED:`          | Request the maximum rights allowed to the caller |
| `:GENERIC_READ:`             | Request object-specific read access              |
| `:GENERIC_WRITE:`            | Request object-specific write access             |
| `:GENERIC_EXECUTE:`          | Request object-specific execute access           |
| `:GENERIC_ALL:`              | Request all object-specific access               |

## Class Methods

### `from_int value`

Constructs a mask from a native integer while preserving unknown bits.

## Fields

### `int`

Returns the native integer mask.

### `specific_rights`

Returns the object-specific low 16 bits.

### `standard_rights`

Returns the standard-rights portion as an `AccessMask`.

### `generic_rights`

Returns the generic-rights portion as an `AccessMask`.

## Methods

### `contains right`

Tests whether all bits represented by a symbol are set.

#### Returns

[`Bool`](../../std/bool.md)

## Operators

`|`, `&`, and `^` combine masks. `~` complements a mask within the supported
bit set. Iteration yields the symbols represented by a mask.
