# CyclicImportError

Raised when a cyclic module dependency is detected.

## Inherits

- [`RuntimeError`](./runtime-error.md)

## Constructor

### `CyclicImportError name`

Builds an error naming the module at which an import cycle was detected.

#### Parameters

| Name   | Type              | Description                                |
| ------ | ----------------- | ------------------------------------------ |
| `name` | [`Str`](./str.md) | the module at which the cycle was detected |

#### Example

```
throw CyclicImportError "pkg.mod"
```
