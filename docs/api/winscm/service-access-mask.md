# ServiceAccessMask

Windows service access rights.

## Constructor

### `ServiceAccessMask ...rights`

Constructs a mask from symbols or one iterable of symbols.

#### Supported symbols

| Symbol                              | Meaning                                         |
| ----------------------------------- | ----------------------------------------------- |
| `:SERVICE_QUERY_CONFIG:`            | Query the service configuration                 |
| `:SERVICE_CHANGE_CONFIG:`           | Change the service configuration                |
| `:SERVICE_QUERY_STATUS:`            | Query the current service status                |
| `:SERVICE_ENUMERATE_DEPENDENTS:`    | Enumerate dependent services                    |
| `:SERVICE_START:`                   | Start the service                               |
| `:SERVICE_STOP:`                    | Stop the service                                |
| `:SERVICE_PAUSE_CONTINUE:`          | Pause or resume the service                     |
| `:SERVICE_INTERROGATE:`             | Request that the service report its status      |
| `:SERVICE_USER_DEFINED_CONTROL:`    | Send user-defined control codes to the service  |
| `:SERVICE_ALL_ACCESS:`              | All service-specific rights                     |

The generic object rights in
[`security.windows.AccessMask`](../security/windows/access-mask.md) are also
supported.

## Methods

### `contains right`

Tests whether all bits represented by a symbol are set.

#### Returns

[`Bool`](../std/bool.md)

## Operators

`|`, `&`, and `^` combine masks. `~` complements a mask within the supported
bit set. Iteration yields the symbols represented by a mask.
