# MissingPosError

Raised when a required positional argument is not provided.

## Inherits

- [`RuntimeError`](./runtime-error.md)

## Constructor

### `MissingPosError index`

Builds an error naming a missing positional item.

#### Parameters

| Name    | Type              | Description                             |
| ------- | ----------------- | --------------------------------------- |
| `index` | [`Int`](./int.md) | zero-based position of the missing item |

#### Example

```
throw MissingPosError 3
```
