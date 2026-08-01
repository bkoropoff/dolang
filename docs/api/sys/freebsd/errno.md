# Errno

[`sys.unix.Errno`](../unix/errno.md) originating from FreeBSD.

## Constructor

### `Errno value`

Creates a FreeBSD error number.

**Parameters:**

| Name  | Type                      | Description      |
| ----- | ------------------------- | ---------------- |
| value | [`int`](../../std/int.md) | Raw error number |

Known codes are available as class fields such as `Errno.ENOENT`.

## Fields

### `os`

The originating operating system, `:FREEBSD:`.

## Inherits

- [`sys.unix.Errno`](../unix/errno.md)
