# `Info`

A snapshot of one process on the target, as of when it was taken.

Produced by [`enumerate`](./index.md#enumerate) and by
[`Proc.info()`](./proc.md#info). Nothing here re-reads the process, so a field
describes the process as it was, not as it is.

## Fields

Every field but `pid` and `name` can be `nil`: no other attribute is available
on every platform, for every target process, to every caller. `nil` means the
attribute was not obtained for this particular process — a field that the
record's platform does not have at all raises
[`FieldError`](../std/field-error.md) instead, and those are grouped below.

### `pid`

The process ID, as an [`Int`](../std/int.md).

### `parent_pid`

The parent process ID, or `nil` where the platform reports none.

### `name`

The kernel's short name for the process, as a [`Str`](../std/str.md).

Not the executable path: Linux and the BSDs truncate it, and it reflects
whatever the process last set rather than what it was launched as.

### `exe`

The executable path as an [`fs.Path`](../fs/path.md), or `nil`.

Reading another user's executable path needs elevated rights on every platform.

### `cmdline`

The argument vector as a tuple of [`Str`](../std/str.md), or `nil`.

Restricted to processes owned by the same user on macOS, and unavailable on
Windows, which has no documented interface for reading another process's
command line.

### `cwd`

The working directory as an [`fs.Path`](../fs/path.md), or `nil`.

Available on Linux and macOS. FreeBSD exposes it only through `libprocstat`,
and Windows not at all.

## Unix Fields

The following field is only available on records captured from a Unix target.

### `unix_id`

The process credentials as a
[`security.unix.Identity`](../security/unix/identity.md).

On macOS `group_ids` is the kernel credential list, capped at 16 entries. The
interface that resolves memberships beyond that answers only for the calling
process, so a foreign group list there can be a truncated view where
[`security.unix.id()`](../security/unix/index.md) is not.

## Windows Fields

The following field is only available on records captured from a Windows
target.

### `token_info`

The process access token as a
[`security.windows.TokenInfo`](../security/windows/tokeninfo.md), or `nil`.

`nil` on a record produced by [`enumerate`](./index.md#enumerate): reading a
token costs a process open and a token open per entry, and is denied for most of
the table to an unelevated caller. [`Proc.info()`](./proc.md#info) fills it in,
having already paid for a handle.

## Example

```
for info = proc.enumerate()
  if (info.name == "sshd")
    echo "$(info.pid) $(info.exe)"
```
