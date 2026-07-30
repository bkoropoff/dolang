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

### `io_mode mode func ...`

Executes a function with the current strand's external process I/O mode set for
the duration of the call.

In `:LINE:` mode, external process input is treated as UTF-8, split on line
boundaries, and yields `Str` values with any line endings removed. Output to an
external process sends the `Str` form of each value as UTF-8 with
platform-specific line endings appended, except for `Bin` values, which are
always sent verbatim. This is the default behavior.

In `:CHUNK:` mode, input yields arbitrary-size `Bin` values with no other
processing. Output sends `Bin` values verbatim and otherwise sends the `Str`
form of each value as UTF-8 with no further transformation.

In a pipeline, the mode of the strand *adjacent* to the external process
determines behavior -- that is, the producer or consumer's mode determines
behavior. When the iterator or sink of a strand running an
external process is *not* a pipeline channel, the mode of that strand is used.
Adjacent external processes in a pipeline always communicate in raw bytes
regardless of mode.

#### Parameters

| Name   | Type | Description                                   |
| ------ | ---- | --------------------------------------------- |
| `mode` |      | `:LINE:` or `:CHUNK:`                         |
| `func` |      | function to execute with that channel mode    |
| `...`  |      | additional arguments passed to `func`         |

#### Returns

The return value of `func`.

#### Example

```
let chunks = []
io_mode :CHUNK: do run gzip -c stdin: ["hello world"] stdout: $chunks

assert (chunks[0].starts_with b"\x1f\x8b")
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

#### Parameters

| Name   | Type                     | Description                                                        |
| ------ | ------------------------ | ------------------------------------------------------------------ |
| `func` | `func`                   | function whose output to capture                                   |
| `trim` | [`Bool`](../std/bool.md) | whether to trim trailing carriage return/newline (default: `true`) |

#### Returns

[`Str`](../std/str.md)

```
let output = sub do run echo hello
assert_eq $output "hello"
```
