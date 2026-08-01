# Terminal Output

The `term` module provides terminal/console interfaces, including terminal
styling.

## Ordinary Output

The shell prelude provides [`echo`](../api/term/index.md#echo-args) and
[`print`](../api/term/index.md#print-options-args).

```
echo "result: $value"
print "working...\n"
```

These functions always output to `term.output()`. `echo` behaves similarly to
the Unix program or shell builtin, separating its arguments with spaces and
ending with a newline. Its arguments are converted to strings using the
[`std.arg`](../api/std/index.md#arg-value) coercion, which preserves the
syntactic form of arguments as best as possible. `print` concatenates all its
arguments without spaces, does not append a newline, and uses ordinary `str`
coercion.

Newlines and tabs are preserved, but other C0/C1 controls and terminal escape
sequences are sanitized before being output.

## Capturing the Console

[`term.capture`](../api/term/index.md#capture-console-func-args)
and [`term.sub`](../api/term/index.md#sub-func-trim-can_style-args) override
[`term.output()`](../api/term/index.md#output) for their duration.

```
let greeting = term.sub do echo "Hello, Alice!"
assert_eq $greeting "Hello, Alice!"
```

## Child Process Output

The main strand's implicit output is set once at startup: to
[`term.default`](../api/term/index.md#default) if stdout is a terminal, or to
[`shell.stdout`](../api/shell/stdout.md) otherwise. A child process launched
with no `stdout:` override inherits whichever one is current, so it follows
console/terminal interception — `progress` indicators,
[`term.capture`](../api/term/index.md#capture-console-func-args) — only when
stdout was a terminal to begin with. An omitted `stderr:` always defaults to
`term.default`.

## Styled Text

[`term.style`](../api/term/index.md#style-options-args) returns
[`term.Text`](../api/term/text.md), which can contain terminal styling:

```
import term

let warning = term.style WARNING fg: :YELLOW: bold: true
echo $warning "disk space is low"
```

`Text` values may be nested in further `style` calls, with outer style
attributes inherited.

```
let key = term.style important bold: true
let message = term.style "check $key now" fg: :YELLOW:
echo $message
```

Coercing `Text` with `str` returns its ANSI representation. Passing it
directly to `echo` or `print` displays it with its styling.

## Existing ANSI Output

Use [`term.preformat`](../api/term/index.md#preformat-text) for strings that
already contain ANSI SGR styling:

```
let rendered = term.preformat $compiler_output
echo $rendered
```

`preformat` validates and canonicalizes SGR styling. Other terminal controls,
including hyperlinks, are removed.

## Styling Control

`echo` and `print` style their output when the console they are writing to says
it can. That answer is the console's
[`can_style`](../api/term/console.md#can_style) — a property of the destination,
not a global.

For the host console, it is the process-wide policy:

1. If `FORCE_COLOR` is set, any value except `0` enables styling; `0` disables
   it.
2. Otherwise, a non-empty `NO_COLOR` disables styling.
3. Otherwise, styling follows stderr terminal detection.

For a [`capture`](../api/term/index.md#capture-console-func-args) it is `false`
unless asked for, which is what keeps a test asserting on `echo`ed text behaving
the same piped and on a developer's terminal:

```
# Plain, on a terminal or not.
assert_eq (term.sub do echo $warning) "warning"

# Unless the styling is the point.
term.sub can_style: true do echo $warning
```

Because it is the *installed* console that is consulted,
`term.console.can_style` still reports the process-wide policy from inside a
capture — naming it pins to the host, the same as for writes.

## Terminal Dimensions

[`term.console.geometry()`](../api/term/console.md#geometry) returns the
terminal's `rows` and `cols`, or `nil` when stderr is not a terminal.

```
let g = term.console.geometry()
if g
  echo $"─".repeat(g.cols)
```
