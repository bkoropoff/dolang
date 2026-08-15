# LinkTarget

Describes a registry link target without losing its native NT path.

## Fields

### `native`

The lossless `\Registry\...` target as a [`Str`](../std/str.md).

### `root`

The recognized canonical root symbol, or `nil`. Canonical projections use
`:CURRENT_USER:`, `:USERS:`, or `:LOCAL_MACHINE:`.

### `subpath`

The path relative to `root`, or `nil` when the native path is not recognized.
