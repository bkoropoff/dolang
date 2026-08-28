# ConcurrencyError

Raised when a concurrent access violation is detected.

## Inherits

- [`RuntimeError`](./runtime-error.md)

## Constructor

### `ConcurrencyError message?`

Builds an error reporting a conflicting concurrent operation.

#### Parameters

| Name      | Type               | Description                            |
| --------- | ------------------ | -------------------------------------- |
| `message` | [`Str`](./str.md)? | detail appended to the generic message |

#### Example

```
throw ConcurrencyError "shared buffer in use"
```
