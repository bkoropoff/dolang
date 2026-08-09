# ServiceConfig

Immutable snapshot returned by [`Service.config()`](./service.md#config).

## Fields

### `service_type`

Service type flags. **Type:** [`ServiceType`](./service-type.md)

### `start_type`

Service start mode. **Type:** sym|[`Int`](../std/int.md)

Recognized values are `:BOOT_START:`, `:SYSTEM_START:`, `:AUTO_START:`,
`:DEMAND_START:`, and `:DISABLED:`. An unrecognized native value is returned
as an `Int`.

### `error_control`

Startup error severity. **Type:** sym|[`Int`](../std/int.md)

Recognized values are `:IGNORE:`, `:NORMAL:`, `:SEVERE:`, and `:CRITICAL:`.
An unrecognized native value is returned as an `Int`.

### `binary_path`

Service command line. **Type:** [`Str`](../std/str.md)

### `load_order_group`

Load-order group. **Type:** [`Str`](../std/str.md)|`nil`

### `tag_id`

Load-order tag. **Type:** [`Int`](../std/int.md)

### `dependencies`

Service and load-order-group dependencies. **Type:** [`Tuple`](../std/tuple.md)
of [`Str`](../std/str.md)

### `service_start_name`

Account name. **Type:** [`Str`](../std/str.md)

### `display_name`

Display name. **Type:** [`Str`](../std/str.md)
