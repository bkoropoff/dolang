# ValueError

Raised when a value has an acceptable type but invalid contents, range, or
meaning for the operation.

## Inherits

- [`RuntimeError`](./runtime-error.md)

## Constructor

### `ValueError message`

Builds an error reporting a value of the right type but unusable contents.

#### Parameters

| Name      | Type              | Description                               |
| --------- | ----------------- | ----------------------------------------- |
| `message` | [`Str`](./str.md) | description of the problem with the value |

#### Example

```
throw ValueError "expected a positive count"
```
