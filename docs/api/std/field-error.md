# FieldError

Raised when accessing a nonexistent field on an object.

## Inherits

- [`RuntimeError`](./runtime-error.md)

## Constructor

### `FieldError name`

Builds an error naming a field that does not exist.

#### Parameters

| Name   | Type              | Description                   |
| ------ | ----------------- | ----------------------------- |
| `name` | [`Sym`](./sym.md) | the field that does not exist |

#### Example

```
throw FieldError :width:
```
