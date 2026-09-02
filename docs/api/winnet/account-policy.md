# AccountPolicy

Describes the local password and account-lockout policy.

## Fields

### `min_password_length`

Minimum number of characters required in a password. **Type:**
[`Int`](../std/int.md)

### `max_password_age`

Maximum password age, or `nil` when passwords do not expire. **Type:**
[`time.Duration`](../time/duration.md)?

### `min_password_age`

Minimum time before a password can be changed. **Type:**
[`time.Duration`](../time/duration.md)

### `force_logoff`

Delay before forcibly logging off users after their valid logon time expires,
or `nil` when forced logoff is disabled. **Type:**
[`time.Duration`](../time/duration.md)?

### `password_history_length`

Number of previous passwords retained. **Type:** [`Int`](../std/int.md)

### `lockout_duration`

Time a locked account remains locked. **Type:**
[`time.Duration`](../time/duration.md)

### `lockout_observation_window`

Time after which the failed-logon counter resets. **Type:**
[`time.Duration`](../time/duration.md)

### `lockout_threshold`

Failed logons allowed before lockout. Zero disables lockout. **Type:**
[`Int`](../std/int.md)
