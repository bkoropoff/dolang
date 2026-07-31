# `Console`

The console interface: where human-readable output goes.

`Console` is abstract. The provided implementations are the host console
([`term.console`](./index.md#console)), [`SinkConsole`](./sink-console.md).

## Fields

### `can_style`

Whether the console supports ANSI styling.

## Methods

### `write data`

Writes bytes verbatim and reports how many were written.

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

**Parameters:**

| Name   | Type                                           | Description    |
| ------ | ---------------------------------------------- | -------------- |
| `data` | [`Str`](../std/str.md)\|[`Bin`](../std/bin.md) | Bytes to write |

**Returns:** [`Int`](../std/int.md) byte count, excluding the line ending

### `flush()`

Makes buffered output visible.

### `geometry()`

The console's dimensions, or `nil` if it is just a stream.

**Returns:** [`Geometry`](./geometry.md) or `nil`

```
let g = term.console.geometry()
if g
  echo "terminal is $(g.cols)x$(g.rows)"
```

`nil` means "not a terminal", not "the size could not be determined" — a
terminal that reports no window size defaults to the conventional 24×80.

## Operators

### Sink

`Console` is a [sink](../std/sink.md). Values put into it are framed per the
ambient [I/O mode](../shell/index.md#with_io_mode-mode-func) and then written
as bytes:

```
strand.pipeline output: $term.console
  do strand.from ["building", "linking"]
```

## Subclassing

Do classes may subclass `Console` to implement one. Supply `write`, `writeln`,
and `flush`; `put`, the sink protocol, and the capability members come from the
base:

| Console                            | `can_style`    | `geometry()`  |
| ---------------------------------- | -------------- | ------------- |
| `Console` (the base)               | `false`        | `nil`         |
| Host (`term.console`)              | styling policy | terminal size |
| [`SinkConsole`](./sink-console.md) | as constructed | `nil`         |

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
