# ServiceControlsAccepted

Controls accepted by a running service.

## Constructor

### `ServiceControlsAccepted ...controls`

Constructs a value from symbols or one iterable of symbols.

#### Supported symbols

| Symbol                    | Accepts control requests to                       |
| ------------------------- | ------------------------------------------------- |
| `:STOP:`                  | Stop the service                                  |
| `:PAUSE_CONTINUE:`        | Pause or continue the service                     |
| `:SHUTDOWN:`              | Prepare for system shutdown                       |
| `:PARAMCHANGE:`           | Reload configuration parameters                   |
| `:NETBINDCHANGE:`         | Handle a network binding change                   |
| `:HARDWAREPROFILECHANGE:` | Handle a hardware-profile change                  |
| `:POWEREVENT:`            | Handle a power event                              |
| `:SESSIONCHANGE:`         | Handle a terminal-services session change         |
| `:PRESHUTDOWN:`           | Prepare for system shutdown before other services |
| `:TIMECHANGE:`            | Handle a system-time change                       |
| `:TRIGGEREVENT:`          | Handle a service-trigger event                    |

## Fields

### `int`

Returns the complete native mask as an integer, including unknown bits.

## Class Methods

### `from_int value`

Constructs a mask from a native integer while preserving every bit.

## Methods

### `contains control`

Tests whether all bits represented by a symbol are set.

#### Returns

[`Bool`](../std/bool.md)

## Operators

`|`, `&`, and `^` combine values. `~` complements a value within the supported
bit set. Iteration yields the symbols represented by a value.
