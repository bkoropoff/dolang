# `Console`

The console interface: where human-readable output goes.

A console is a byte stream, not merely a [sink](../std/sink.md).
[`echo`](./index.md#echo-args) always terminates a line and
[`print`](./index.md#print-options-args) never does, and neither is subject to
the [I/O mode](../shell/index.md#with_io_mode-mode-func) — so the terminator
cannot be left to value framing. Only the console knows its own line ending,
which is what `writeln` is for. `put` is layered on top, so a console is usable
as an ordinary sink as well.

`Console` itself is the interface; its methods throw
[`UnsupportedError`](../std/unsupported-error.md). The implementations are the
host console ([`term.console`](./index.md#console)),
[`SinkConsole`](./sink-console.md), and whatever Do code subclasses this with.
`type value Console` is the test that a value has the surface.

[`echo`](./index.md#echo-args), [`print`](./index.md#print-options-args),
diagnostics, and undirected child process output are all *anonymous*, so they
go to [`output()`](./index.md#output) — the capture installed by
[`capture`](./index.md#capture-console-func-args) if there is one, else the host
console. `term.console` is a name, so it pins to the host past any capture, for
`write` and `put` alike.

Not the same as [`shell.stderr`](../shell/stderr.md): that is the process's
error stream and bypasses terminal takeover and capture entirely. The host
console happens to write there when nothing has taken the terminal over, which
is why the two are easy to confuse.

## Fields

### `can_style`

Whether ANSI styling should be emitted to this console.

Fixed for the life of an installed console: `echo` and `print` read it when
[`capture`](./index.md#capture-console-func-args) installs the console, not on
every write. That is what makes a capture's styling deterministic — mutating
the field afterwards does not change the current capture's answer.

The host console answers the `FORCE_COLOR`/`NO_COLOR`/tty policy described in
[Styling Control](../../shell/terminal-output.md#styling-control). Everything
else answers `false` unless it was asked for.

```
term.capture (term.SinkConsole(out, can_style: true)) do
  echo $warning
```

## Methods

### `write data`

Writes bytes verbatim and reports how many were written.

Nothing is sanitized. `echo` and `print` strip control sequences on their own
path before reaching the console; `write` is below that layer.

**Parameters:**

| Name   | Type                                           | Description    |
| ------ | ---------------------------------------------- | -------------- |
| `data` | [`Str`](../std/str.md)\|[`Bin`](../std/bin.md) | Bytes to write |

**Returns:** [`Int`](../std/int.md) byte count

```
term.console.write b"\x1b[2K"
```

### `writeln data`

Writes bytes followed by *this console's* line ending.

Not the same as `write "…\n"`: a capture over a value sink picks its own
ending, and only the console knows which one.

**Parameters:**

| Name   | Type                                           | Description    |
| ------ | ---------------------------------------------- | -------------- |
| `data` | [`Str`](../std/str.md)\|[`Bin`](../std/bin.md) | Bytes to write |

**Returns:** [`Int`](../std/int.md) byte count, excluding the line ending

### `flush()`

Makes buffered output visible. A `SinkConsole` also emits any partial final
line here.

### `geometry()`

The console's dimensions, or `nil` if it is just a stream.

A call rather than a field: the terminal is resized while the program runs.

**Returns:** [`Geometry`](./geometry.md) or `nil`

```
let g = term.console.geometry()
if g
  echo "terminal is $(g.cols)x$(g.rows)"
```

`nil` means "not a terminal", not "the size could not be determined" — a
terminal that reports no window size still answers, with the conventional
24×80. So `term.console.geometry()` is the honest test for whether a real
terminal is attached.

An answer here says nothing about cursor control. A console may support
`geometry()` and still throw
[`UnsupportedError`](../std/unsupported-error.md) for other terminal
operations — under a progress display, for instance, the width is real but the
cursor belongs to the display.

### Line endings

Which ending `writeln` appends is the console's own business, and the two
built-in consoles answer differently on Windows:

| Console                            | `writeln`      | `put` (value framing) |
| ---------------------------------- | -------------- | --------------------- |
| Host (`term.console`)              | LF everywhere  | Platform              |
| [`SinkConsole`](./sink-console.md) | Platform       | Platform              |

Each answer is locally right: a terminal takes LF, while bytes crossing into a
value sink should look like the platform's. `echo` therefore writes LF on
Windows as it always has, and only `:CHUNK:` capture can observe the difference
— `:LINE:` strips either ending.

## Operators

### Sink

`Console` is a [sink](../std/sink.md). Values put into it are framed per the
ambient [I/O mode](../shell/index.md#with_io_mode-mode-func) and then written
as bytes:

```
strand.pipeline output: $term.console
  do strand.from ["building", "linking"]
```

`echo` and `print` are *not* subject to the I/O mode — they are human output
and go through `writeln`/`write`. The rule is that adapters honor the mode and
`term` functions do not.

## Subclassing

Do classes may subclass `Console` to implement one. Supply `write`, `writeln`,
and `flush`; `put`, the sink protocol, and the capability members come from the
base:

| Console                            | `can_style`    | `geometry()`  |
| ---------------------------------- | -------------- | ------------- |
| `Console` (the base)               | `false`        | `nil`         |
| Host (`term.console`)              | styling policy | terminal size |
| [`SinkConsole`](./sink-console.md) | as constructed | `nil`         |

Unlike the write methods, the capability members have a safe default, so a
subclass only overrides them when it has a better answer — a `can_style` field
and a `geometry` method, both of which shadow the base.

```
class Recorder: term.Console
  pub field lines

  def (init) self
    term.Console.(init) $self
    self.lines = []

  pub def write self data
    self.lines.push (str data)
    data.len

  pub def writeln self data
    self.lines.push (str data)
    data.len

  pub def flush _self
    nil

let recorder = Recorder()
term.capture $recorder do echo hello
assert_eq $recorder.lines ["hello"]
```

A console whose own methods call `echo` does not recurse: while a write is
being dispatched, console output falls through to the host.
