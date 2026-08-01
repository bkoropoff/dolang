# Error

`Error` is raised for system and I/O failures.

It can be subclassed for library-specific operational failures.

## Constructor

### `Error message :code?`

Creates a system error.

**Parameters:**

| Name    | Type                            | Description              |
| ------- | ------------------------------- | ------------------------ |
| message | [`str`](../std/str.md)          | Error message            |
| code    | [`ErrorCode`](./error-code.md)? | Underlying platform code |

```
let error = Error "operation failed" code: $sys.linux.Errno.EIO
```

```
try
  fs.read "/definitely/missing"
catch Error: err
  echo $Str(err)
```

`str(err)` returns the underlying system error message and appends the native
symbolic code in parentheses when it is known.

## Fields

### `code`

`code` contains the underlying native system error code when one exists:

```
try
  fs.read "/definitely/missing"
catch Error: err
  echo $err.code
```

The value is [`sys.linux.Errno`](./linux/errno.md),
[`sys.freebsd.Errno`](./freebsd/errno.md),
[`sys.macos.Errno`](./macos/errno.md), or
[`sys.windows.WinError`](./windows/win-error.md), according to the system where
the error originated. Errors without a native code expose `nil`.

## Inherits

- [`RuntimeError`](../std/runtime-error.md)
