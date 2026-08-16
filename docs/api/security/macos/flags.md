# `Flags`

macOS extended ACE inheritance flags.

## Constructor

### `Flags ...bits`

Constructs a flag set from symbols or one iterable of symbols.

#### Supported symbols

| Symbol                | Meaning                                                            |
| --------------------- | ------------------------------------------------------------------ |
| `:FILE_INHERIT:`      | Files created within a directory inherit this entry                |
| `:DIRECTORY_INHERIT:` | Subdirectories created within a directory inherit this entry       |
| `:LIMIT_INHERIT:`     | Stop propagating this entry after one level of inheritance         |
| `:ONLY_INHERIT:`      | This entry is inherited but does not apply to the directory itself |
| `:INHERITED:`         | This entry was inherited from a parent directory                   |

```
let inherit = macos.Flags(:FILE_INHERIT:, :DIRECTORY_INHERIT:)
```

## Methods

### `contains bit`

Tests whether all bits represented by a symbol are set.

#### Returns

[`Bool`](../../std/bool.md)

## Operators

`|`, `&`, and `^` combine flag sets. `~` complements a flag set within the
supported bit set. `==` compares flag sets. Iteration yields the symbols
represented by a flag set.
