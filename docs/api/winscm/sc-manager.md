# ScManager

An open handle to the Windows Service Control Manager.

## Methods

### `open_service name access? func?`

Opens an existing service.

**Parameters:**

| Name     | Type                                                            | Description                                             |
| -------- | --------------------------------------------------------------- | ------------------------------------------------------- |
| `name`   | [`Str`](../std/str.md)                                          | Service name                                            |
| `access` | [`ServiceAccessMask`](./service-access-mask.md)\|sym\|iterable? | Access rights (default: `:SERVICE_QUERY_STATUS:`)       |
| `func`   | func?                                                           | Callable to run with the service; auto-closes when done |

**Returns:** [`Service`](./service.md) when no `func` is given, otherwise the
result of calling `func`

**Errors:**

- [`sys.NotFoundError`](../sys/not-found-error.md) — the service does not exist

### `create_service name display_name binary_path ...options func?`

Creates a service.

**Parameters:**

| Name                 | Type                                                            | Description                                                  |
| -------------------- | --------------------------------------------------------------- | ------------------------------------------------------------ |
| `name`               | [`Str`](../std/str.md)                                          | Service name                                                 |
| `display_name`       | [`Str`](../std/str.md)                                          | Display name                                                 |
| `binary_path`        | [`Str`](../std/str.md)                                          | Service command line                                         |
| `service_type`       | [`ServiceType`](./service-type.md)\|sym\|iterable?              | Service type (default: `:WIN32_OWN_PROCESS:`)                |
| `start_type`         | sym?                                                            | Start mode (default: `:DEMAND_START:`)                       |
| `error_control`      | sym?                                                            | Startup error severity (default: `:NORMAL:`)                 |
| `access`             | [`ServiceAccessMask`](./service-access-mask.md)\|sym\|iterable? | Returned handle's rights (default: `:SERVICE_QUERY_STATUS:`) |
| `load_order_group`   | [`Str`](../std/str.md)?                                         | Load-order group                                             |
| `dependencies`       | iterable?                                                       | Service or load-order-group names                            |
| `service_start_name` | [`Str`](../std/str.md)?                                         | Account name                                                 |
| `password`           | [`Str`](../std/str.md)?                                         | Account password                                             |
| `func`               | func?                                                           | Callable to run with the service; auto-closes when done      |

`start_type` accepts `:BOOT_START:`, `:SYSTEM_START:`, `:AUTO_START:`,
`:DEMAND_START:`, or `:DISABLED:`. `error_control` accepts `:IGNORE:`,
`:NORMAL:`, `:SEVERE:`, or `:CRITICAL:`.

**Returns:** [`Service`](./service.md) when no `func` is given, otherwise the
result of calling `func`

```
let service = manager.create_service
  my-service
  "My Service"
  r"C:\Program Files\My Service\service.exe"
  access: :SERVICE_ALL_ACCESS:
```

### `enumerate_services :service_type? :state_filter?`

Fetches a snapshot of matching services.

**Parameters:**

| Name           | Type                                               | Description                                    |
| -------------- | -------------------------------------------------- | ---------------------------------------------- |
| `service_type` | [`ServiceType`](./service-type.md)\|sym\|iterable? | Types to include (default: `:WIN32:`)          |
| `state_filter` | sym?                                               | `:ACTIVE:`, `:INACTIVE:`, or `:ALL:` (default) |

**Returns:** Iterable snapshot of [`ServiceInfo`](./service-info.md)

### `close()`

Closes the manager. Closing an already-closed manager is a no-op.
