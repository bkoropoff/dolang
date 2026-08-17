# Range

Ranges represent a sequence of numbers from start to end (exclusive) with a
step. They are used for iteration and for slicing sequence types.

Range values can be created either with `..` syntax or by calling the `Range`
type object directly.

## Range Expressions

```
let bounded = (1..5)
let from_start = (..5)
let to_end = (1..)
let all = (..)
```

`a..b` is half-open: it includes `a` and excludes `b`.

Open-ended forms keep the missing endpoint as `nil` internally.

## Creating Ranges

Call the `Range` type object with positional or key arguments:

### Positional Arguments

```
# Just a end value (start defaults to 0, step to 1)
let r = Range 5

# Start and end
let r = Range 0 10

# With step
let r = Range 0 20 2

# Explicitly open-ended end
let r = Range 1 nil

# Fully open descriptor
let r = Range nil nil
```

### Key Arguments

```
# Just a end value (start defaults to 0, step to 1)
let r = Range end: 5

# Start and end
let r = Range start: 0 end: 10

# With step
let r = Range start: 0 end: 20 step: 2
```

### Mixed Arguments

Positional and key arguments can be mixed:

```
# Positional end with key step
let r = Range 10 step: 2

# Positional start with key end and step
let r = Range 0 end: 20 step: 5
```

#### Parameters

| Name    | Type  | Description                 | Position |
| ------- | ----- | --------------------------- | -------- |
| `start` | `Int` | starting value (default: 0) | 0        |
| `end`   | `Int` | ending value (exclusive)    | 1        |
| `step`  | `Int` | step size (default: 1)      | 2        |

Passing `nil` leaves `start` or `end` open instead of applying the usual
default. In contrast, omitting positional `start` still defaults it to `0`.

## Fields

### `start`

The starting value, or `nil` for open-start ranges.

### `end`

The ending value (exclusive), or `nil` for open-end ranges.

### `step`

The step size.

## Iteration

Ranges are iterable:

```
for i = Range 5
  echo $i
# 0, 1, 2, 3, 4

for i = Range 0 10 3
  echo $i
# 0, 3, 6, 9

for i = Range start: 1 end: 10 step: 3
  echo $i
# 1, 4, 7
```

Open-ended ranges with a concrete start are also iterable:

```
let values = []
for i = 2..
  if (i >= 5)
    break
  values.push $i
assert_eq $values [2, 3, 4]
```

`..b` and `..` are not iterable.

## Indexing

Some collection types interpret ranges as slices when used in indexing:

- [`Array`](./array.md)
- [`Tuple`](./tuple.md)
- [`Bin`](./bin.md)
- [`Str`](./str.md)

These slices return new values rather than views.

For slice indexing, omitted `start` means `0`, omitted `end` means the
collection length, and negative endpoints count from the end.

`array`, `tuple`, and `Bin` also accept non-unity integer steps. Positive steps
skip elements; negative steps produce reversed slices. For negative steps,
omitted bounds default to the end of the collection and to just before the
start, respectively. `Str` remains contiguous-only.

Other mappings such as [`Dict`](./dict.md) treat ranges as ordinary keys.

## Methods

### `contains value`

Tests whether the given value is within the Range interval. For increasing
ranges (start < end), checks if value is in \[start, end). For decreasing
ranges (start > end), checks if value is in (end, start].

#### Parameters

| Name    | Type | Description          |
| ------- | ---- | -------------------- |
| `value` |      | the value to check   |

#### Returns

`Bool`

#### Example

```
# Increasing Range [0, 5)
let r1 = Range 0 5
assert (r1.contains 0)
assert (r1.contains 4)
assert (!r1.contains 5)

# Decreasing Range (0, 5]
let r2 = Range 5 0 -1
assert (r2.contains 5)
assert (r2.contains 1)
assert (!r2.contains 0)
```
