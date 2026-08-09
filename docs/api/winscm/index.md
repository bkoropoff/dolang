# winscm

The `winscm` module manages Windows services.

This API uses the VFS in scope for the current strand, so it works through
remote and elevated Windows VFS contexts. Other targets raise
[`sys.UnsupportedError`](../sys/unsupported-error.md).

## Types

| Type                                                        | Description                         |
| ----------------------------------------------------------- | ----------------------------------- |
| [`ScManager`](./sc-manager.md)                              | Open Service Control Manager handle |
| [`Service`](./service.md)                                   | Open service handle                 |
| [`ServiceConfig`](./service-config.md)                      | Immutable configuration snapshot    |
| [`Status`](./status.md)                                     | Service status snapshot             |
| [`ServiceInfo`](./service-info.md)                          | Enumerated service entry            |
| [`ManagerAccessMask`](./manager-access-mask.md)             | Manager access rights               |
| [`ServiceAccessMask`](./service-access-mask.md)             | Service access rights               |
| [`ServiceType`](./service-type.md)                          | Service type flags                  |
| [`NotifyMask`](./notify-mask.md)                            | Status-change notification flags    |
| [`ServiceControlsAccepted`](./service-controls-accepted.md) | Accepted service-control flags      |

## Functions

### `open access? func?`

Opens the local Service Control Manager.

**Parameters:**

| Name     | Type                                                            | Description                                             |
| -------- | --------------------------------------------------------------- | ------------------------------------------------------- |
| `access` | [`ManagerAccessMask`](./manager-access-mask.md)\|sym\|iterable? | Access rights (default: `:SC_MANAGER_CONNECT:`)         |
| `func`   | func?                                                           | Callable to run with the manager; auto-closes when done |

**Returns:** [`ScManager`](./sc-manager.md) when no `func` is given,
otherwise the result of calling `func`

```
winscm.open access: :SC_MANAGER_ENUMERATE_SERVICE: do |manager|
  for service = manager.enumerate_services()
    echo $service.name
```
