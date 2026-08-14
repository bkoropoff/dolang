# Bin

Binary data; an immutable sequence of bytes.

## Fields

### `len`

Returns the byte length of the binary data.

#### Type

[`Int`](./index.md)

```
assert_eq (b"hello".len) 5
assert_eq (b"".len) 0
```

## Instance Methods

### `starts_with prefix`

Tests whether the binary data starts with the given prefix.

#### Parameters

| Name     | Type              | Description      |
| -------- | ----------------- | ---------------- |
| `prefix` | [`Bin`](./bin.md) | the prefix bytes |

#### Returns

[`Bool`](./index.md)

```
assert (b"hello".starts_with b"he")
assert (!(b"hello".starts_with b"lo"))
```

### `without_prefix prefix`

Returns the binary data with the prefix removed if it matches, otherwise
returns the original data.

#### Parameters

| Name     | Type              | Description          |
| -------- | ----------------- | -------------------- |
| `prefix` | [`Bin`](./bin.md) | the prefix to remove |

#### Returns

[`Bin`](./bin.md)

```
assert_eq (b"hello".without_prefix b"he") b"llo"
assert_eq (b"hello".without_prefix b"xx") b"hello"
```

### `ends_with suffix`

Tests whether the binary data ends with the given suffix.

#### Parameters

| Name     | Type              | Description      |
| -------- | ----------------- | ---------------- |
| `suffix` | [`Bin`](./bin.md) | the suffix bytes |

#### Returns

[`Bool`](./index.md)

```
assert (b"hello".ends_with b"lo")
```

### `without_suffix suffix`

Returns the binary data with the suffix removed if it matches, otherwise returns
the original data.

#### Parameters

| Name     | Type              | Description          |
| -------- | ----------------- | -------------------- |
| `suffix` | [`Bin`](./bin.md) | the suffix to remove |

#### Returns

[`Bin`](./bin.md)

```
assert_eq (b"hello".without_suffix b"lo") b"hel"
```

### `split delimiter [limit: int]`

Splits the binary data by the delimiter, returning an iterator that yields
segments in **left-to-right** order.

