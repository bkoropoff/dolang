# winscm

The `winscm` module manages Windows services.

This API uses the VFS in scope for the current strand, so it works through
remote and elevated Windows VFS contexts. Other targets raise
[`sys.UnsupportedError`](../sys/unsupported-error.md).

## Types

| Type                                                        | Description                         |
| ----------------------------------------------------------- | ----------------------------------- |
| [`ManagerAccessMask`](./manager-access-mask.md)             | Manager access rights               |
| [`NotifyMask`](./notify-mask.md)                            | Status-change notification flags    |
| [`ScManager`](./sc-manager.md)                              | Open Service Control Manager handle |
| [`Service`](./service.md)                                   | Open service handle                 |
| [`ServiceAccessMask`](./service-access-mask.md)             | Service access rights               |
| [`ServiceConfig`](./service-config.md)                      | Immutable configuration snapshot    |
| [`ServiceControlsAccepted`](./service-controls-accepted.md) | Accepted service-control flags      |
| [`ServiceInfo`](./service-info.md)                          | Enumerated service entry            |
| [`ServiceType`](./service-type.md)                          | Service type flags                  |
| [`Status`](./status.md)                                     | Service status snapshot             |

## Enumeration values

### `control` values

| Value           | Meaning                                 |
| --------------- | --------------------------------------- |
| `:STOP:`        | Requests that the service stop          |
| `:PAUSE:`       | Requests that the service pause         |
| `:CONTINUE:`    | Requests that a paused service resume   |
| `:INTERROGATE:` | Requests that the service report status |

### `error_control` values

| Value        | Meaning                                                     |
| ------------ | ----------------------------------------------------------- |
| `:IGNORE:`   | Logs the error and continues startup                        |
| `:NORMAL:`   | Logs the error, displays a message, and continues startup   |
| `:SEVERE:`   | Restarts with the last known-good configuration if possible |
| `:CRITICAL:` | Restarts with it; fails startup if that restart fails       |

### Service state values

| Symbol              | Meaning                        |
| ------------------- | ------------------------------ |
| `:STOPPED:`         | Not running                    |
| `:START_PENDING:`   | Starting                       |
| `:STOP_PENDING:`    | Stopping                       |
| `:RUNNING:`         | Running                        |
| `:CONTINUE_PENDING:`| Resuming from the paused state |
| `:PAUSE_PENDING:`   | Pausing                        |
| `:PAUSED:`          | Paused                         |

### `start_type` values

| Value            | Meaning                                       |
| ---------------- | --------------------------------------------- |
| `:BOOT_START:`   | Starts during boot before the driver is ready |
| `:SYSTEM_START:` | Starts during kernel initialization           |
| `:AUTO_START:`   | Starts automatically during system startup    |
| `:DEMAND_START:` | Starts when requested                         |
| `:DISABLED:`     | Cannot be started                             |

## Functions

### `open :access? func?`

Opens the local Service Control Manager.

#### Parameters

| Name     | Type                                                            | Description                                             |
| -------- | --------------------------------------------------------------- | ------------------------------------------------------- |
| `access` | [`ManagerAccessMask`](./manager-access-mask.md)\|sym\|iterable? | Access rights (default: `:SC_MANAGER_CONNECT:`)         |
| `func`   | `Func`?                                                         | Function to run with the manager; auto-closes when done |

#### Returns

[`ScManager`](./sc-manager.md) when no `func` is given,
otherwise the result of calling `func`

#### Example

```
winscm.open access: :SC_MANAGER_ENUMERATE_SERVICE: do |manager|
  for service = manager.enumerate_services()
    echo $service.name
```
