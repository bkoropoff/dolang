# ServiceConfig

Immutable snapshot returned by [`Service.config()`](./service.md#config).

## Fields

### `binary_path`

Service command line. **Type:** [`Str`](../std/str.md)

### `dependencies`

Service and load-order-group dependencies. **Type:** [`Tuple`](../std/tuple.md)
of [`Str`](../std/str.md)

### `display_name`

Display name. **Type:** [`Str`](../std/str.md)

### `error_control`

Startup error severity. **Type:** sym|[`Int`](../std/int.md)

For recognized values, see
[`error_control` values](./index.md#error_control-values). An unrecognized
native value is returned as an `Int`.

### `load_order_group`

Load-order group. **Type:** [`Str`](../std/str.md)|`nil`

### `service_start_name`

Account name. **Type:** [`Str`](../std/str.md)

### `service_type`

Service type flags. **Type:** [`ServiceType`](./service-type.md)

### `start_type`

Service start mode. **Type:** sym|[`Int`](../std/int.md)

For recognized values, see [`start_type` values](./index.md#start_type-values).
An unrecognized native value is returned as an `Int`.

### `tag_id`

Load-order tag. **Type:** [`Int`](../std/int.md)
