# `Flags`

NFSv4 ACE inheritance and audit flags.

## Constructor

### `Flags ...bits`

Constructs a flag set from symbols or one iterable of symbols.

#### Supported symbols

| Symbol                   | Meaning                                                                          |
| ------------------------ | -------------------------------------------------------------------------------- |
| `:FILE_INHERIT:`         | Files created within a directory inherit this entry                              |
| `:DIRECTORY_INHERIT:`    | Subdirectories created within a directory inherit this entry                     |
| `:NO_PROPAGATE_INHERIT:` | Stop propagating this entry after one level of inheritance                       |
| `:INHERIT_ONLY:`         | This entry is inherited but does not apply to the directory itself               |
| `:SUCCESSFUL_ACCESS:`    | Generate an audit/alarm event on successful access (`:AUDIT:`/`:ALARM:` entries) |
| `:FAILED_ACCESS:`        | Generate an audit/alarm event on failed access (`:AUDIT:`/`:ALARM:` entries)     |
| `:INHERITED:`            | This entry was inherited from a parent directory                                 |

```
let inherit = nfs4.Flags(:FILE_INHERIT:, :DIRECTORY_INHERIT:)
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
