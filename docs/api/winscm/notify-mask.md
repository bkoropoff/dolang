# NotifyMask

Service status changes to observe.

## Constructor

### `NotifyMask ...changes`

Constructs a mask from symbols or one iterable of symbols.

#### Supported symbols

| Symbol              | Observes a change to                                       |
| ------------------- | ---------------------------------------------------------- |
| `:STOPPED:`         | The stopped state                                          |
| `:START_PENDING:`   | The start-pending state                                    |
| `:STOP_PENDING:`    | The stop-pending state                                     |
| `:RUNNING:`         | The running state                                          |
| `:CONTINUE_PENDING:`| The continue-pending state                                 |
| `:PAUSE_PENDING:`   | The pause-pending state                                    |
| `:PAUSED:`          | The paused state                                           |
| `:CREATED:`         | Service creation                                           |
| `:DELETED:`         | Service deletion                                           |
| `:DELETE_PENDING:`  | A service becoming marked for deletion                     |

## Methods

### `contains change`

Tests whether all bits represented by a symbol are set.

#### Returns

[`Bool`](../std/bool.md)

## Operators

`|`, `&`, and `^` combine masks. `~` complements a mask within the supported
bit set. Iteration yields the symbols represented by a mask.
