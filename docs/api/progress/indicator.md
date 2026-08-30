# Indicator

A progress indicator created by
[`progress.show`](./index.md#show-func).

An indicator is either a progress bar (when `total` is set) or a spinner (when
`total` is not set). Setting `total` dynamically switches between the two
modes.

## Fields

### `icon`

The prefix icon (`Str`). Read-only — see
[`update`](#update-icon-message-total-position-delta-units).

### `message`

The indicator message text (`Str`). Read-only — see
[`update`](#update-icon-message-total-position-delta-units).

### `position`

The current position (`Int`). Read-only — see
[`update`](#update-icon-message-total-position-delta-units).

### `total`

The total value for bar mode (`Int`), or `nil` for spinner mode. Read-only —
see [`update`](#update-icon-message-total-position-delta-units).

## Methods

### `delta n?`

Adjusts the position by `n` (default +1). Positive values increment, negative
values decrement. Equivalent to `update delta: n` but without the overhead of
unpacking unused keys — the common case for a tight loop that only bumps
progress.

| Name | Type                   | Description                  |
| ---- | ---------------------- | ---------------------------- |
| `n`  | [`Int`](../std/int.md) | Amount to adjust (default 1) |

### `update :icon? :message? :total? :position? :delta? :units?`

Applies one or more changes atomically in a single call — one redraw instead
of one per field.

| Name       | Type                    | Description                                      |
| ---------- | ----------------------- | ------------------------------------------------ |
| `icon`     | [`Str`](../std/str.md)? | New prefix icon                                  |
| `message`  | [`Str`](../std/str.md)? | New message text                                 |
| `total`    | [`Int`](../std/int.md)? | New total; `nil` switches to spinner mode        |
| `position` | [`Int`](../std/int.md)? | Absolute position                                |
| `delta`    | [`Int`](../std/int.md)? | Relative adjustment (positive or negative)       |
| `units`    | [`Sym`](../std/sym.md)? | New `COUNT`, `BYTES`, or `PERCENT` display units |

`position` and `delta` are exclusive — passing both raises an error. Omitted
keys are left unchanged.

The first `position` an indicator is given sets where it starts rather than
counting as progress it just made, so an indicator that opens partway
through — a resumed download, say — doesn't report the opening jump as a
burst of throughput. Positions after that are measured normally.

In non-terminal (plain-text) output, `total`/`position`/`delta` changes are
rate-limited (see `progress.with`'s `interval:`), but `icon`/`message`
and `units` changes always print immediately.

```
w.update icon: 📦 message: "installing $pkg"
w.update total: 100
w.update delta: 1
w.update units: :BYTES:
w.update units: :PERCENT: # 40% when position is 4 and total is 10
```
