# Iterable

Abstract type for iterable values. Values that implement the `(iter)` protocol
are instances of `Iterable`, which can be used for type testing.

`Iterable` is not constructible directly.

```
assert (type [1, 2, 3] Iterable)
```

## Methods

### `iter`

Returns an iterator over the value.

#### Returns

[`Iter`](./iter.md)

### Forwarded methods

`x.foo(...)` on an `Iterable` means `x.iter().foo(...)`, so most of
[`Iter`](./iter.md)'s methods can be called on a container directly:

```
assert_eq [...[1, 2, 3].map (do |x| x * 2)] [2, 4, 6]
assert_eq ([1, 2, 3].fold 0 (do |acc x| acc + x)) 6
```

| Method                         | Description                                 |
| ------------------------------ | ------------------------------------------- |
| [`all pred?`](./iter.md)       | Whether every item is truthy                |
| [`any pred?`](./iter.md)       | Whether any item is truthy                  |
| [`fold init func`](./iter.md)  | Accumulates a value over the items          |
| [`map func`](./iter.md)        | Transforms each item                        |
| [`filter pred`](./iter.md)     | Keeps items for which `pred` is truthy      |
| [`chain ...values`](./iter.md) | Concatenates with further iterables         |
| [`zip ...values`](./iter.md)   | Pairs items with those of further iterables |
| [`take n`](./iter.md)          | Yields at most the first `n` items          |
| [`skip n`](./iter.md)          | Discards the first `n` items                |
| [`enumerate`](./iter.md)       | Pairs each item with its index              |
| [`find pred`](./iter.md)       | First item for which `pred` is truthy       |
| [`min :default?`](./iter.md)   | Smallest item                               |
| [`max :default?`](./iter.md)   | Largest item                                |

Three of `Iter`'s methods are deliberately not forwarded, and raise
[`FieldError`](./field-error.md) on a container:

- `next` is stateful. A container has no iteration position of its own, so
  each call would mint a fresh iterator and a loop over `next` would keep
  returning the first item.
- `count` would be an O(n) way to ask for what most containers answer in
  constant time with `len`. [`Dict`](./dict.md) also defines `count` with an
  unrelated meaning.
- `kv` describes the pair shape of an iterator; how a value spreads is the
  iterable's own business.

Call `iter` explicitly to reach any of them:

```
assert_eq ([1, 2, 3].iter().count()) 3
```
