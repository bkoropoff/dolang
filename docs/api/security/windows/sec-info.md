# SecInfo

Security descriptor components loaded by a query.

## Constructor

### `SecInfo ...components`

Constructs flags from component symbols.

| Symbol    | Component                    |
| --------- | ---------------------------- |
| `:OWNER:` | Owner SID                    |
| `:GROUP:` | Primary group SID            |
| `:DACL:`  | Discretionary ACL            |
| `:SACL:`  | System ACL                   |
| `:ALL:`   | All supported components     |

## Methods

### `contains component`

Tests whether a component is selected.

## Operators

`|`, `&`, and `^` combine values. `~` complements a value within the supported
bit set. Iteration yields selected symbols.
