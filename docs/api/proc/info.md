# `Info`

Information about a process at a moment in time.

## Fields

All fields other than `pid` or `name` may be `nil` for a variety of reasons
other than those explicitly documented:

- The process had already exited and the value was no longer available.
- The process was a kernel process and therefore lacked it (e.g. `exe`)
- The value could not be read because of inadequate permissions.

### `pid`

The process ID, as an [`Int`](../std/int.md).

### `parent_pid`

The parent process ID as an [`Int`](../std/int.md).

Windows does not reparent orphaned processes, so the parent PID may be invalid
or reassigned.

### `name`

The kernel's short name for the process as a [`Str`](../std/str.md).

On Unix, this reflects whatever the process last set, and is typically
truncated.

### `exe`

The executable path as an [`fs.Path`](../fs/path.md).

Reading another user's executable path needs elevated rights on every platform.

### `cmdline`

The argument vector as a tuple of [`Str`](../std/str.md).

Restricted to processes owned by the same user on macOS.

Windows represents arguments as a single string rather than an array,
so the value reported here is split according to typical C runtime conventions.
[`cmdline_win`](#cmdline_win) is the raw original.

### `cwd`

The working directory as an [`fs.Path`](../fs/path.md).

Reading another user's working directory needs elevated rights.

On Windows this is maintained by the C runtime and is not an authoritative
path reported by the kernel, so it may have an arbitrary value set
by the application.

### `status`

How the process ended as a [`Status`](./status.md), or `nil` if it was still
running.

## Unix Fields

The following field is only available on records captured from a Unix target.

### `unix_id`

The process credentials as a
[`security.unix.Identity`](../security/unix/identity.md).

On macOS, `groups` is capped at 16 entries.

## Windows Fields

The following fields are only available on records captured from a Windows
target.

### `cmdline_win`

The command line as a single [`Str`](../std/str.md).

```
let p = proc.open $pid
echo $p.info().cmdline_win
```

### `token_info`

The process access token as a
[`security.windows.TokenInfo`](../security/windows/tokeninfo.md).

## Example

```
for info = proc.enumerate()
  if (info.name == "sshd")
    echo "$(info.pid) $(info.exe)"
```
