# `Mask`

NFSv4 ACE permission bits.

## Constructor

### `Mask ...bits`

Constructs a mask from symbols or one iterable of symbols.

#### Supported symbols

| Symbol                | Meaning                                                |
| --------------------- | ------------------------------------------------------ |
| `:READ_DATA:`         | Read the file's data, or list a directory              |
| `:WRITE_DATA:`        | Write the file's data, or create a file in a directory |
| `:APPEND_DATA:`       | Append to the file's data, or create a subdirectory    |
| `:READ_NAMED_ATTRS:`  | Read named attributes                                  |
| `:WRITE_NAMED_ATTRS:` | Write named attributes                                 |
| `:EXECUTE:`           | Execute the file, or traverse a directory              |
| `:DELETE_CHILD:`      | Delete a file or directory within a directory          |
| `:READ_ATTRIBUTES:`   | Read basic attributes                                  |
| `:WRITE_ATTRIBUTES:`  | Write basic attributes                                 |
| `:DELETE:`            | Delete the file or directory                           |
| `:READ_ACL:`          | Read the ACL                                           |
| `:WRITE_ACL:`         | Write the ACL                                          |
| `:WRITE_OWNER:`       | Change owner and owning group                          |
| `:SYNCHRONIZE:`       | Use synchronous I/O                                    |

#### Example

```
let rw = nfs4.Mask(:READ_DATA:, :WRITE_DATA:)
```

## Methods

### `contains bit`

Tests whether all bits represented by a symbol are set.

#### Returns

[`Bool`](../../std/bool.md)

## Operators

`|`, `&`, and `^` combine masks. `~` complements a mask within the supported
bit set. `==` compares masks. Iteration yields the symbols represented by a
mask.
