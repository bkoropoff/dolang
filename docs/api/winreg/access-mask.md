# AccessMask

Registry key access rights.

## Constructor

### `AccessMask ...rights`

Constructs a mask from symbols or one iterable of symbols.

#### Supported symbols

| Symbol                 | Meaning                                                                                      |
| ---------------------- | -------------------------------------------------------------------------------------------- |
| `:QUERY_VALUE:`        | Queries key values                                                                           |
| `:SET_VALUE:`          | Sets key values                                                                              |
| `:CREATE_SUB_KEY:`     | Creates subkeys                                                                              |
| `:ENUMERATE_SUB_KEYS:` | Enumerates subkeys                                                                           |
| `:NOTIFY:`             | Receives change notifications                                                                |
| `:CREATE_LINK:`        | Creates symbolic-link keys                                                                   |
| `:WOW64_64KEY:`        | Uses the 64-bit registry view                                                                |
| `:WOW64_32KEY:`        | Uses the 32-bit registry view                                                                |
| `:READ:`               | Query values, enumerate subkeys, receive change notifications, and read security information |
| `:WRITE:`              | Set values, create subkeys, and read security information                                    |
| `:READ_WRITE:`         | Combines `:READ:` and `:WRITE:`                                                              |

The generic object rights in
[`security.windows.AccessMask`](../security/windows/access-mask.md) are also
supported.

#### Example

```
let access = AccessMask :READ: :WRITE_DAC:
```

## Fields

### `int`

Returns the complete native mask as an integer, including unknown bits.

## Class Methods

### `from_int value`

Constructs a mask from a native integer while preserving every bit.

## Methods

### `contains right`

Tests whether all bits represented by a symbol are set.

#### Returns

[`Bool`](../std/bool.md)

## Operators

`|`, `&`, and `^` combine masks. `~` complements a mask within the supported
bit set. Iteration yields the symbols represented by a mask.
