# Terminal Output

The `term` module separates ordinary text from trusted terminal presentation.
This prevents values containing escape sequences or control characters from
changing terminal state unexpectedly.

## Ordinary Output

The shell prelude provides [`echo`](../api/term/index.md#echo-args) and
[`print`](../api/term/index.md#print-options-args).

```
echo "result: $value"
print "working...\n"
```

These functions always output to stderr regardless of strand input/output state.
`echo` behaves similarly to the Unix program or shell builtin, separating its
arguments with spaces and ending with a newline. Its arguments are converted to
strings using the [`std.arg`](../api/std/index.md#arg-value) coercion, which
preserves the syntactic form of arguments as best as possible. `print`
concatenates all its arguments without spaces, does not append a newline, and
uses ordinary `str` coercion.

Newlines and tabs are preserved, but other C0/C1 controls and terminal escape
sequences are sanitized before being output.

Sanitization belongs to `echo` and `print`, not to the destination. Nothing
below them applies it — not [`term.console`](../api/term/console.md), not
[`shell.stdout`](../api/shell/stdout.md), and certainly not a child process that
inherited the stream. It is a property of these two functions, not a guarantee
about what reaches the terminal.

## Three Destinations

Output has three destinations, and they are deliberately distinct:

| Destination      | Reached by                                 | Follows takeover |
| ---------------- | ------------------------------------------ | ---------------- |
| Console          | `echo`, `print`, `term.console`            | Yes              |
| Implicit sink    | `strand.put`, pipelines                    | No               |
| Explicit handles | `shell.stdin`/`stdout`/`stderr`, `fs.File` | No               |

The implicit sink *starts as* `shell.stdout` — the same object, not an analogy —
but it is redirected by pipelines and by `run`, so the two are not synonyms.

`echo` targets the console rather than stdout on purpose. When that is the wrong
choice the failure is loud and locally diagnosable: `script.dol > out.txt`
leaves the file empty while the output is plainly visible on the terminal. The
reverse mistake — diagnostics mixed into a structured stdout stream — is silent,
and surfaces as corruption in whatever consumes it downstream.

### `term.console` versus `shell.stderr`

Both usually write to the same place, which is why they are easy to confuse:

- [`term.console`](../api/term/console.md) is the *human channel*. When an
  extension takes the terminal over shell-wide, output follows it.
- [`shell.stderr`](../api/shell/stderr.md) is the *error stream*. It bypasses
  such a takeover entirely — caveat emptor.

Use the console for anything a person reads; use `shell.stderr` when you
specifically mean the stream.

## Capturing the Console

[`term.capture`](../api/term/index.md#capture-console-func-args) installs a
console for the duration of a call, and
[`term.sub`](../api/term/index.md#sub-func-trim-args) returns what was written
as a string. This is the same pinning rule one level further in: `echo`,
`print`, diagnostics, and undirected child output are anonymous and follow
[`term.output()`](../api/term/index.md#output), while
[`term.console`](../api/term/console.md) is a name and pins to the host.

```
let greeting = term.sub do greet Alice
assert_eq $greeting "Hello, Alice!"
```

The pairing with `proc.sub` makes the three destinations self-documenting:

```
term.sub do run mytool     # what mytool told a person   (its stderr)
sub do run mytool          # what mytool produced        (its stdout)
```

A capture is inherited by strands spawned inside it, and nests — the innermost
one wins, and the outer resumes when it ends. Since a console is evaluated
before the override it installs, a capture can never route into a strand that
already inherited it, so cycles are impossible by construction.

A plain sink passed to `capture` is wrapped in a
[`SinkConsole`](../api/term/sink-console.md), which is a bytestream-to-value
boundary and therefore honors the I/O mode. `term.sub` sits on the byte side
instead and reports exactly what was written.

Styling is off inside a capture: a capture is not a terminal, and without this
an assertion on captured text would pass in CI and fail on a developer's
terminal.

## Child Process Output

A child process launched with [`run`](../api/proc/index.md) and no `stdout:` or
`stderr:` argument has an *anonymous* channel, so it follows the ambient
console. If an extension has taken the terminal over, its output is copied to
the console rather than to the inherited descriptor, so it cannot scribble over
a progress display. Otherwise it inherits the stream directly.

The same applies to a capture: an undirected child's stderr is console-bound,
so an enclosing `term.capture` takes it. Child stdout is the data stream and
keeps going to the implicit sink.

Naming a handle pins the channel to exactly what it names:

```
# Follows the console — pumped through a progress display or capture.
run mytool

# The real stream, whatever is happening on the terminal.
run mytool stdout: $shell.stdout
```

This is the same "ambient state governs anonymous channels, named handles pin"
rule that the [I/O mode](../api/shell/index.md#with_io_mode-mode-func) follows.

Copying to the console is a byte-to-byte edge: the child emits bytes and the
console consumes bytes, so no framing applies in either direction. Nothing is
split into lines, nothing must be valid UTF-8, and no line ending is added or
translated.

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

Styling is enabled when stderr is a terminal at process startup, otherwise
`Text` renders without it. Environment variables override terminal detection:

1. If `FORCE_COLOR` is set, any value except `0` enables styling; `0` disables
   it.
2. Otherwise, a non-empty `NO_COLOR` disables styling.
3. Otherwise, styling follows stderr terminal detection.

`term.have_terminal` reports whether stderr was a terminal; it does not include
the environment-variable override.

## Raw Output

Output to the default stdout sink using `strand.put` is not sanitized, but
follows the current I/O mode. See
[`shell.with_io_mode`](../api/shell/index.md#with_io_mode-mode-func).
