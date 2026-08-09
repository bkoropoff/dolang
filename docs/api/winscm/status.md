# Status

Snapshot of a service's current status.

## Fields

### `service_type`

Service type flags. **Type:** [`ServiceType`](./service-type.md)

### `current_state`

Current state. **Type:** sym|[`Int`](../std/int.md)

For recognized values, see
[service state values](./index.md#service-state-values). An unrecognized native
value is returned as an `Int`.

### `controls_accepted`

Controls accepted by the service. **Type:**
[`ServiceControlsAccepted`](./service-controls-accepted.md)

### `win32_exit_code`

Service exit code. **Type:** [`Int`](../std/int.md)

### `service_specific_exit_code`

Service-defined exit code. **Type:** [`Int`](../std/int.md)

### `check_point`

Progress checkpoint for a pending operation. **Type:** [`Int`](../std/int.md)

### `wait_hint`

Estimated time required for a pending operation. **Type:**
[`Int`](../std/int.md)

### `process_id`

Service process identifier. **Type:** [`Int`](../std/int.md)
