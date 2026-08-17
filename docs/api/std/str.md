# Str

Strings are immutable sequences of UTF-8 bytes.

## Constructor

### `Str value`

Accepts a string or decodes UTF-8 binary data. The lowercase `str` function
instead returns the general-purpose textual representation of any value.

```
assert_eq (Str b"hello") "hello"
assert_eq (str 42) "42"
```

## Fields

### `len`

Returns the byte length of the string.

#### Type

[`Int`](./index.md)

#### Example

```
assert_eq $"hello".len 5
assert_eq $"".len 0
```

## Methods

### `starts_with prefix`

Tests whether the string starts with the given prefix.

#### Parameters

| Name     | Type              | Description       |
| -------- | ----------------- | ----------------- |
| `prefix` | [`Str`](./str.md) | the prefix string |

#### Returns

[`Bool`](./index.md)

#### Example

```
assert ("foobar".starts_with "foo")
assert (!("foobar".starts_with "bar"))
```

### `without_prefix prefix`

Returns the string with the prefix removed if it matches, otherwise returns
the original string.

#### Parameters

| Name     | Type              | Description          |
| -------- | ----------------- | -------------------- |
| `prefix` | [`Str`](./str.md) | the prefix to remove |

#### Returns

[`Str`](./str.md)

#### Example

```
assert_eq ("foobar".without_prefix "foo") "bar"
assert_eq ("foobar".without_prefix "baz") "foobar"
```

### `ends_with suffix`

Tests whether the string ends with the given suffix.

#### Parameters

| Name     | Type              | Description       |
| -------- | ----------------- | ----------------- |
| `suffix` | [`Str`](./str.md) | the suffix string |

#### Returns

[`Bool`](./index.md)

#### Example

```
assert ("foobar".ends_with "bar")
```

### `without_suffix suffix`

Returns the string with the suffix removed if it matches, otherwise returns
the original string.

#### Parameters

| Name     | Type              | Description          |
| -------- | ----------------- | -------------------- |
| `suffix` | [`Str`](./str.md) | the suffix to remove |

#### Returns

[`Str`](./str.md)

#### Example

```
assert_eq ("foobar".without_suffix "bar") "foo"
```

### `split delimiter [limit: int]`

Splits the string by the delimiter, returning an iterator that yields segments
in **left-to-right** order.

The optional `limit` controls how many splits are performed and from which end:

- `limit: N` (positive) — split at most N times from the **left**; the last
  element is the unsplit remainder.
- `limit: -N` (negative) — split at most N times from the **right**, but still
  yield segments left-to-right. Useful for splitting off a known-length suffix
  (e.g. a file extension).
- Omitted — split fully with no limit.

#### Parameters

| Name        | Type                | Description                                 |
| ----------- | ------------------- | ------------------------------------------- |
| `delimiter` | [`Str`](./str.md)   | the delimiter string                        |
| `limit`     | [`Int`](./index.md) | max splits; negative means split from right |

#### Returns

iterator of [`Str`](./str.md)

#### Example

```
assert_eq [..."a,b,c".split ","] ["a", "b", "c"]
assert_eq [..."a,b,c".split "," limit: 1] ["a", "b,c"]

# Negative limit: split from the right, yield left-to-right
let base ext = "archive.tar.gz".split "." limit: -1
assert_eq $base "archive.tar"
assert_eq $ext "gz"
```

### `rsplit delimiter [limit: int]`

Like `split`, but yields segments in **right-to-left** order (rightmost segment
first).

The optional `limit` controls how many splits are performed and from which end:

- `limit: N` (positive) — split at most N times from the **right**; the last
  element yielded is the unsplit left remainder.
- `limit: -N` (negative) — split at most N times from the **left**, but still
  yield segments right-to-left.
- Omitted — split fully with no limit.

#### Parameters

| Name        | Type                | Description                                |
| ----------- | ------------------- | ------------------------------------------ |
| `delimiter` | [`Str`](./str.md)   | the delimiter string                       |
| `limit`     | [`Int`](./index.md) | max splits; negative means split from left |

#### Returns

iterator of [`Str`](./str.md)

#### Example

```
assert_eq [..."a,b,c".rsplit ","] ["c", "b", "a"]
assert_eq [..."a,b,c".rsplit "," limit: 1] ["c", "a,b"]

# Negative limit: split from the left, yield right-to-left
assert_eq [..."a,b,c".rsplit "," limit: -1] ["b,c", "a"]
```

### `join iter?`

Joins values from an input source using this string as a separator.

#### Parameters

| Name    | Type | Description                                      |
| ------- | ---- | ------------------------------------------------ |
| `input` |      | iterable to join (uses default input if omitted) |

#### Returns

[`Str`](./str.md)

#### Example

```
assert_eq (",".join ["a", "b", "c"]) "a,b,c"
```

### `trim chars?`

Removes whitespace (or specified characters) from both ends.

#### Parameters

