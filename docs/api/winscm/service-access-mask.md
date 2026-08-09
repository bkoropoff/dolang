# ServiceAccessMask

Windows service access rights.

## Constructor

### `ServiceAccessMask ...rights`

Constructs a mask from symbols or one iterable of symbols.

Supported service-specific symbols are `:SERVICE_QUERY_CONFIG:`,
`:SERVICE_CHANGE_CONFIG:`, `:SERVICE_QUERY_STATUS:`,
`:SERVICE_ENUMERATE_DEPENDENTS:`, `:SERVICE_START:`, `:SERVICE_STOP:`,
`:SERVICE_PAUSE_CONTINUE:`, `:SERVICE_INTERROGATE:`,
`:SERVICE_USER_DEFINED_CONTROL:`, and `:SERVICE_ALL_ACCESS:`.

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
