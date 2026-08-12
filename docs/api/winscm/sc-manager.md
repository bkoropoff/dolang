# ScManager

An open handle to the Windows Service Control Manager.

## Methods

### `open_service name :access? func?`

Opens an existing service.

#### Parameters

| Name     | Type                                                            | Description                                             |
| -------- | --------------------------------------------------------------- | ------------------------------------------------------- |
| `name`   | [`Str`](../std/str.md)                                          | Service name                                            |
| `access` | [`ServiceAccessMask`](./service-access-mask.md)\|sym\|iterable? | Access rights (default: `:SERVICE_QUERY_STATUS:`)       |
| `func`   | func?                                                           | Function to run with the service; auto-closes when done |

#### Returns

[`Service`](./service.md) when no `func` is given, otherwise the
result of calling `func`

#### Errors

- [`sys.NotFoundError`](../sys/not-found-error.md) — the service does not exist

### `create_service name :...options func?`

Creates a service.

Omit `display_name` or `binary_path` to pass `NULL` for it to Windows.

#### Parameters

| Name                 | Type                                                                          | Description                                                                     |
| -------------------- | ----------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| `name`               | [`Str`](../std/str.md)                                                        | Service name                                                                    |
| `display_name`       | [`Str`](../std/str.md)?                                                       | Display name                                                                    |
| `binary_path`        | [`Str`](../std/str.md)\|[`fs.windows.Path`](../fs/windows/path.md)\|iterable? | Service executable path or command line                                         |
| `service_type`       | [`ServiceType`](./service-type.md)\|sym\|iterable?                            | Service type (default: `:WIN32_OWN_PROCESS:`)                                   |
| `start_type`         | sym?                                                                          | [Start mode](./index.md#start_type-values) (default: `:DEMAND_START:`)          |
| `error_control`      | sym?                                                                          | [Startup error severity](./index.md#error_control-values) (default: `:NORMAL:`) |
| `access`             | [`ServiceAccessMask`](./service-access-mask.md)\|sym\|iterable?               | Returned handle's rights (default: `:SERVICE_QUERY_STATUS:`)                    |
| `load_order_group`   | [`Str`](../std/str.md)?                                                       | Load-order group                                                                |
| `dependencies`       | iterable?                                                                     | Service or load-order-group names                                               |
| `service_start_name` | [`Str`](../std/str.md)?                                                       | Account name                                                                    |
| `password`           | [`Str`](../std/str.md)?                                                       | Account password                                                                |
| `func`               | func?                                                                         | Function to run with the service; auto-closes when done                         |

##### `binary_path`

A `Str` is passed to Windows verbatim. An absolute `fs.windows.Path` is used
as the executable path and quoted when needed. An iterable supplies command
arguments as `Str` values or absolute `fs.windows.Path` values; each element
is quoted with the Windows command-line rules.

#### Returns

[`Service`](./service.md) when no `func` is given, otherwise the
result of calling `func`

#### Example

```
let service = manager.create_service my-service
  display_name: My Service
  binary_path: [r"C:\Program Files\My Service\service.exe"]
  access: :SERVICE_ALL_ACCESS:
```

### `enumerate_services :service_type? :state_filter?`

Opens a live forward enumeration of matching services. Entries are fetched as
iteration advances.

#### Parameters

| Name           | Type                                               | Description                                    |
| -------------- | -------------------------------------------------- | ---------------------------------------------- |
| `service_type` | [`ServiceType`](./service-type.md)\|sym\|iterable? | Types to include (default: `:WIN32:`)          |
| `state_filter` | sym?                                               | `:ACTIVE:`, `:INACTIVE:`, or `:ALL:` (default) |

#### Returns

Iterable of [`ServiceInfo`](./service-info.md)

### `close()`

Closes the manager. Closing an already-closed manager is a no-op.
