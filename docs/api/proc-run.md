# proc.run

The `proc.run` module provides access to external programs. See
[External Programs](../shell/external-programs.md) for lookup, redirection,
capture, and pipeline patterns.

## Programs

Access a program as a field, index `run` with its name or path, or pass it as
the first argument to `run`:

```
run.git status
run["/bin/echo"] hello
run git status
```

### `program ...args :stdin? :stdout? :stderr? :policy? :mode?`

Resolves and runs the program. Positional arguments are converted with
[`std.verbatim`](./std/index.md#verbatim-value).

#### Parameters

| Name     | Type                                       | Description                            |
| -------- | ------------------------------------------ | -------------------------------------- |
| `args`   | *                                          | Program arguments                      |
| `stdin`  | iterable\|path\|handle?                    | Standard input source                  |
| `stdout` | sink\|path\|handle?                        | Standard output destination            |
| `stderr` | sink\|path\|handle\|[`sym`](./std/sym.md)? | Standard error destination             |
| `policy` | [`dict`](./std/dict.md)?                   | Termination policy overrides           |
| `mode`   | [`sym`](./std/sym.md)?                     | `:LINE:` (default) or `:CHUNK:` output |

#### Returns

`nil` after the program exits successfully.

#### I/O redirection

Omitted stdin and stdout use the strand's current input and output. Omitted
stderr uses the strand's current [console](./term/console.md).

A redirect accepts:

- A [`str`](./std/str.md) or [`fs.Path`](./fs/path.md), opened as a file for
  the appropriate direction.
- An iterable for `stdin:`, or a sink for `stdout:` and `stderr:`.
- The corresponding [`shell.stdin`](./shell/stdin.md),
  [`shell.stdout`](./shell/stdout.md), or [`shell.stderr`](./shell/stderr.md)
  handle.
- [`std.null`](./std/null.md), providing empty input or discarding output.

`stderr: :STDOUT:` connects stderr to the selected stdout destination.

#### Output mode

`mode:` controls values produced when stdout or stderr is sent to a sink:
`:LINE:` produces one [`str`](./std/str.md) per line with its terminator intact;
`:CHUNK:` produces arbitrary-sized [`bin`](./std/bin.md) values. It has no
effect on stdin.

#### Termination policy

The `policy:` dictionary accepts these keys:

| Key      | Type                                                        | Description                                               |
| -------- | ----------------------------------------------------------- | --------------------------------------------------------- |
| `signal` | [`sym`](./std/sym.md)\|[`int`](./std/int.md)                | Unix signal name or target-native number                  |
| `grace`  | [`Duration`](./time/duration.md)\|[`float`](./std/float.md) | Time to wait after the termination signal                 |
| `force`  | [`bool`](./std/bool.md)                                     | Force termination after the grace period                  |

Unspecified keys inherit the current
[`proc.with_policy`](./proc/index.md#with_policy-func-signal-grace-force)
settings. Foreground Unix programs are terminated directly; programs launched
under `strand.spawn` or `strand.stream` are placed in a separate process group
and terminated as a group. Windows uses `CTRL_BREAK_EVENT`, with background
programs force-terminated through a Job Object. `signal` is invalid for a
Windows target.

## Program Methods

### `which()`

Resolves the program without running it.

#### Returns

[`fs.Path`](./fs/path.md), or `nil` when the program is not found.

#### Example

```
let git = run.git.which()
```
