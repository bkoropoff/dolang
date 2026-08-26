# `Console`

The console interface: where human-readable output goes.

`Console` is abstract. The provided implementations are the host console
([`term.console`](./index.md#console)), [`SinkConsole`](./sink-console.md).

## Fields

### `can_style`

Whether the console supports ANSI styling.

### `is_tty`

Whether the console is a real terminal. This is the determinative test —
unlike [`geometry()`](#geometry), which may answer `nil` for a real terminal
that simply cannot report its size.

### `line_ending`

The line ending appropriate to this console's device, as a
[`Str`](../std/str.md).

The console owns the policy but does not apply it: a caller that wants a
terminated line appends this itself, in a single `write`.

```
term.console.write "done$(term.console.line_ending)"
```

Every built-in console reports `"\n"`: a console is a terminal-shaped stream
rather than a file, and `echo` writes LF on Windows too. A subclass may report
something else, and `echo`/`print` will honor it. For a *file's* native ending
use [`shell.line_ending()`](../shell/index.md#line_ending) instead.

## Methods

### `write data`

Writes bytes verbatim and reports how many were written.

#### Parameters

| Name   | Type                                           | Description    |
| ------ | ---------------------------------------------- | -------------- |
| `data` | [`Str`](../std/str.md)\|[`Bin`](../std/bin.md) | Bytes to write |

#### Returns

[`Int`](../std/int.md) byte count

#### Example

```
term.console.write b"\x1b[2K"
```

A caller that wants a terminated line assembles it and issues **one** `write`
rather than two — concurrent strands writing to the same console would
otherwise interleave between the text and its terminator.

### `flush()`

Makes buffered output visible.

### `geometry()`

The console's dimensions, or `nil` if it is just a stream.

#### Returns

[`Geometry`](./geometry.md) or `nil`

#### Example

```
let g = term.console.geometry()
if g
  echo "terminal is $(g.cols)x$(g.rows)"
```

`nil` is advisory: it also covers a real terminal whose size could not be
determined. Use [`is_tty`](#is_tty) to test for a real terminal regardless of
whether a size is available.

The host console never returns `nil` here — see
[`Geometry`](./geometry.md#rows) for how `rows`/`cols` are each
independently `nil` when unknown rather than the whole result being absent.

## Operators

### Sink

`Console` is a [sink](../std/sink.md). A value put into it contributes exactly
its own bytes — no terminator is added. Use
[`precrimp`](../std/sink.md#precrimp-terminator) to get one:

```
pipeline output: (term.console.precrimp())
  do strand.from ["building", "linking"]
```

## Subclassing

Do classes may subclass `Console` to implement one. Supply `write` and `flush`;
`put`, the sink protocol, and the capability members come from the base:

| Console                            | `can_style`    | `is_tty`      | `geometry()`  |
| ---------------------------------- | -------------- | ------------- | ------------- |
| `Console` (the base)               | `false`        | `false`       | `nil`         |
| Host (`term.console`)              | styling policy | real tty test | never `nil`   |
| [`SinkConsole`](./sink-console.md) | as constructed | `false`       | `nil`         |

```
class Recorder: term.Console
  pub field lines

  def (init) self
    term.Console.(init) $self
    self.lines = []

  pub def write self data
    self.lines.push (str data)
    data.len

  pub def flush _self
    nil

let recorder = Recorder()
term.capture $recorder do echo hello
assert_eq $recorder.lines ["hello\n"]
```

Override `line_ending` to change what `echo` appends; the base reports `"\n"`.

A console whose own methods call `echo` does not recurse: while a write is
being dispatched, console output falls through to the host.
