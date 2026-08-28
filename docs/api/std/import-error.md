# ImportError

Raised when a module import fails (e.g. module not found).

## Inherits

- [`RuntimeError`](./runtime-error.md)

## Constructor

### `ImportError name`

Builds an error naming a module that could not be found.

#### Parameters

| Name   | Type              | Description                        |
| ------ | ----------------- | ---------------------------------- |
| `name` | [`Str`](./str.md) | the module that could not be found |

#### Example

```
throw ImportError "pkg.mod"
```
