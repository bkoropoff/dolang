# progress

The `progress` module provides terminal progress bars and spinners. It
coordinates output so that `term.echo`, `term.print`, and child process output
do not clobber progress indicators.

Progress state is implicit: `progress.with` activates a progress context for the
current scope, and `progress.show` creates widgets at the appropriate
nesting depth automatically.

When stderr is not a terminal, `with`/`show` switch to a plain-text rendering
instead: each indicator prints an indented
`icon <parent:id> message bar-or-position elapsed` line through the same
`NO_COLOR`/`FORCE_COLOR`-aware output path `term.echo` uses. The `<parent:id>`
tag stands in for the tree structure a live redraw would otherwise show visually
— `<3>` for a top-level indicator, `<3:7>` for one whose immediate parent is
`<3>`. IDs are assigned per `progress.with` scope in creation order and are
never reused. `Indicator.update`/`delta` calls that only change position
(`position:`/`delta:`/`total:`) are rate-limited (see `interval:` below) so a
tight loop doesn't flood a log file; `icon:`/`message:` changes always print
immediately, since they represent a change in what the indicator is doing, not
routine progress. A line is always printed when an indicator is created and when
its scope exits, so the last known status is never lost — the first shows
`started` and the last shows `finished (Xs)` in place of an elapsed time, so a
fast-finishing indicator's closing line doesn't read as just another routine
update.

## Functions

### `with func`

Activates a progress context for the duration of `func`. Terminal output (echo,
print, child process stderr/stdout) is routed through the progress display so it
does not interfere with active indicators.

#### Parameters

| Name       | Type   | Description                                  |
| ---------- | ------ | -------------------------------------------- |
| `func`     | func   | Callback (no arguments)                      |
| `style`    | dict?  | Display style overrides                      |
| `interval` | float? | Plain-mode rate limit in seconds (default 5) |

`interval` only affects the plain-text (non-terminal) rendering — it has no
effect when connected to a real terminal.

##### Style dict

The optional `style` parameter accepts a dict whose top-level keys name the
visual elements of the progress display. Each element accepts a sub-dict of
properties.

###### Elements with width and color

| Element   | Default width | Default fg | Default attrs |
| --------- | ------------- | ---------- | ------------- |
| `bar`     | 20            | `cyan`     |               |
| `message` | 30            |            | `bold`        |
| `icon`    | 2             |            | `bold`        |

###### Color-only elements

| Element    | Default fg | Default attrs |
| ---------- | ---------- | ------------- |
| `spinner`  | `cyan`     |               |
| `elapsed`  |            | `dim`         |
| `position` |            |               |
| `total`    |            |               |

###### Properties

All properties are optional.

| Property | Type                                           | Description                                   |
| -------- | ---------------------------------------------- | --------------------------------------------- |
| `width`  | [`Int`](../std/int.md)                         | Character width (bar, message, icon only)     |
| `fg`     | [`Sym`](../std/sym.md)                         | Foreground color                              |
| `bg`     | [`Sym`](../std/sym.md)                         | Background color                              |
| `attrs`  | array of `Str`                                 | Text attributes                               |
| `alt`    | dict                                           | Alt style for unfilled bar portion (bar only) |

The `alt` sub-dict accepts the same `fg`, `bg`, and `attrs` properties. The
default alt style for `bar` is `fg: :BLUE:`.

###### Color names

`:BLACK:`, `:RED:`, `:GREEN:`, `:YELLOW:`, `:BLUE:`,
`:MAGENTA:`, `:CYAN:`, and `:WHITE:`. Prefix the name with `BRIGHT_` for bright
variants, such as `:BRIGHT_CYAN:`. Use `:BRIGHT:` alone to brighten the default
color.

###### Attributes

`bold`, `dim`, `italic`, `underlined`, `blink`, `reverse`, `hidden`, and
`strikethrough`.

All keys are optional; omitted values use defaults.

#### Returns

The return value of `func`

#### Example

```
progress.with do
  progress.show total: 100 message: "downloading" do |w|
    for i = Range 100
      w.delta()

# With custom style
progress.with
  style:
    bar:
      width: 30
      fg: :GREEN:
      alt:
        fg: :BLACK:
        attrs:
          - dim
    message:
      fg: :WHITE:
      attrs:
        - bold
    position:
      fg: :YELLOW:
  do progress.show message: "working" do |w|
    # ...
```

### `show func`

Creates a progress indicator and runs `func` with it. The indicator is
automatically removed when `func` returns. When called inside another indicator
scope, the new indicator appears indented beneath the parent widget.

If `total` is provided, the indicator starts in bar mode (showing a progress
bar). Otherwise, it starts in spinner mode. The mode can be changed dynamically
by setting `total` on the indicator.

Outside a `progress.with` scope, the callback is invoked with a dummy indicator
whose methods are silent no-ops.

#### Parameters

| Name      | Type                                            | Description                                         |
| --------- | ----------------------------------------------- | --------------------------------------------------- |
| `func`    | func                                            | Callback receiving an [`Indicator`](./indicator.md) |
| `total`   | [`Int`](../std/int.md)?                         | Total value for bar mode                            |
| `message` | [`Str`](../std/str.md)?                         | Initial message                                     |
| `icon`    | [`Str`](../std/str.md)?                         | Prefix icon, e.g. "📦"                              |
| `units`   | [`Sym`](../std/sym.md)\|[`Str`](../std/str.md)? | Unit format                                         |
| `tick`    | [`Float`](../std/float.md)?                     | Tick interval in seconds (default 0.08)             |

##### Units

| Value     | Description                     |
| --------- | ------------------------------- |
| `:COUNT:` | Display as `pos/len` or `pos`   |
| `:BYTES:` | Display as human-readable bytes |

When `total` is provided and `units` is omitted, units default to `:COUNT:`.
When neither `total` nor `units` is provided, spinner mode shows only elapsed
time.

#### Returns

The return value of `func`

#### Example

```
progress.with do
  progress.show total: 3 message: "building" do |w|
    progress.show message: "step 1" do |_|
      do_step_1()
    w.delta()
```

!!! warning
    The [`Indicator`](./indicator.md) object is only valid inside its callback.
    Using it after the callback returns raises a runtime error.

### `steps ...steps :message? :icon?`

Runs a sequence of steps under a progress indicator tracking step completion.

A step can be a `func` or a `dict` containing one positional `func`
and optional `name` and `icon` keys.

```
let results = progress.steps message: building icon: "•"
  - name: compile
    icon: "C"
    do compile()
  do test()
  - name: package
    do package()
```

Before a named step runs, its name is appended to the overall message with a
colon, such as `building: compile`. An unnamed step uses the overall message
unchanged. If no overall message is provided, a named step uses its name by
itself. A step's `icon` overrides the default icon for that step.

If a step raises an error, remaining steps are skipped and the error is
propagated.

| Name      | Type                          | Description                          |
| --------- | ----------------------------- | ------------------------------------ |
| `steps`   | func\|dict*                   | Callables or annotated step specs    |
| `message` | [`Str`](../std/str.md)?       | Overall message prefix               |
| `icon`    | [`Str`](../std/str.md)?       | Default icon (default `"●"`)         |

#### Returns

An `array` containing each step's return value, in order.