The optional `limit` works identically to
[`str.split`](./str.md#split-delimiter-limit-int): positive splits from the
left, negative splits from the right (but still yields left-to-right).

#### Parameters

| Name        | Type                | Description                                 |
| ----------- | ------------------- | ------------------------------------------- |
| `delimiter` | [`Bin`](./bin.md)   | the delimiter bytes                         |
| `limit`     | [`Int`](./index.md) | max splits; negative means split from right |

#### Returns

iterator of [`Bin`](./bin.md)

```
assert_eq [...b"a,b,c".split b","] [b"a", b"b", b"c"]
assert_eq [...b"a,b,c".split b"," limit: 1] [b"a", b"b,c"]
let base ext = b"archive.tar.gz".split b"." limit: -1
assert_eq $base b"archive.tar"
assert_eq $ext b"gz"
```

### `rsplit delimiter [limit: int]`

Like `split`, but yields segments in **right-to-left** order. Mirrors
[`str.rsplit`](./str.md#rsplit-delimiter-limit-int).

#### Parameters

| Name        | Type                | Description                                |
| ----------- | ------------------- | ------------------------------------------ |
| `delimiter` | [`Bin`](./bin.md)   | the delimiter bytes                        |
| `limit`     | [`Int`](./index.md) | max splits; negative means split from left |

#### Returns

iterator of [`Bin`](./bin.md)

```
assert_eq [...b"a,b,c".rsplit b","] [b"c", b"b", b"a"]
assert_eq [...b"a,b,c".rsplit b"," limit: 1] [b"c", b"a,b"]
```

### `join iter?`

Joins values from an input source using this binary data as a separator.

#### Parameters

| Name    | Type | Description                                      |
| ------- | ---- | ------------------------------------------------ |
| `input` |      | iterable to join (uses default input if omitted) |

#### Returns

[`Bin`](./bin.md)

```
assert_eq (b",".join [b"a", b"b", b"c"]) b"a,b,c"
```

### `trim chars?`

Removes bytes (or specified characters) from both ends.

#### Parameters

| Name    | Type                                           | Description                                  |
| ------- | ---------------------------------------------- | -------------------------------------------- |
| `chars` | [`Bin`](./bin.md)\|[`Iterable`](./iterable.md) | bytes to trim (defaults to whitespace bytes) |

The pattern is a set of *bytes*, from a [`Bin`](./bin.md) or from each element
of an iterable of them, so bytes that are not valid UTF-8 are a pattern like
any other. A [`Str`](./str.md) is not accepted: in a set, a multi-byte
character would be split into bytes that mean nothing on their own.

#### Returns

[`Bin`](./bin.md)

```
assert_eq (b"  hello  ".trim()) b"hello"
assert_eq (b"xxhelloxx".trim b"x") b"hello"
assert_eq (b"xyhelloyx".trim [b"x", b"y"]) b"hello"
assert_eq (b"\x00\xffdata\xff\x00".trim b"\x00\xff") b"data"
```

### `trim_start chars?`

Removes bytes (or specified characters) from the start.

#### Parameters

| Name    | Type                                           | Description                                         |
| ------- | ---------------------------------------------- | --------------------------------------------------- |
| `chars` | [`Bin`](./bin.md)\|[`Iterable`](./iterable.md) | bytes to trim, as a set (see [`trim`](#trim-chars)) |

#### Returns

[`Bin`](./bin.md)

```
assert_eq (b"  hello  ".trim_start()) b"hello  "
assert_eq (b"xxhelloxx".trim_start b"x") b"helloxx"
```

### `trim_end chars?`

Removes bytes (or specified characters) from the end.

#### Parameters

| Name    | Type                                           | Description                                         |
| ------- | ---------------------------------------------- | --------------------------------------------------- |
| `chars` | [`Bin`](./bin.md)\|[`Iterable`](./iterable.md) | bytes to trim, as a set (see [`trim`](#trim-chars)) |

#### Returns

[`Bin`](./bin.md)

```
assert_eq (b"  hello  ".trim_end()) b"  hello"
assert_eq (b"xxhelloxx".trim_end b"x") b"xxhello"
```

### `chomp`

Removes one trailing line terminator.

One complete terminator — `\r\n` or `\n`, never a lone `\r` — and nothing else.
Data without one is returned unchanged.

#### Returns

[`Bin`](./bin.md)

```
assert_eq (b"line\n".chomp()) b"line"
assert_eq (b"line\r\n".chomp()) b"line"
```

[`Iter.chomp`](./iter.md#chomp) lifts this over an iterator — it is exactly
`.map do |x| x.chomp()`, with the mapping done inline. Distinct from
[`trim_end`](#trim_end-chars), which strips whitespace generally.

### `contains needle`

Tests whether the binary data contains the given bytes.

#### Parameters

| Name     | Type              | Description       |
| -------- | ----------------- | ----------------- |
| `needle` | [`Bin`](./bin.md) | the bytes to find |

#### Returns

[`Bool`](./index.md)

```
assert (b"hello".contains b"ell")
assert (b"hello".contains b"lo")
assert (!(b"hello".contains b"world"))
assert (b"hello".contains b"")
```

### `unpack`

Unpacks binary data into an array of byte values (integers from 0-255).

#### Returns

[`Array`](./array.md) of [`Int`](./index.md)

```
let bytes = b"hello"
assert_eq $bytes.unpack() [104, 101, 108, 108, 111]
```

### `hex`

Returns the binary data as a lowercase hexadecimal string.

#### Returns

[`Str`](./str.md)

```
assert_eq (b"ABC".hex()) "414243"
assert_eq (b"\x00\x01\xff".hex()) "0001ff"
```

## Operations

### Indexing

Binary data accepts [`Range`](./range.md) values for slicing by byte position:

```
assert_eq (b"abcd"[1..3]) b"bc"
assert_eq (b"abcd"[..2]) b"ab"
assert_eq (b"abcd"[2..]) b"cd"
assert_eq (b"abcd"[..]) b"abcd"
assert_eq (b"foobar"[-3..]) b"bar"
assert_eq (b"abcd"[Range 0 4 2]) b"ac"
assert_eq (b"abcd"[Range nil nil -1]) b"dcba"
```

This returns a new binary value. Slice boundaries must be in bounds. Omitted
`start` means `0`, omitted `end` means the binary length, and negative
endpoints count from the end. Negative steps reverse the slice.

## Constructors

### `Bin value`

Accepts binary data or copies the UTF-8 bytes of a string.

#### Parameters

| Name    | Type                                 | Description  |
| ------- | ------------------------------------ | ------------ |
| `value` | [`Bin`](./bin.md)\|[`Str`](./str.md) | source bytes |

```
let data = Bin "hello"
assert_eq $data b"hello"
```

## Class Methods

### `pack array`

Packs an array of integers (0-255) into binary data.

#### Parameters

| Name    | Type                  | Description               |
| ------- | --------------------- | ------------------------- |
| `array` | [`Array`](./array.md) | array of integers (0-255) |

#### Returns

[`Bin`](./bin.md)

```
let bytes = bin.pack [104, 101, 108, 108, 111]
assert_eq $bytes b"hello"
```

### `unpack value`

Unpacks any value that can be converted to binary into an array of byte values.

#### Parameters

| Name    | Type | Description                        |
| ------- | ---- | ---------------------------------- |
| `value` |      | value to unpack (converted to bin) |

#### Returns

[`Array`](./array.md) of [`Int`](./index.md)

```
assert_eq (bin.unpack b"hello") [104, 101, 108, 108, 111]
```
