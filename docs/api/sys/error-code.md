# ErrorCode

Native system error code.

This base type is not directly constructible. Use a platform-specific code
type.

`str(code)` returns the native symbolic name when known and the decimal value
otherwise.

## Fields

### `value`

The raw integer value.
