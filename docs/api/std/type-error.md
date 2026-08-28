# TypeError

Raised when an operation receives a value of the wrong type.

## Inherits

- [`RuntimeError`](./runtime-error.md)

## Constructor

### `TypeError message`

Builds an error reporting a value of the wrong type.

#### Parameters

| Name      | Type              | Description                 |
| --------- | ----------------- | --------------------------- |
| `message` | [`Str`](./str.md) | description of the mismatch |

#### Example

```
throw TypeError "expected Int"
```
