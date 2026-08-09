# NotifyMask

Service status changes to observe.

## Constructor

### `NotifyMask ...changes`

Constructs a mask from symbols or one iterable of symbols.

Supported symbols are `:STOPPED:`, `:START_PENDING:`, `:STOP_PENDING:`,
`:RUNNING:`, `:CONTINUE_PENDING:`, `:PAUSE_PENDING:`, `:PAUSED:`, `:CREATED:`,
`:DELETED:`, and `:DELETE_PENDING:`.

## Methods

### `contains change`

Tests whether all bits represented by a symbol are set.

**Returns:** [`Bool`](../std/bool.md)

## Operators

`|`, `&`, and `^` combine masks. `~` complements a mask within the supported
bit set. Iteration yields the symbols represented by a mask.
