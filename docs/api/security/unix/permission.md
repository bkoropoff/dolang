# `Permission`

POSIX read/write/execute permission bits.

## Constructor

### `Permission ...bits`

Constructs a permission set from symbols or one iterable of symbols.

#### Supported symbols

| Symbol      | Meaning            |
| ----------- | ------------------ |
| `:READ:`    | Read permission    |
| `:WRITE:`   | Write permission   |
| `:EXECUTE:` | Execute permission |

#### Example

```
let rw = unix.Permission(:READ:, :WRITE:)
```

## Methods

### `contains bit`

Tests whether all bits represented by a symbol are set.

#### Returns

[`Bool`](../../std/bool.md)

## Operators

`|`, `&`, and `^` combine permission sets. `~` complements a permission set
within the supported bit set. `==` compares permission sets. Iteration
yields the symbols represented by a permission set.
