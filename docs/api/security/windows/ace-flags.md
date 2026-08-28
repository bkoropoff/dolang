# AceFlags

Flags stored in an ACE header.

## Constructor

### `AceFlags ...flags`

Constructs flags from symbols or one iterable of symbols.

#### Supported symbols

| Symbol                   | Meaning                                                 |
| ------------------------ | ------------------------------------------------------- |
| `:OBJECT_INHERIT:`       | Non-container child objects inherit the entry           |
| `:CONTAINER_INHERIT:`    | Container child objects inherit the entry               |
| `:NO_PROPAGATE_INHERIT:` | Inherited copies stop propagating after one generation  |
| `:INHERIT_ONLY:`         | The entry applies only through inheritance              |
| `:INHERITED:`            | The entry was inherited                                 |
| `:CRITICAL:`             | The entry is critical and cannot be removed             |
| `:SUCCESSFUL_ACCESS:`    | An audit or alarm entry selects successful access       |
| `:FAILED_ACCESS:`        | An audit or alarm entry selects failed access           |

The trust-protected-filter flag occupies the same bit as `:SUCCESSFUL_ACCESS:`
and has no symbol of its own. Read it from an access-filter entry's
[`trust_protected_filter`](./ace.md#trust_protected_filter) field.

The [`Ace`](./ace.md#ace-allow-deny-audit-mask-options) constructor sets the
outcome flags from its own `successful` and `failed` parameters and rejects
them in `flags`.

## Class Methods

### `from_int value`

Constructs flags from a native integer while preserving unknown bits.

## Fields

### `int`

Returns the native integer flags.

## Methods

### `contains flag`

Tests whether all bits represented by a symbol are set.

#### Returns

[`Bool`](../../std/bool.md)

## Operators

`|`, `&`, and `^` combine flags. `~` complements a value within the supported
bit set. Iteration yields the symbols represented by a value.
