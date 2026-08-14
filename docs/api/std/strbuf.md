# StrBuf

Mutable UTF-8 string buffer; the mutable counterpart to [`Str`](./str.md).

## Constructor

### `StrBuf initial?`

Builds a buffer, optionally seeded with the contents of an existing string.

#### Parameters

| Name      | Type               | Description      |
| --------- | ------------------ | ---------------- |
| `initial` | [`Str`](./str.md)? | initial contents |

```
let buf = StrBuf("hello")
assert_eq $buf.len 5
```

## Fields

### `len`

Returns the byte length of the buffer's current contents.

#### Type

[`Int`](./index.md)

```
assert_eq (StrBuf("hello").len) 5
```

## Methods

### `append value`

Appends `value` to the buffer. `Str` input is copied byte-for-byte; anything
else is converted the same way [`str`](./index.md#str-value) would convert
it, written directly into the buffer.

#### Parameters

| Name    | Type | Description     |
| ------- | ---- | --------------- |
| `value` |      | value to append |

```
let buf = StrBuf()
buf.append "foo"
buf.append 42
assert_eq $buf.freeze() "foo42"
```

### `extend value`

Appends the raw bytes of `value`, which must be a [`Str`](./str.md).

#### Parameters

| Name    | Type              | Description      |
| ------- | ----------------- | ---------------- |
| `value` | [`Str`](./str.md) | string to append |

```
let buf = StrBuf("foo")
buf.extend "bar"
assert_eq $buf.freeze() "foobar"
```

### `insert index value`

Inserts `value` at the given byte index, shifting the rest of the buffer
right in place.

#### Parameters

| Name    | Type                | Description                                    |
| ------- | ------------------- | ---------------------------------------------- |
| `index` | [`Int`](./index.md) | insertion point; must fall on a UTF-8 boundary |
| `value` | [`Str`](./str.md)   | string to insert                               |

```
let buf = StrBuf("foobar")
buf.insert 3 "XYZ"
assert_eq $buf.freeze() "fooXYZbar"
```

### `remove range`

Removes and returns the substring covered by `range`, shifting the rest of
the buffer left in place.

#### Parameters

| Name    | Type                  | Description                                         |
| ------- | --------------------- | --------------------------------------------------- |
| `range` | [`Range`](./range.md) | byte range to remove; must fall on UTF-8 boundaries |

#### Returns

[`Str`](./str.md)

```
let buf = StrBuf("foobar")
assert_eq (buf.remove (1..3)) "oo"
assert_eq $buf.freeze() "fbar"
```

Unlike `insert`, `remove` only accepts a range — a scalar index would remove
a single UTF-8 code unit, which is rarely a useful result.

### `truncate len`

Shrinks the buffer to `len` bytes, discarding anything past that point.

#### Parameters

| Name  | Type                | Description                               |
| ----- | ------------------- | ----------------------------------------- |
| `len` | [`Int`](./index.md) | new length; must fall on a UTF-8 boundary |

```
let buf = StrBuf("foobar")
buf.truncate 3
assert_eq $buf.freeze() "foo"
```

### `clear`

Empties the buffer, retaining its allocated capacity.

```
let buf = StrBuf("foobar")
buf.clear()
assert_eq $buf.len 0
buf.append "x"
assert_eq $buf.freeze() "x"
```

### `freeze`

Converts the buffer's current contents into an immutable [`Str`](./str.md)
in place, without copying, and empties the buffer. The buffer stays usable
afterward, and the returned string is unaffected by later mutation.

#### Returns

[`Str`](./str.md)

```
let buf = StrBuf("abc")
let frozen = buf.freeze()
assert_eq $frozen "abc"
assert_eq $buf.len 0
buf.append "def"
assert_eq $frozen "abc"
```

### `drain size?`

Returns an iterator that removes and yields up to `size` bytes at a time
from the front of the buffer as it's consumed.

Each chunk boundary is rounded down to the nearest UTF-8 code point
boundary, unless that would produce an empty chunk (`size` smaller than the
first remaining code point), in which case it rounds up instead so every
chunk yielded is non-empty.

#### Parameters

| Name   | Type                 | Description                                   |
| ------ | -------------------- | --------------------------------------------- |
| `size` | [`Int`](./index.md)? | maximum bytes per chunk (defaults to 512 KiB) |

#### Returns

iterator of [`Str`](./str.md)

```
let buf = StrBuf("hello world")
assert_eq [...buf.drain(4)] ["hell", "o wo", "rld"]
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

| Name     | Type              | Description       |
| -------- | ----------------- | ----------------- |
| `prefix` | [`Str`](./str.md) | the prefix string |

#### Returns

[`Bool`](./index.md)

```
assert (StrBuf("foobar").starts_with "foo")
```

### `ends_with suffix`

Tests whether the buffer's contents end with the given suffix.

#### Parameters

| Name     | Type              | Description       |
| -------- | ----------------- | ----------------- |
| `suffix` | [`Str`](./str.md) | the suffix string |

#### Returns

[`Bool`](./index.md)

```
assert (StrBuf("foobar").ends_with "bar")
```

### `contains needle`

Tests whether the buffer's contents contain the given substring.

#### Parameters

| Name     | Type              | Description           |
| -------- | ----------------- | --------------------- |
| `needle` | [`Str`](./str.md) | the substring to find |

#### Returns

[`Bool`](./index.md)

```
assert (StrBuf("foobar").contains "oob")
```

### `sub start end?`

Returns a copy of the substring from `start` to `end` (or to the end of the
buffer if omitted), without modifying the buffer.

#### Parameters

| Name    | Type                 | Description           |
| ------- | -------------------- | --------------------- |
| `start` | [`Int`](./index.md)  | start index           |
| `end`   | [`Int`](./index.md)? | end index (exclusive) |

#### Returns

[`Str`](./str.md)

```
let buf = StrBuf("foobar")
assert_eq (buf.sub 2) "obar"
assert_eq (buf.sub 2 4) "ob"
```

## Operations

### Indexing

`StrBuf` supports only range indexing and assignment, matching `Str` — a
single UTF-8 code unit is rarely a useful result on its own:

```
let buf = StrBuf("foobar")
assert_eq $buf[2..4] "ob"
buf[1..3] = "XYZ"
assert_eq $buf.freeze() "fXYZbar"
```

Range assignment may grow or shrink the buffer: the replacement doesn't need
to be the same length as the range it replaces. Both the range and the
assigned value's boundaries must land on UTF-8 code point boundaries.

### Sink

`StrBuf` is a [sink](./sink.md), so it can be the target of a pipeline, of
[`strand.put`](../strand/index.md), or of a
[`run`](../proc-run.md#io-redirection) redirect. `put` is
[`append`](#append-value): the value's string form goes in verbatim, with no
line terminator added.

```
let buf = StrBuf()
run.uname -r stdout: $buf
```

Framing is the caller's to choose, which is what lets a `StrBuf` stand in for
any other sink without changing what gets written:

```
let buf = StrBuf()
let out = buf.precrimp()
out.put "first"
out.put "second"
assert_eq $buf.freeze() "first\nsecond\n"
```
