# PipeSender

Sends values to a strand pipe. `PipeSender` is created for connections between
[`strand.pipeline`](../strand/index.md#pipeline-stage-stages-input-output)
stages and for a [`strand.stream`](../strand/index.md#stream-func) strand's
implicit output; it cannot be constructed directly.

## Inherits

- [`Sink`](../std/sink.md)

## Methods

### `close error? :backtrace?`

Closes the sender, signaling end of input to its receiver.

If `error` is provided, the receiver re-raises it after draining any buffered
values.

#### Parameters

| Name        | Type                                      | Description                       |
| ----------- | ----------------------------------------- | --------------------------------- |
| `error`     |                                           | Optional error value to propagate |
| `backtrace` | [`strand.Backtrace`](../strand/index.md)? | Backtrace for `error`             |
