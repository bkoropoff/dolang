# `Proc`

An open handle to a non-child process. Returned by
[`open`](./index.md#open-target-func). Using a closed handle raises
[`StateError`](../std/state-error.md).

## Fields

### `pid`

The process ID, as an [`Int`](../std/int.md).

## Methods

### `info()`

Takes a fresh snapshot of process state.

#### Returns

[`Info`](./info.md)

### `kill()`

Terminates the process aggressively in a manner typical for the target platform:

- Sends `SIGKILL` on Unix.
- `TerminateProcess()` on Windows.

### `signal sig`

Sends a signal to the process.

#### Parameters

| Name  | Type                   | Description                          |
| ----- | ---------------------- | ------------------------------------ |
| `sig` | [`sym`](../std/sym.md) | Signal name, such as `:TERM:`        |

#### Errors

Raises [`sys.UnsupportedError`](../sys/error.md) against a Windows target.

#### Example

```
let p = proc.open $pid
p.signal :HUP:
```

### `terminate()`

Terminates the process in the typical manner for the target platform:

- Sends `SIGTERM` on Unix.
- `TerminateProcess()` on Windows.

Note that this is equivalent to `kill()` on Windows, as there is no
consistent mechanism to "gently" terminate a non-child process on
that platform.

### `wait()`

Waits for the process to exit.

#### Returns

[`Status`](./status.md)

### `close()`

Closes the handle.
