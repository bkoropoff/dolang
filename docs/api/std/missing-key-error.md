# MissingKeyError

Raised when a required key argument is not provided.

## Inherits

- [`RuntimeError`](./runtime-error.md)

## Constructor

### `MissingKeyError key`

Builds an error naming a missing key item.

#### Parameters

| Name  | Type | Description     |
| ----- | ---- | --------------- |
| `key` |      | the missing key |

#### Example

```
throw MissingKeyError :host:
```
