# ManagerAccessMask

Service Control Manager access rights.

## Constructor

### `ManagerAccessMask ...rights`

Constructs a mask from symbols or one iterable of symbols.

Supported manager-specific symbols are `:SC_MANAGER_CONNECT:`,
`:SC_MANAGER_CREATE_SERVICE:`, `:SC_MANAGER_ENUMERATE_SERVICE:`,
`:SC_MANAGER_LOCK:`, `:SC_MANAGER_QUERY_LOCK_STATUS:`,
`:SC_MANAGER_MODIFY_BOOT_CONFIG:`, and `:SC_MANAGER_ALL_ACCESS:`.

The generic rights `:DELETE:`, `:READ_CONTROL:`, `:WRITE_DAC:`,
`:WRITE_OWNER:`, `:SYNCHRONIZE:`, `:STANDARD_RIGHTS_REQUIRED:`,
`:STANDARD_RIGHTS_ALL:`, `:ACCESS_SYSTEM_SECURITY:`, `:MAXIMUM_ALLOWED:`,
`:GENERIC_READ:`, `:GENERIC_WRITE:`, `:GENERIC_EXECUTE:`, and `:GENERIC_ALL:`
are also supported.

## Methods

### `contains right`

Tests whether all bits represented by a symbol are set.

**Returns:** [`Bool`](../std/bool.md)

## Operators

`|`, `&`, and `^` combine masks. `~` complements a mask within the supported
bit set. Iteration yields the symbols represented by a mask.
