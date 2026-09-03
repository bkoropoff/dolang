# `State`

Supertype for stateful digest handles.

Subtype of [`Sink`](../std/sink.md). Putting a [`Str`](../std/str.md)
or [`Bin`](../std/bin.md) value updates the digest state with its bytes.

## Methods

### `digest()`

Returns the current digest bytes without consuming the handle.

#### Returns

[`Bin`](../std/bin.md) - Digest snapshot

#### Example

```
let state = Blake3()
state.update "abc"
let first = state.digest()
let second = state.digest()
assert_eq $first $second
state.update "def"
assert_eq (str (fmt (state.digest()) kind: :HEX:))
  b22b3b2ee0e7c0a8e75a988d1d7e874e3c6de8b00a4427a47887877454b45db1
```

### `update data`

Updates the digest state with the bytes of `data`.

#### Parameters

| Name   | Type                                           | Description     |
| ------ | ---------------------------------------------- | --------------- |
| `data` | [`Str`](../std/str.md)\|[`Bin`](../std/bin.md) | Input to hash   |

#### Returns

The same handle, for chaining.

#### Example

```
let state = Blake3()
state.update "ab"
state.update b"c"
assert_eq (str (fmt (state.digest()) kind: :HEX:))
  6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85

let sink = Blake3()
sink.put "ab"
sink.put b"c"
assert_eq $sink.digest() (blake3 "abc")
```
