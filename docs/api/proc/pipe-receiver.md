# PipeReceiver

Receives values from a strand pipe. `PipeReceiver` is created for connections
between
[`strand.pipeline`](../strand/index.md#pipeline-stage-stages-input-output)
stages and for a [`strand.stream`](../strand/index.md#stream-func) strand's
implicit input; it cannot be constructed directly.

## Inherits

- [`Iter`](../std/iter.md)

## Methods

### `close()`

Closes the receiver and stops its sender.

### `lines()`

Selects line framing when the receiver reads bytes from an external program.
This is the default.

#### Returns

`PipeReceiver`.

### `chunks()`

Selects arbitrary-sized [`Bin`](../std/bin.md) chunks when the receiver reads
bytes from an external program.

#### Returns

`PipeReceiver`.

Both framing methods reconfigure this receiver in place. They do not alter
values sent directly by another Do pipeline stage.
