# TimedOutError

Raised when a strand times out. Timeout is cooperative and is observed at
suspend or interrupt-check points.

## Inherits

- [`RuntimeError`](./runtime-error.md)

## Constructor

### `TimedOutError`

Builds an error reporting that a strand timed out.

#### Example

```
throw TimedOutError()
```
