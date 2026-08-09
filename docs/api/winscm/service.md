# Service

An open Windows service handle.

## Methods

### `delete()`

Marks the service for deletion.

### `close()`

Closes the service. Closing an already-closed service is a no-op.

### `start ...args`

Starts the service with optional string arguments.

**Parameters:**

| Name   | Type                        | Description       |
| ------ | --------------------------- | ----------------- |
| `args` | [`Str`](../std/str.md)*     | Service arguments |

### `control control`

Sends a control request and returns the resulting status.

**Parameters:**

| Name      | Type | Description                                                       |
| --------- | ---- | ----------------------------------------------------------------- |
| `control` | sym  | `:STOP:`, `:PAUSE:`, `:CONTINUE:`, or `:INTERROGATE:`             |

**Returns:** [`Status`](./status.md)

### `query_status()`

Fetches the current service status.

**Returns:** [`Status`](./status.md)

### `config()`

Fetches the current service configuration.

**Returns:** Immutable [`ServiceConfig`](./service-config.md) snapshot

### `set_config ...options`

Updates the supplied configuration fields. Omitted fields are unchanged.

**Parameters:**

| Name                 | Type                                               | Description                       |
| -------------------- | -------------------------------------------------- | --------------------------------- |
| `service_type`       | [`ServiceType`](./service-type.md)\|sym\|iterable? | Service type flags                |
| `start_type`         | sym?                                               | Service start mode                |
| `error_control`      | sym?                                               | Startup error severity            |
| `binary_path`        | [`Str`](../std/str.md)?                            | Service command line              |
| `load_order_group`   | [`Str`](../std/str.md)?                            | Load-order group                  |
| `dependencies`       | iterable?                                          | Service or load-order-group names |
| `service_start_name` | [`Str`](../std/str.md)?                            | Account name                      |
| `password`           | [`Str`](../std/str.md)?                            | Account password                  |
| `display_name`       | [`Str`](../std/str.md)?                            | Display name                      |

`start_type` accepts `:BOOT_START:`, `:SYSTEM_START:`, `:AUTO_START:`,
`:DEMAND_START:`, or `:DISABLED:`. `error_control` accepts `:IGNORE:`,
`:NORMAL:`, `:SEVERE:`, or `:CRITICAL:`.

```
service.set_config
  start_type: :AUTO_START:
  dependencies: (tuple "RpcSs" "EventLog")
```

### `wait_for_status_change mask`

Waits until one of the requested service status changes occurs.

**Parameters:**

| Name   | Type                                            | Description        |
| ------ | ----------------------------------------------- | ------------------ |
| `mask` | [`NotifyMask`](./notify-mask.md)\|sym\|iterable | Changes to observe |

**Returns:** [`Status`](./status.md)

### `sec_desc :owner? :group? :dacl? :sacl?`

Gets selected parts of the service's Windows security descriptor. Owner,
group, and DACL default to `true`; SACL defaults to `false`.

**Parameters:**

| Name    | Type                     | Description                |
| ------- | ------------------------ | -------------------------- |
| `owner` | [`Bool`](../std/bool.md) | Load the owner SID         |
| `group` | [`Bool`](../std/bool.md) | Load the primary group SID |
| `dacl`  | [`Bool`](../std/bool.md) | Load the discretionary ACL |
| `sacl`  | [`Bool`](../std/bool.md) | Load the system ACL        |

**Returns:** [`security.windows.SecDesc`](../security/windows/secdesc.md)

### `set_sec_desc desc`

Applies the components selected by a Windows security descriptor's `mask`.

**Parameters:**

| Name   | Type                                                         | Description                  |
| ------ | ------------------------------------------------------------ | ---------------------------- |
| `desc` | [`security.windows.SecDesc`](../security/windows/secdesc.md) | Security descriptor to apply |
