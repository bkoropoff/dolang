# `Console`

The console: where human-readable output goes. Obtained as
[`term.console`](./index.md#console).

It may be a full terminal, or it may be a plain byte sink; it may support
styling even when it is not a terminal. Ask it what it supports rather than what
it is.

[`echo`](./index.md#echo-args) and [`print`](./index.md#print-options-args)
render here, and so does child process output that has not been given a
destination of its own. When an extension takes over the terminal, the console
follows it, so writing here during a progress display goes through the display
instead of fighting it.

Not the same as [`shell.stderr`](../shell/stderr.md): that is the process's
error stream and bypasses terminal takeover entirely. The console happens to
write there when nothing has taken the terminal over, which is why the two are
easy to confuse.

## Methods

### `write data`

Writes bytes verbatim and reports how many were written. Identical in behavior
to [`shell.stdout.write`](../shell/stdout.md#write-data), except that it goes to
the current console rather than to a fixed stream.

Nothing is sanitized. `echo` and `print` strip control sequences on their own
path before reaching the console; `write` is below that layer.

```
term.console.write b"\x1b[2K"
```

### `flush()`

Flushes buffered output to the console.

## Operators

### Sink

`Console` is a [sink](../std/sink.md). Values put into it are framed per the
ambient [I/O mode](../shell/index.md#with_io_mode-mode-func):

```
["building", "linking"] | term.console
```

`echo` and `print` are *not* subject to the I/O mode — they are terminal-bound
human output and are always line-framed. The rule is that adapters honor the
mode and `term` functions do not.
