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

#### Parameters

| Name     | Type                                                          | Description                                         |
| -------- | ------------------------------------------------------------- | --------------------------------------------------- |
| `func`   | `func`                                                        | Callable to execute                                 |
| `signal` | [`sym`](../std/sym.md)                                        | Unix signal name (default: `:TERM:`)                |
| `grace`  | [`Duration`](../time/duration.md)\|[`float`](../std/float.md) | Time before forced termination (default: 5 seconds) |
| `force`  | [`bool`](../std/bool.md)                                      | Force termination after the grace period            |
| `...`    |                                                               | Additional arguments passed to `func`               |

#### Returns

The return value of `func`.

The signal setting is retained when the strand later targets a Unix VFS.
Windows launches always use `CTRL_BREAK_EVENT`. With `force: false`, a process
that outlives the grace period is orphaned.

```
with_policy signal: :INT: grace: 2.5 do
  run worker
```

### `sub func :chomp?`

Captures the output of a function as a string.

Everything written is captured as a byte stream, without line decoding or
normalization — including values put into the strand's output, which
contribute exactly their own bytes.

`chomp:` is a whole-capture strip, not a per-value one: it removes at most one
line ending, from the end of the finished string.

#### Parameters

| Name    | Type                     | Description                                      |
| ------- | ------------------------ | ------------------------------------------------ |
| `func`  | `func`                   | function whose output to capture                 |
| `chomp` | [`Bool`](../std/bool.md) | Remove one trailing LF or CRLF (default: `true`) |

#### Returns

[`Str`](../std/str.md)

```
let output = sub do run echo hello
assert_eq $output "hello"
```
