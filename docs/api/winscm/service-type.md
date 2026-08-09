# ServiceType

Windows service type flags.

## Constructor

### `ServiceType ...types`

Constructs a value from symbols or one iterable of symbols.

Supported symbols are `:KERNEL_DRIVER:`, `:FILE_SYSTEM_DRIVER:`,
`:WIN32_OWN_PROCESS:`, `:WIN32_SHARE_PROCESS:`, `:INTERACTIVE_PROCESS:`,
`:DRIVER:`, and `:WIN32:`.

## Methods

### `contains service_type`

Tests whether all bits represented by a symbol are set.

**Returns:** [`Bool`](../std/bool.md)

## Operators

`|`, `&`, and `^` combine values. `~` complements a value within the supported
bit set. Iteration yields the symbols represented by a value.
