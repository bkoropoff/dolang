# AccessMask

Generic Windows object access rights.

## Constructor

### `AccessMask ...rights`

Constructs a mask from symbols or one iterable of symbols.

Supported symbols are `:DELETE:`, `:READ_CONTROL:`, `:WRITE_DAC:`,
`:WRITE_OWNER:`, `:SYNCHRONIZE:`, `:STANDARD_RIGHTS_REQUIRED:`,
`:STANDARD_RIGHTS_ALL:`, `:ACCESS_SYSTEM_SECURITY:`, `:MAXIMUM_ALLOWED:`,
`:GENERIC_READ:`, `:GENERIC_WRITE:`, `:GENERIC_EXECUTE:`, and `:GENERIC_ALL:`.

## Methods

### `contains right`

Tests whether all bits represented by a symbol are set.

**Returns:** [`Bool`](../../std/bool.md)

## Operators

`|`, `&`, and `^` combine masks. `~` complements a mask within the supported
bit set. Iteration yields the symbols represented by a mask.
