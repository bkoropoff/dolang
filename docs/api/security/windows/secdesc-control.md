# SecDescControl

Security descriptor control flags.

## Constructor

### `SecDescControl ...flags`

Constructs control flags from symbols or one iterable of symbols.

#### Supported symbols

| Symbol                         | Meaning                               |
| ------------------------------ | ------------------------------------- |
| `:OWNER_DEFAULTED:`            | Owner supplied by a default method    |
| `:GROUP_DEFAULTED:`            | Group supplied by a default method    |
| `:DACL_PRESENT:`               | DACL present                          |
| `:DACL_DEFAULTED:`             | DACL supplied by a default method     |
| `:SACL_PRESENT:`               | SACL present                          |
| `:SACL_DEFAULTED:`             | SACL supplied by a default method     |
| `:DACL_AUTO_INHERIT_REQUIRED:` | DACL requires inheritance processing  |
| `:SACL_AUTO_INHERIT_REQUIRED:` | SACL requires inheritance processing  |
| `:DACL_AUTO_INHERITED:`        | DACL was automatically inherited      |
| `:SACL_AUTO_INHERITED:`        | SACL was automatically inherited      |
| `:DACL_PROTECTED:`             | DACL blocks inheritable ACEs          |
| `:SACL_PROTECTED:`             | SACL blocks inheritable ACEs          |
| `:RM_CONTROL_VALID:`           | Resource-manager control byte valid   |
| `:SELF_RELATIVE:`              | Descriptor uses self-relative storage |

## Methods

### `contains flag`

Tests whether all bits represented by a symbol are set.

#### Returns

[`Bool`](../../std/bool.md)

## Operators

`|`, `&`, and `^` combine flags. `~` complements flags within the supported
bit set. Iteration yields the symbols represented by the flags.
