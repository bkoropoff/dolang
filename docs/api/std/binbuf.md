# BinBuf

Mutable byte buffer; the mutable counterpart to [`Bin`](./bin.md).

## Constructor

### `BinBuf initial?`

Builds a buffer, optionally seeded with the contents of existing binary data.

#### Parameters

| Name      | Type               | Description      |
| --------- | ------------------ | ---------------- |
| `initial` | [`Bin`](./bin.md)? | initial contents |

```
let buf = BinBuf(b"hello")
assert_eq $buf.len 5
```

## Fields

### `len`

Returns the byte length of the buffer's current contents.

#### Type

[`Int`](./index.md)

```
assert_eq (BinBuf(b"hello").len) 5
```

## Methods

### `append value`

Appends `value` to the buffer. `Str`/`Bin` input is copied byte-for-byte
(round-tripping non-UTF-8 bytes exactly); anything else is converted the
same way [`str`](./index.md#str-value) would convert it, written directly
into the buffer.

#### Parameters

| Name    | Type | Description     |
| ------- | ---- | --------------- |
| `value` |      | value to append |

```
let buf = BinBuf()
buf.append b"foo"
buf.append "bar"
buf.append 42
assert_eq $buf.freeze() b"foobar42"
```

### `push ...bytes`

Appends one or more raw byte values to the buffer.

#### Parameters

| Name       | Type                        | Description           |
| ---------- | --------------------------- | --------------------- |
| `...bytes` | [`Int`](./index.md) (0-255) | byte values to append |

```
let buf = BinBuf()
buf.push 104 105
assert_eq $buf.freeze() b"hi"
```

### `extend value`

Appends the raw bytes of `value`, which must be a [`Str`](./str.md) or
[`Bin`](./bin.md).

#### Parameters

| Name    | Type                                 | Description     |
| ------- | ------------------------------------ | --------------- |
| `value` | [`Bin`](./bin.md)\|[`Str`](./str.md) | bytes to append |

```
let buf = BinBuf(b"foo")
buf.extend b"bar"
assert_eq $buf.freeze() b"foobar"
```

### `insert index value`

Inserts `value` at the given byte index, shifting the rest of the buffer
right in place. `value` may be a single byte value, or a `Str`/`Bin` slice.

#### Parameters

| Name    | Type                                                              | Description             |
| ------- | ----------------------------------------------------------------- | ----------------------- |
| `index` | [`Int`](./index.md)                                               | insertion point         |
| `value` | [`Int`](./index.md) (0-255)\|[`Bin`](./bin.md)\|[`Str`](./str.md) | byte or bytes to insert |

```
let buf = BinBuf(b"foobar")
buf.insert 3 b"XYZ"
assert_eq $buf.freeze() b"fooXYZbar"
buf.insert 0 65
assert_eq $buf.freeze() b"AfooXYZbar"
```

### `remove index_or_range`

Removes and returns a byte or a range of bytes, shifting the rest of the
buffer left in place. An [`Int`](./index.md) index removes and returns a
single byte; a [`Range`](./range.md) removes and returns a [`Bin`](./bin.md)
of the removed bytes.

#### Parameters

| Name             | Type                                       | Description                   |
| ---------------- | ------------------------------------------ | ----------------------------- |
| `index_or_range` | [`Int`](./index.md)\|[`Range`](./range.md) | byte index or range to remove |

#### Returns

[`Int`](./index.md) or [`Bin`](./bin.md), matching the argument's kind

```
let buf = BinBuf(b"foobar")
assert_eq (buf.remove 0) 102
assert_eq $buf.freeze() b"oobar"

let buf2 = BinBuf(b"foobar")
assert_eq (buf2.remove (1..3)) b"oo"
assert_eq $buf2.freeze() b"fbar"
```

### `truncate len`

Shrinks the buffer to `len` bytes, discarding anything past that point.

#### Parameters

| Name  | Type                | Description |
| ----- | ------------------- | ----------- |
| `len` | [`Int`](./index.md) | new length  |

```
let buf = BinBuf(b"foobar")
buf.truncate 3
assert_eq $buf.freeze() b"foo"
```

### `clear`

Empties the buffer, retaining its allocated capacity.

```
let buf = BinBuf(b"foobar")
buf.clear()
assert_eq $buf.len 0
buf.append b"x"
assert_eq $buf.freeze() b"x"
```

### `freeze`

Converts the buffer's current contents into an immutable [`Bin`](./bin.md)
in place, without copying, and empties the buffer. The buffer stays usable
afterward, and the returned value is unaffected by later mutation.

#### Returns

[`Bin`](./bin.md)

```
let buf = BinBuf(b"abc")
let frozen = buf.freeze()
assert_eq $frozen b"abc"
assert_eq $buf.len 0
buf.append b"def"
assert_eq $frozen b"abc"
```

### `drain size?`

Returns an iterator that removes and yields up to `size` bytes at a time
from the front of the buffer as it's consumed.

#### Parameters

| Name   | Type                 | Description                                   |
| ------ | -------------------- | --------------------------------------------- |
| `size` | [`Int`](./index.md)? | maximum bytes per chunk (defaults to 512 KiB) |

#### Returns

iterator of [`Bin`](./bin.md)

```
let buf = BinBuf(b"hello world")
assert_eq [...buf.drain(4)] [b"hell", b"o wo", b"rld"]
assert_eq $buf.len 0
```

Draining never shifts the buffer's remaining contents proportionally to
their length: consumed bytes are dropped by advancing an internal cursor,
not by copying the tail down on every chunk. Other mutating methods (and
`freeze`) settle this cursor transparently before they run, so indices
passed to them are always relative to the buffer's current (undrained)
content. Multiple `drain` iterators created from the same buffer share this
cursor, so they interleave draining the same content rather than each
redraining it independently.

### `starts_with prefix`

Tests whether the buffer's contents start with the given prefix.

#### Parameters

| Name     | Type              | Description      |
| -------- | ----------------- | ---------------- |
| `prefix` | [`Bin`](./bin.md) | the prefix bytes |

#### Returns

[`Bool`](./index.md)

```
assert (BinBuf(b"foobar").starts_with b"foo")
```

### `ends_with suffix`

Tests whether the buffer's contents end with the given suffix.

#### Parameters

| Name     | Type              | Description      |
| -------- | ----------------- | ---------------- |
| `suffix` | [`Bin`](./bin.md) | the suffix bytes |

#### Returns

[`Bool`](./index.md)

```
assert (BinBuf(b"foobar").ends_with b"bar")
```

### `contains needle`

Tests whether the buffer's contents contain the given bytes.

#### Parameters

| Name     | Type              | Description       |
| -------- | ----------------- | ----------------- |
| `needle` | [`Bin`](./bin.md) | the bytes to find |

#### Returns

[`Bool`](./index.md)

```
assert (BinBuf(b"foobar").contains b"oob")
```

### `sub start end?`

Returns a copy of the byte range from `start` to `end` (or to the end of
the buffer if omitted), without modifying the buffer.

#### Parameters

| Name    | Type                 | Description           |
| ------- | -------------------- | --------------------- |
| `start` | [`Int`](./index.md)  | start index           |
| `end`   | [`Int`](./index.md)? | end index (exclusive) |

#### Returns

[`Bin`](./bin.md)

```
let buf = BinBuf(b"foobar")
assert_eq (buf.sub 2) b"obar"
assert_eq (buf.sub 2 4) b"ob"
```

### `hex`

Returns the buffer's contents as a lowercase hexadecimal string, without
modifying the buffer.

#### Returns

[`Str`](./str.md)

```
assert_eq (BinBuf(b"ABC").hex()) "414243"
```

## Operations

### Indexing

Unlike `StrBuf`, `BinBuf` supports both scalar and range indexing and
assignment:

```
let buf = BinBuf(b"foobar")
assert_eq $buf[0] 102
assert_eq $buf[2..4] b"ob"
buf[0] = 70
buf[1..3] = b"XYZ"
assert_eq $buf.freeze() b"FXYZbar"
```

Range assignment may grow or shrink the buffer: the replacement doesn't need
to be the same length as the range it replaces.
