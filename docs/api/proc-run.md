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
[`std.verbatim`](../api/std/index.md#verbatim-value).

```
run.echo hello world
# Runs: /usr/bin/echo hello world
```

## Key Arguments

| Name      | Type                    | Description                              |
| --------- | ----------------------- | ---------------------------------------- |
| `stdin`?  | iterable\|path\|handle  | Source for the program's standard input  |
| `stdout`? | sink\|path\|handle      | Target for the program's standard output |
| `stderr`? | sink\|path\|handle\|sym | Target for the program's standard error  |
| `policy`? | [`dict`](./std/dict.md) | Termination policy overrides             |
| `mode`?   | [`sym`](./std/sym.md)   | Framing for captured output              |

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
  pipeline ends, and [`fs.File`](./fs/file.md) handles. The crossing is
  [lossless](./shell/index.md#stream-framing): a value put into `stdin:`
  writes exactly its own bytes, and a value read out of `stdout:`/`stderr:`
  keeps its line ending. `mode:` selects the framing for the values read back,
  `:LINE:` by default.
- One of the [`shell.stdin`](./shell/stdin.md),
  [`shell.stdout`](./shell/stdout.md), or [`shell.stderr`](./shell/stderr.md)
  handles, which hands the program the corresponding stream directly.
- `nil` or [`NULLITER`](./std/index.md#nulliter), discarding the stream.

`stderr: :stdout:` merges the program's standard error into whatever its
standard output is connected to.

```
run.tar czf archive.tar.gz src stderr: :stdout:
run.make -j8 stdout: build.log stderr: build.log
run.sort stdin: (["c", "a", "b"].crimp()) stdout: $sorted
```

An omitted `stderr:` defaults to [`term.default`](./term/index.md#default),
which follows console/terminal interception such as `progress` module
indicators or [`term.capture`](./term/index.md#capture-console-func-args-mode).
Naming `shell.stdout`/`shell.stderr` explicitly opts out of that entirely.

See [Terminal output](../shell/terminal-output.md) for the full model.

#### Framing

`mode:` chooses how a program's output is cut into values when it is pumped
into a sink: `:LINE:` (the default) for one [`str`](./std/str.md) per line with
its terminator intact, `:CHUNK:` for arbitrary [`bin`](./std/bin.md) chunks. It
applies to both `stdout:` and `stderr:` — a redirect that splits them is
already naming two sinks and can adapt each one separately.

```
let chunks = []
run gzip -c stdin: ["hello world"] stdout: $chunks mode: :CHUNK:
assert (chunks[0].starts_with b"\x1f\x8b")

let lines = []
run.grep nologin stdin: $passwd stdout: (lines.prechomp())
```

`mode:` has no effect on `stdin:`, which is unframed: each value written
contributes its own bytes and nothing else. Use
[`crimp`](./std/iter.md#crimp-terminator) to terminate them.

```
run.sort stdin: (["c", "a", "b"].crimp()) stdout: $sorted
```

An [`fs.File`](./fs/file.md) read as `stdin:` brings its own framing, from the
`b` flag it was opened with.

### Termination Policy

Use the `policy:` dictionary to override termination behavior for one launch.
Unspecified fields inherit the current
[`proc.with_policy`](./proc/index.md#with_policy-func-signal-grace-force)
settings.

| Key      | Type                                                        | Description                                               |
| -------- | ----------------------------------------------------------- | --------------------------------------------------------- |
| `signal` | [`sym`](./std/sym.md)\|[`int`](./std/int.md)                | Unix signal name or target-native number                  |
| `grace`  | [`Duration`](./time/duration.md)\|[`float`](./std/float.md) | Time to wait for process to exit after termination signal |
| `force`  | [`bool`](./std/bool.md)                                     | Force termination after the grace period                  |

```
run worker policy: {signal: :INT:, grace: 10.0, force: true}
```

Foreground Unix processes are terminated directly. Processes launched under
`strand.spawn` or `strand.stream` are placed in a separate process group and
terminated as a group. Windows uses `CTRL_BREAK_EVENT`; background launches are
force-terminated through a Job Object. A `signal` override is invalid for a
Windows target.

### Capturing Output

Use [`sub`](proc/index.md#sub-func-chomp) to capture a program's standard output
as a string:

```
let kernel = sub do run.uname -r
echo "Kernel: $kernel"
```

To capture all console-bound output, use
[`term.sub`](./term/index.md#sub-func-chomp-can_style-args); this picks up
unredirected undirected stderr.

```
let complaints = term.sub do run.mytool
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
