# ServiceControlsAccepted

Controls accepted by a running service.

## Constructor

### `ServiceControlsAccepted ...controls`

Constructs a value from symbols or one iterable of symbols.

Supported symbols are `:STOP:`, `:PAUSE_CONTINUE:`, `:SHUTDOWN:`,
`:PARAMCHANGE:`, `:NETBINDCHANGE:`, `:HARDWAREPROFILECHANGE:`, `:POWEREVENT:`,
`:SESSIONCHANGE:`, `:PRESHUTDOWN:`, `:TIMECHANGE:`, and `:TRIGGEREVENT:`.

## Methods

### `contains control`

Tests whether all bits represented by a symbol are set.

**Returns:** [`Bool`](../std/bool.md)

## Operators

`|`, `&`, and `^` combine values. `~` complements a value within the supported
bit set. Iteration yields the symbols represented by a value.
