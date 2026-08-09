# AccessMask

Registry key access rights.

## Constructor

### `AccessMask ...rights`

Constructs a mask from symbols or one iterable of symbols.

#### Supported symbols

| Symbol         | Meaning                                                                                      |
| -------------- | -------------------------------------------------------------------------------------------- |
| `:READ:`       | Query values, enumerate subkeys, receive change notifications, and read security information |
| `:WRITE:`      | Set values, create subkeys, and read security information                                    |
| `:READ_WRITE:` | Combines `:READ:` and `:WRITE:`                                                              |

The generic object rights in
[`security.windows.AccessMask`](../security/windows/access-mask.md) are also
supported.

#### Example

```
let access = AccessMask :READ: :WRITE_DAC:
```

## Methods

### `contains right`

Tests whether all bits represented by a symbol are set.

#### Returns

[`Bool`](../std/bool.md)

## Operators

`|`, `&`, and `^` combine masks. `~` complements a mask within the supported
bit set. Iteration yields the symbols represented by a mask.
