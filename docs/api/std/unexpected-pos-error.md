# UnexpectedPosError

Raised when an unexpected positional argument is passed.

## Inherits

- [`RuntimeError`](./runtime-error.md)

## Constructor

### `UnexpectedPosError index`

Builds an error naming an unexpected positional item.

#### Parameters

| Name    | Type              | Description                                |
| ------- | ----------------- | ------------------------------------------ |
| `index` | [`Int`](./int.md) | zero-based position of the unexpected item |

#### Example

```
throw UnexpectedPosError 2
```
