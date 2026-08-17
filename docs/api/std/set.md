# Set

Sets are ordered, mutable collections with unique membership semantics.

Iteration preserves insertion order. Reinserting an existing value is a no-op
and does not move it to the end.

## Constructor

### `Set iterable`

Builds a set from one iterable.

## Fields

### `len`

Returns the number of members.

#### Type

[`Int`](./index.md)

## Methods

### `add value`

Adds `value` if it is not already present.

#### Parameters

| Name    | Type | Description         |
| ------- | ---- | ------------------- |
| `value` |      | the value to insert |

#### Example

```
let s = Set [1, 2]
s.add 3
s.add 2
assert_eq [...s] [1, 2, 3]
```

### `delete value`

Removes `value` if present.

#### Parameters

| Name    | Type | Description         |
| ------- | ---- | ------------------- |
| `value` |      | the value to remove |

#### Returns

[`Bool`](./index.md) indicating whether a value was removed

### `clear`

Removes all members.

```
let s = Set [1, 2]
s.clear()
assert_eq $s.len 0
```

### `copy`

Returns a shallow copy of the Set.

Insertion order is preserved. Contained values are *not* recursively copied.

#### Returns

[`Set`](./set.md)

### `contains value`

Tests whether the Set contains `value`.

#### Parameters

| Name    | Type | Description       |
| ------- | ---- | ----------------- |
| `value` |      | the value to test |

#### Returns

[`Bool`](./index.md)

### `union other`

Returns a new Set containing all members from `self`, then first-seen members
from `other`.

#### Parameters

| Name    | Type              | Description |
| ------- | ----------------- | ----------- |
| `other` | [`Set`](./set.md) | other Set   |

#### Returns

[`Set`](./set.md)

### `intersect other`

Returns a new Set containing members that are present in both sets, in
the receiver's insertion order.

#### Parameters

| Name    | Type              | Description |
| ------- | ----------------- | ----------- |
| `other` | [`Set`](./set.md) | other Set   |

#### Returns

[`Set`](./set.md)

### `diff other`

Returns a new Set containing members from the receiver that are not present in
`other`.

#### Parameters

| Name    | Type              | Description |
| ------- | ----------------- | ----------- |
| `other` | [`Set`](./set.md) | other Set   |

#### Returns

[`Set`](./set.md)

### `sym_diff other`

Returns a new Set containing members present in exactly one of the two
sets.

#### Parameters

| Name    | Type              | Description |
| ------- | ----------------- | ----------- |
| `other` | [`Set`](./set.md) | other Set   |

#### Returns

[`Set`](./set.md)

### `is_subset other`

Returns `true` when every member of the receiver is present in `other`.

#### Parameters

| Name    | Type              | Description |
| ------- | ----------------- | ----------- |
| `other` | [`Set`](./set.md) | other Set   |

#### Returns

[`Bool`](./index.md)

### `is_superset other`

Returns `true` when every member of `other` is present in the receiver.

#### Parameters

| Name    | Type              | Description |
| ------- | ----------------- | ----------- |
| `other` | [`Set`](./set.md) | other Set   |

#### Returns

[`Bool`](./index.md)

## Operations

### Iteration

Iterating a Set yields values in insertion order:

```
let s = Set [3, 1, 2, 1]
assert_eq [...s] [3, 1, 2]
```
