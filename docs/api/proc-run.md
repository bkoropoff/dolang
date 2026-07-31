# proc.run

The `proc.run` module provides access to external programs.

## Accessing Programs

Use `proc.run` as a namespace to access programs by name:

```
run.ls -la
run.git status
run["/bin/echo"] hello world
run[Path "tools/build.sh"] --help
```

Or import specific programs:

```
import proc.run:
  - ls
  - git
  - curl

ls -la
git status
```

Or call it with the program as a first argument:

```
run cat Cargo.toml
```

## Program Execution

When a program object is called, it spawns the external program with the given
arguments. Arguments are converted to strings using
[`std.arg`](../api/std/index.md#arg-value).

```
run.echo hello world
# Runs: /usr/bin/echo hello world
```

## Reserved Keyword Arguments

Four keyword arguments are reserved by the launch itself rather than passed to
the program: `stdin:`, `stdout:`, `stderr:`, and `policy:`.

| Name      | Type                     | Description                              |
| --------- | ------------------------ | ---------------------------------------- |
| `stdin`   | iterable\|path\|handle   | Source for the program's standard input  |
| `stdout`  | sink\|path\|handle       | Target for the program's standard output |
| `stderr`  | sink\|path\|handle\|sym  | Target for the program's standard error  |
| `policy`  | [`dict`](./std/dict.md)  | Termination policy overrides             |

### I/O Redirection

Omitted, programs participate in Do's I/O system:

- Program **stdin** is connected to the current input
- Program **stdout** is connected to the current output
- Program **stderr** goes to the [console](./term/console.md)

This means programs work naturally in pipelines:

```
import strand

let result = strand.pipeline
  do run.cat /etc/passwd
  do run.grep nologin
  do strand.each do |line| [...line.split ":"]
  do strand.collect()
```

Given explicitly, each accepts:

- A [`str`](./std/str.md) or [`fs.Path`](./fs/path.md), which is opened as a
  file for the appropriate direction.
- Any iterable (`stdin:`) or sink (`stdout:`/`stderr:`), including arrays,
  pipeline ends, and [`fs.File`](./fs/file.md) handles. Values crossing this
  boundary are framed per the ambient
  [I/O mode](./shell/index.md#with_io_mode-mode-func).
- One of the [`shell.stdin`](./shell/stdin.md),
  [`shell.stdout`](./shell/stdout.md), or [`shell.stderr`](./shell/stderr.md)
  handles, which hands the program the corresponding stream directly.
- `nil` or [`NULLITER`](./std/index.md#nulliter), discarding the stream.

`stderr: :stdout:` merges the program's standard error into whatever its
standard output is connected to.

```
run.tar czf archive.tar.gz src stderr: :stdout:
run.make -j8 stdout: build.log stderr: build.log
run.sort stdin: ["c", "a", "b"] stdout: $sorted
```

#### Naming a handle opts out of console routing

An omitted `stdout:`/`stderr:` is an *anonymous* channel and follows the
ambient console, so when an extension has taken the terminal over the program's
output is copied to the console instead of to the inherited stream. Naming a
handle pins the channel to exactly what it names:

```
# Follows the console — pumped through a progress display if one is active.
run mytool

# The real stream, whatever is happening on the terminal.
run mytool stdout: $shell.stdout
```

See [Terminal output](../shell/terminal-output.md) for the full model.

### Termination Policy

Use the reserved `policy:` dictionary to override termination behavior for one
launch. Unspecified fields inherit the current
[`proc.with_policy`](./proc/index.md#with_policy-func-signal-grace-force)
settings.

| Key      | Type                                                        | Description                              |
| -------- | ----------------------------------------------------------- | ---------------------------------------- |
| `signal` | [`sym`](./std/sym.md)\|[`int`](./std/int.md)                | Unix signal name or target-native number |
| `grace`  | [`Duration`](./time/duration.md)\|[`float`](./std/float.md) | Time before forced termination           |
| `force`  | [`bool`](./std/bool.md)                                     | Force termination after the grace period |

```
run worker policy: {signal: :INT:, grace: 10.0, force: true}
```

Foreground Unix processes are terminated directly. Processes launched under
`strand.spawn` or `strand.stream` are placed in a separate process group and
terminated as a group. Windows uses `CTRL_BREAK_EVENT`; background launches
are force-terminated through a Job Object. A per-launch `signal` override is
invalid for a Windows target. Numeric signals are accepted only for direct
launch policies and are interpreted using the target's numbering. Named signals
cross VFS boundaries symbolically and are resolved by the target VFS. They
produce `ValueError` at launch when the target does not support them.

### Capturing Output

Use [`sub`](proc/index.md#sub-func-trim) to capture a program's output as a
string:

```
let kernel = sub do run.uname -r
echo "Kernel: $kernel"
```

### Environment

Programs inherit the current environment from [`shell.env`](shell/index.md#env).
Use the [`shell.env`](shell/index.md#env) function to set variables for a
specific invocation:

```
env LANG: C do
  run.sort input.txt
```

## Program Methods

### `which()`

Returns the resolved path to the program executable, if found.

```
echo $run.ls.which()
# Prints: /usr/bin/ls

echo $run.nonexistent.which()
# Prints: nil
```