| Name    | Type                                           | Description                                 |
| ------- | ---------------------------------------------- | ------------------------------------------- |
| `chars` | [`Str`](./str.md)\|[`Iterable`](./iterable.md) | characters to trim (defaults to whitespace) |

The pattern is a set of *characters*, from a [`Str`](./str.md) or from each
element of an iterable of them. A [`Bin`](./bin.md) is not accepted: bytes only
become characters through a decode the caller should ask for.

#### Returns

[`Str`](./str.md)

#### Example

```
assert_eq ("  hello  ".trim()) "hello"
assert_eq ("xxhelloxx".trim "x") "hello"
assert_eq ("xyhelloyx".trim ["x", "y"]) "hello"
```

### `trim_start chars?`

Removes whitespace (or specified characters) from the start.

#### Parameters

| Name    | Type                                           | Description                                              |
| ------- | ---------------------------------------------- | -------------------------------------------------------- |
| `chars` | [`Str`](./str.md)\|[`Iterable`](./iterable.md) | characters to trim, as a set (see [`trim`](#trim-chars)) |

#### Returns

[`Str`](./str.md)

#### Example

```
assert_eq ("  hello  ".trim_start()) "hello  "
assert_eq ("xxhelloxx".trim_start "x") "helloxx"
```

### `trim_end chars?`

Removes whitespace (or specified characters) from the end.

#### Parameters

| Name    | Type                                           | Description                                              |
| ------- | ---------------------------------------------- | -------------------------------------------------------- |
| `chars` | [`Str`](./str.md)\|[`Iterable`](./iterable.md) | characters to trim, as a set (see [`trim`](#trim-chars)) |

#### Returns

[`Str`](./str.md)

#### Example

```
assert_eq ("  hello  ".trim_end()) "  hello"
assert_eq ("xxhelloxx".trim_end "x") "xxhello"
```

### `chomp`

Removes one trailing line terminator.

One complete terminator — `\r\n` or `\n`, never a lone `\r` — and nothing else.
A string without one is returned unchanged.

#### Returns

[`Str`](./str.md)

#### Example

```
assert_eq ("line\n".chomp()) "line"
assert_eq ("line\r\n".chomp()) "line"
assert_eq ("line".chomp()) "line"
```

Distinct from [`trim_end`](#trim_end-chars), which is about whitespace
generally and takes an optional character set:

```
assert_eq ("z  \n".chomp()) "z  "
assert_eq ("z  \n".trim_end()) "z"
```

[`Iter.chomp`](./iter.md#chomp) lifts this over an iterator — it is exactly
`.map do |x| x.chomp()`, with the mapping done inline.

### `contains needle`

Tests whether the string contains the given substring.

#### Parameters

| Name     | Type              | Description           |
| -------- | ----------------- | --------------------- |
| `needle` | [`Str`](./str.md) | the substring to find |

#### Returns

[`Bool`](./index.md)

#### Example

```
assert ("foobar".contains "foo")
assert ("foobar".contains "bar")
assert (!"foobar".contains "baz")
assert ("foobar".contains "")  # empty string is always contained
```

### `replace from to`

Returns a new string with all non-overlapping occurrences of `from` replaced
with `to`.

#### Parameters

| Name   | Type              | Description                |
| ------ | ----------------- | -------------------------- |
| `from` | [`Str`](./str.md) | substring to replace       |
| `to`   | [`Str`](./str.md) | replacement string         |

#### Returns

[`Str`](./str.md)

#### Example

```
assert_eq ("foo bar foo".replace "foo" "baz") "baz bar baz"
assert_eq ("banana".replace "na" "") "ba"
assert_eq ("abc".replace "" "-") "-a-b-c-"
```

### `upper`

Returns the string converted to uppercase.

#### Returns

[`Str`](./str.md)

#### Example

```
assert_eq ("hello".upper()) "HELLO"
assert_eq ("Hello World".upper()) "HELLO WORLD"
```

### `lower`

Returns the string converted to lowercase.

#### Returns

[`Str`](./str.md)

#### Example

```
assert_eq ("HELLO".lower()) "hello"
assert_eq ("Hello World".lower()) "hello world"
```

### `repeat count`

Returns the string repeated `count` times.

#### Parameters

| Name    | Type                | Description                     |
| ------- | ------------------- | ------------------------------- |
| `count` | [`Int`](./index.md) | non-negative repetition count   |

#### Returns

[`Str`](./str.md)

#### Example

```
assert_eq ("ab".repeat 3) "ababab"
assert_eq ("ab".repeat 0) ""
```

## Operations

### Indexing

Strings accept [`Range`](./range.md) values for slicing by byte position:

```
assert_eq $"abcd"[1..3] "bc"
assert_eq $"abcd"[..2] "ab"
assert_eq $"abcd"[2..] "cd"
assert_eq $"abcd"[..] "abcd"
assert_eq $"foobar"[-3..] "bar"
```

This returns a new string. Slice boundaries must still fall on valid UTF-8
boundaries. Omitted `start` means `0`, omitted `end` means the string length,
and negative endpoints count from the end.
