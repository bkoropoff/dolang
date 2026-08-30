# Iter

Abstract type for iterators. Values that implement the iteration protocol
are instances of `Iter`, which can be used for type testing.

`Iter` is not constructible directly. See
[Classes](../../language/classes.md) for defining custom iterators.

```
assert_eq (type $ [1, 2, 3].iter()) $Iter
```

## Inherits

- [`Iterable`](./iterable.md)

## Methods

### `all pred?`

Returns `true` if every yielded value is truthy.

When `pred` is provided, it tests `pred(value)` instead.

Empty iterators return `true`.

### `any pred?`

Returns `true` if any yielded value is truthy.

When `pred` is provided, it tests `pred(value)` instead.

Empty iterators return `false`.

### `chain ...values`

Returns an iterator that yields this iterator followed by each additional
iterable in sequence.

### `chomp`

Creates a wrapper `Iter` which removes one trailing line terminator (`\r\n` or
`\n`) from each item, if present.

#### Errors

Getting an item raises [`TypeError`](./type-error.md) if the underlying
`Iter` yields neither a `Str` nor a `Bin`.

#### Example

```
assert_eq [...["a\n", "b\r\n", "c"].chomp()] ["a", "b", "c"]

for line = shell.stdin.chomp()
  echo "got $line"
```

Distinct from [`Str.trim_end`](./str.md), which is about whitespace generally
and takes an optional character set. `chomp` is about a line terminator
specifically and takes nothing.

### `count`

Consumes the iterator and returns the number of yielded values.

### `crimp terminator?`

Creates a wrapper `Iter` which appends a line terminator to each item.

The inverse of [`chomp`](#chomp), and the usual way to terminate values on
their way into a byte stream.

The terminator is appended **unconditionally**: an item that already ends in
one gets a second.

#### Parameters

| Name         | Type                                  | Description                         |
| ------------ | ------------------------------------- | ----------------------------------- |
| `terminator` | [`Str`](./str.md)\|[`Bin`](./bin.md)? | Appended to each item; default `\n` |

Items must be [`Str`](./str.md) or [`Bin`](./bin.md), or a type error will
result. Pass [`shell.line_ending()`](../shell/index.md#line_ending) for the
current target's native ending.

#### Errors

Raises [`TypeError`](./type-error.md) for an item, or a terminator, that is
neither a `Str` nor a `Bin`, or if a `Bin` terminator would leave a `Str` item
invalid UTF-8.

#### Example

```
assert_eq [...["a", "b"].crimp()] ["a\n", "b\n"]
assert_eq [...["a"].crimp("\r\n")] ["a\r\n"]

run cmd stdin: (["one", "two"].crimp())
```

### `enumerate`

Returns an iterator that yields `[index, value]` tuples.

The first index is `0`.

### `filter pred`

Creates a wrapper `Iter` which yields each `value` from the wrapper iterator
only if `pred(value)` is truthy.

### `find pred :default? :else?`

Consumes the iterator and returns the first value where `pred(value)` is truthy.

#### Parameters

| Name      | Type | Description                              |
| --------- | ---- | ---------------------------------------- |
| `pred`    |      | function used to test values             |
| `default` |      | value to return if no value matches      |
| `else`    |      | function to invoke if no value matches   |

#### Errors

Raises [`RuntimeError`](./runtime-error.md) if no value matches and
no fallback is provided.

### `fold init func`

Consumes the iterator left-to-right, repeatedly applying `func(acc, value)`.

Returns `init` unchanged if the iterator is empty.

### `kv`

Returns an iterator wrapper that preserves normal iteration, but opts into
key/value spreading.

When spread in a keyed context such as a dict literal or argument spread, each
yielded item must unpack into exactly two values.

```
let entries = ["x=1", "y=2"].iter().map do |e| e.split "="

assert_eq {...entries.kv()} {"x": "1", "y": "2"}
```

### `map func`

Creates a wrapper `Iter` which yields `func(value)` for each `value` yielded by
the wrapper iterator.

### `max :default?`

Consumes the iterator and returns the maximum yielded value.

#### Errors

Raises [`IterStop`](./iter-stop.md) if the iterator is empty and no
`default:` is provided.

### `min :default?`

Consumes the iterator and returns the minimum yielded value.

#### Errors

Raises [`IterStop`](./iter-stop.md) if the iterator is empty and no
`default:` is provided.

### `next :default? :else?`

Returns the next value from the iterator.

#### Parameters

| Name      | Type | Description                                 |
| --------- | ---- | ------------------------------------------- |
| `default` |      | value to return if the iterator is empty    |
| `else`    |      | function to invoke if the iterator is empty |

#### Errors

Raises [`IterStop`](./iter-stop.md) when exhausted and no fallback
is provided.

### `skip n`

Returns an iterator that discards the first `n` values, then yields the rest.

#### Parameters

| Name | Type | Description         |
| ---- | ---- | ------------------- |
| `n`  | int  | values to discard   |

#### Errors

| Exception                        | Condition           |
| -------------------------------- | ------------------- |
| [`TypeError`](./type-error.md)   | `n` is not an `Int` |
| [`ValueError`](./value-error.md) | `n` is negative     |

### `take n`

Returns an iterator that yields at most `n` values.

#### Parameters

| Name | Type | Description             |
| ---- | ---- | ----------------------- |
| `n`  | int  | maximum values to yield |

#### Errors

| Exception                        | Condition           |
| -------------------------------- | ------------------- |
| [`TypeError`](./type-error.md)   | `n` is not an `Int` |
| [`ValueError`](./value-error.md) | `n` is negative     |

### `zip ...values`

Returns an iterator that yields one tuple for each step across this iterator
and the additional iterables.
The zipped iterator stops as soon as any input is exhausted.
