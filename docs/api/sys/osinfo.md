# OsInfo

Operating system information for the active VFS target returned by
[`os_info`](./index.md).

## Fields

### `family`

Operating system family derived from the target OS. Supported values are
`:UNIX:` and `:WINDOWS:`.

### `is_wine`

Whether the process is running under Wine.

This field is only available for Windows targets.

### `os`

Specific operating system. Supported values are `:LINUX:`, `:MACOS:`,
`:FREEBSD:`, and `:WINDOWS:`.
