# CompileError

Raised on compilation errors.

## Inherits

- [`RuntimeError`](./runtime-error.md)

## Constructor

### `CompileError message`

Builds an error reporting a compilation failure.

#### Parameters

| Name      | Type              | Description                            |
| --------- | ----------------- | -------------------------------------- |
| `message` | [`Str`](./str.md) | description of the compilation failure |

#### Example

```
throw CompileError "unexpected token"
```
