# WinError

[`sys.ErrorCode`](../error-code.md) containing a Windows system error code.

## Constructor

### `WinError value`

Creates a Windows system error code.

**Parameters:**

| Name  | Type                      | Description       |
| ----- | ------------------------- | ----------------- |
| value | [`int`](../../std/int.md) | Unsigned raw code |

Known codes are available as class fields such as
`WinError.ERROR_FILE_NOT_FOUND`.

## Inherits

- [`sys.ErrorCode`](../error-code.md)
