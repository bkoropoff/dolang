# proc

The `proc` module provides functions and objects for process invocation and
output capture.

## Types

| Name                  | Description                                      |
| --------------------- | ------------------------------------------------ |
| [`Error`](./error.md) | Error raised when a process exits unsuccessfully |

## Functions

### `with_policy func :signal? :grace? :force? ...`

Runs a callable with temporary process termination defaults.

**Parameters:**

| Name     | Type                                                          | Description                                         |
| -------- | ------------------------------------------------------------- | --------------------------------------------------- |
| `func`   | `func`                                                        | Callable to execute                                 |
| `signal` | [`sym`](../std/sym.md)                                        | Unix signal name (default: `:TERM:`)                |
| `grace`  | [`Duration`](../time/duration.md)\|[`float`](../std/float.md) | Time before forced termination (default: 5 seconds) |
| `force`  | [`bool`](../std/bool.md)                                      | Force termination after the grace period            |
| `...`    |                                                               | Additional arguments passed to `func`               |

**Returns:**

The return value of `func`.

The signal setting is retained when the strand later targets a Unix VFS.
Windows launches always use `CTRL_BREAK_EVENT`. With `force: false`, a process
that outlives the grace period is orphaned.

```
with_policy signal: :INT: grace: 2.5 do
  run worker
```

### `mute func ...`

Executes a function with its output discarded.

#### Parameters

| Name   | Type   | Description                           |
| ------ | ------ | ------------------------------------- |
| `func` | `func` | function to execute with muted output |
| `...`  |        | additional arguments passed to `func` |

#### Returns

The return value of `func`.

The `mute` function redirects the output of the given function to
[`NULLITER`](../std/index.md#nulliter), effectively discarding
`stdout` of any executed external programs.

```
# Execute a command without printing its output
mute do run printf "this will not be printed"
```

### `sub func :trim?`

Captures the output of a function as a string.

External process output is captured as a byte stream without line decoding or
normalization. Value writes still cross the sink boundary and follow the
current I/O mode.

#### Parameters

| Name   | Type                     | Description                                      |
| ------ | ------------------------ | ------------------------------------------------ |
| `func` | `func`                   | function whose output to capture                 |
| `trim` | [`Bool`](../std/bool.md) | Remove one trailing LF or CRLF (default: `true`) |

#### Returns

[`Str`](../std/str.md)

```
let output = sub do run echo hello
assert_eq $output "hello"
```
