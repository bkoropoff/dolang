# Array

Arrays are ordered, mutable sequences of values.

## Constructor

### `Array iterable`

Builds an array from one iterable. The lowercase `array` factory instead
collects its positional arguments verbatim.

## Fields

### `len`

Returns the number of elements.

#### Type

[`Int`](./index.md)

#### Example

```
assert_eq $[1, 2, 3].len 3
```

## Methods

### `clear`

Removes all elements from the array.

```
let arr = [1, 2, 3]
arr.clear()
assert_eq $arr.len 0
```

### `contains element`

Tests whether the array contains the given element (by equality).

#### Parameters

| Name      | Type | Description        |
| --------- | ---- | ------------------ |
| `element` |      | the value to check |

#### Returns

[`Bool`](./index.md)

#### Example

```
let arr = [1, 2, 3, "hello"]
assert (arr.contains 2)
assert (arr.contains "hello")
assert (!arr.contains 4)
assert (![].contains 1)
```

### `copy`

Returns a shallow copy of the array. Contents are *not* copied recursively.

#### Returns

[`Array`](./array.md)

When inherited by a Do subclass, `copy()` calls the subclass constructor with
the source array as a single positional argument.

### `delete index`

Deletes the element at `index` if it exists.
Negative indexes count from the end.

Out-of-bounds indexes are ignored.

#### Returns

[`Bool`](./index.md) indicating whether an element was removed

#### Example

```
let arr = [10, 20, 30]
assert (arr.delete 1)
assert (arr.delete -1)
assert (!(arr.delete 99))
assert_eq $arr [10]
```

### `get index :default? :else?`

Retrieves the value at the given index. Returns `nil` if out of bounds and no
alternative is provided. Negative indexes count from the end.

#### Parameters

| Name       | Type                | Description                         |
| ---------- | ------------------- | ----------------------------------- |
| `index`    | [`Int`](./index.md) | the index to access                 |
| `default:` |                     | value to return if out of bounds    |
| `else:`    |                     | function to invoke if out of bounds |

#### Returns

The value, or the default/else result.

#### Example

```
let arr = [10, 20, 30]
assert_eq (arr.get 0) 10
assert_eq (arr.get -1) 30
assert_eq (arr.get 5 default: "missing") "missing"
assert_eq (arr.get 5 else: do "computed") "computed"
```

### `insert index ...values`

Inserts one or more values at the specified index, shifting existing elements.
Negative indexes count from the end; `-1` inserts before the last element.

#### Parameters

| Name        | Type                | Description               |
| ----------- | ------------------- | ------------------------- |
| `index`     | [`Int`](./index.md) | the position to insert at |
| `...values` |                     | values to insert          |

#### Example

```
let arr = [1, 2, 3]
arr.insert 1 42
assert_eq $arr [1, 42, 2, 3]
arr.insert -1 99
assert_eq $arr [1, 42, 2, 99, 3]
```

### `pairs`

Returns an iterator yielding `[index, value]` pairs.

#### Returns

iterator of `[int, value]` pairs

#### Example

```
for i v = [10, 20, 30].pairs()
  echo "$i: $v"
# 0: 10
# 1: 20
# 2: 30
```

### `pop index? :default? :else?`

Removes and returns the last element, or the element at `index` if provided.
Raises an error if the selected element does not exist and no alternative is
provided. Negative indexes count from the end.

#### Parameters

| Name       | Type                | Description                                            |
| ---------- | ------------------- | ------------------------------------------------------ |
| `index`    | [`Int`](./index.md) | optional index to remove; defaults to the last element |
| `default:` |                     | value to return if the element does not exist          |
| `else:`    |                     | function to invoke if the element does not exist       |

#### Returns

The removed value, or the default/else result.

#### Example

```
let arr = [1, 2, 3]
assert_eq $arr.pop() 3
assert_eq $arr [1, 2]
assert_eq $arr.pop(0) 1

let empty = []
assert_eq (empty.pop default: "none") "none"
```

### `push ...values`

Appends one or more values to the end of the array.

#### Parameters

| Name        | Type | Description      |
| ----------- | ---- | ---------------- |
| `...values` |      | values to append |

#### Example

```
let arr = [1, 2]
arr.push 3
assert_eq $arr [1, 2, 3]
```

### `sort :key? :reverse?`

Sorts the array in place.

#### Parameters

| Name       | Type                  | Description                           |
| ---------- | --------------------- | ------------------------------------- |
| `key:`     | `Func`?               | computes a sort key for each element  |
| `reverse:` | [`Bool`](./index.md)? | sorts in descending order when `true` |

The `key:` function is evaluated once per element.

#### Example

```
let arr = ["bbb", "a", "cc"]
arr.sort key: (do |x| x.len)
assert_eq $arr ["a", "cc", "bbb"]

arr.sort reverse: true
assert_eq $arr ["cc", "bbb", "a"]
```

## Operations

### Indexing

```
let arr = [10, 20, 30]
assert_eq $arr[0] 10
assert_eq $arr[-1] 30
arr[0] = 99
arr[-1] = 77
assert_eq $arr[0] 99
assert_eq $arr[-1] 77
```

Out-of-bounds access raises an error; use `get` if you wish to avoid this.

Arrays also accept [`Range`](./range.md) values for slicing:

```
let arr = [0, 1, 2, 3]
assert_eq $arr[1..3] [1, 2]
assert_eq $arr[..2] [0, 1]
assert_eq $arr[2..] [2, 3]
assert_eq $arr[..] [0, 1, 2, 3]
assert_eq $arr[Range 0 4 2] [0, 2]
assert_eq $arr[Range nil nil -1] [3, 2, 1, 0]
```

Slice indexing returns a new array.
Omitted `start` means `0`, omitted `end` means the array length, and negative
`start` and `end` values count from the end. Negative steps reverse the slice.

Contiguous slices also support assignment:

```
let arr = [0, 1, 2, 3]
arr[1..3] = [9, 9]
assert_eq $arr [0, 9, 9, 3]

arr[1..1] = (tuple 4 5)
assert_eq $arr [0, 4, 5, 9, 9, 3]
```

The replacement must be an [`Array`](./array.md) or a
[`Tuple`](./tuple.md) — a sequence whose length is already settled. Anything
else, including a [`Range`](./range.md), an iterator, or a lone value, is a
type error rather than being drained or wrapped. Stepped slices are read-only;
assignment with a non-unity step is rejected.

### Iteration

```
for value = [1, 2, 3]
  echo $value
```

### Unpacking

```
let a b ...rest = [1, 2, 3, 4]
assert_eq $a 1
assert_eq $b 2
```
