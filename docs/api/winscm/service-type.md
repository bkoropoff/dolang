# ServiceType

Windows service type flags.

## Constructor

### `ServiceType ...types`

Constructs a value from symbols or one iterable of symbols.

#### Supported symbols

| Symbol                  | Meaning                                             |
| ----------------------- | --------------------------------------------------- |
| `:KERNEL_DRIVER:`       | Kernel-mode device driver                           |
| `:FILE_SYSTEM_DRIVER:`  | File-system driver                                  |
| `:WIN32_OWN_PROCESS:`   | Win32 service running in its own process            |
| `:WIN32_SHARE_PROCESS:` | Win32 service sharing a process with other services |
| `:INTERACTIVE_PROCESS:` | Service that can interact with the desktop          |
| `:DRIVER:`              | Either kind of driver                               |
| `:WIN32:`               | Either kind of Win32 service                        |

## Methods

### `contains service_type`

Tests whether all bits represented by a symbol are set.

#### Returns

[`Bool`](../std/bool.md)

## Operators

`|`, `&`, and `^` combine values. `~` complements a value within the supported
bit set. Iteration yields the symbols represented by a value.
