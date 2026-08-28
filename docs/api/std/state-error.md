# StateError

Raised when an operation is invalid for the current object or runtime state,
such as using a closed handle or stale reference.

## Inherits

- [`RuntimeError`](./runtime-error.md)

## Constructor

### `StateError message`

Builds an error reporting an operation attempted in the wrong state.

#### Parameters

| Name      | Type              | Description                      |
| --------- | ----------------- | -------------------------------- |
| `message` | [`Str`](./str.md) | description of the invalid state |

#### Example

```
throw StateError "connection already closed"
```
