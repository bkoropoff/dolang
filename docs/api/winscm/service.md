# Service

An open Windows service handle.

## Methods

### `delete()`

Marks the service for deletion.

### `close()`

Closes the service. Closing an already-closed service is a no-op.

### `start ...args`

Starts the service with optional string arguments.

#### Parameters

| Name   | Type                        | Description       |
| ------ | --------------------------- | ----------------- |
| `args` | [`Str`](../std/str.md)*     | Service arguments |

### `control control`

Sends a control request and returns the resulting status.

#### Parameters

| Name      | Type | Description                                  |
| --------- | ---- | -------------------------------------------- |
| `control` | sym  | [Control request](./index.md#control-values) |

#### Returns

[`Status`](./status.md)

### `query_status()`

Fetches the current service status.

#### Returns

[`Status`](./status.md)

### `config()`

Fetches the current service configuration.

#### Returns

Immutable [`ServiceConfig`](./service-config.md) snapshot

### `set_config :...options`

Updates the supplied configuration fields. Omitted fields are unchanged.

#### Parameters

| Name                 | Type                                                                          | Description                       |
| -------------------- | ----------------------------------------------------------------------------- | --------------------------------- |
| `service_type`       | [`ServiceType`](./service-type.md)\|sym\|iterable?                            | Service type flags                |
| `start_type`         | sym?                                                                          | Service start mode                |
| `error_control`      | sym?                                                                          | Startup error severity            |
| `binary_path`        | [`Str`](../std/str.md)\|[`fs.windows.Path`](../fs/windows/path.md)\|iterable? | Service command line              |
| `load_order_group`   | [`Str`](../std/str.md)?                                                       | Load-order group                  |
| `dependencies`       | iterable?                                                                     | Service or load-order-group names |
| `service_start_name` | [`Str`](../std/str.md)?                                                       | Account name                      |
| `password`           | [`Str`](../std/str.md)?                                                       | Account password                  |
| `display_name`       | [`Str`](../std/str.md)?                                                       | Display name                      |

For `start_type` and `error_control` values, see
[`winscm` enumeration values](./index.md#enumeration-values). `binary_path`
uses the [`ScManager.create_service` rules](./sc-manager.md#binary_path).

#### Example

```
service.set_config
  start_type: :AUTO_START:
  dependencies: (tuple "RpcSs" "EventLog")
```

### `wait_for_status_change mask`

Waits until one of the requested service status changes occurs.

#### Parameters

| Name   | Type                                            | Description        |
| ------ | ----------------------------------------------- | ------------------ |
| `mask` | [`NotifyMask`](./notify-mask.md)\|sym\|iterable | Changes to observe |

#### Returns

[`Status`](./status.md)

### `sec_desc :owner? :group? :dacl? :sacl?`

Gets selected parts of the service's Windows security descriptor.

#### Parameters

| Name    | Type                      | Description                                  |
| ------- | ------------------------- | -------------------------------------------- |
| `owner` | [`Bool`](../std/bool.md)? | Load the owner SID (default: `true`)         |
| `group` | [`Bool`](../std/bool.md)? | Load the primary group SID (default: `true`) |
| `dacl`  | [`Bool`](../std/bool.md)? | Load the discretionary ACL (default: `true`) |
| `sacl`  | [`Bool`](../std/bool.md)? | Load the system ACL (default: `false`)       |

#### Returns

[`security.windows.SecDesc`](../security/windows/secdesc.md)

### `set_sec_desc desc? ...options`

Applies the components selected by a Windows security descriptor's `mask`.

#### Parameters

| Name   | Type                                                                                                            | Description                 |
| ------ | --------------------------------------------------------------------------------------------------------------- | --------------------------- |
| `desc` | [`security.windows.SecDesc`](../security/windows/secdesc.md)\|[`Bin`](../std/bin.md)\|[`Dict`](../std/dict.md)? | Descriptor, packet, or spec |

The descriptor's
[component options](../security/windows/secdesc.md#component-options) may be
passed as keyword arguments instead of, or alongside, `desc`, exactly as
[`sec_desc`](../security/windows/index.md#sec_desc-desc-options) accepts them.
