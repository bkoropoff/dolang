# Sinkable

Abstract type for values that implement the `(sink)` protocol.
`Sinkable` values can be converted into sinks explicitly and used for type
testing.

`Sinkable` is not constructible directly.

```
assert (type [] Sinkable)
```

## Methods

### `sink`

Returns a sink over the value.

#### Returns

[`Sink`](./sink.md)

### Forwarded methods

`x.foo(...)` on a `Sinkable` means `x.sink().foo(...)`, so [`Sink`](./sink.md)'s
methods can be called on the value directly:

| Method                                         | Description                                   |
| ---------------------------------------------- | --------------------------------------------- |
| [`put value`](./sink.md)                       | Writes a value                                |
| [`premap func`](./sink.md)                     | Transforms each value before it is written    |
| [`prefilter pred`](./sink.md)                  | Writes only values for which `pred` is truthy |
| [`prechomp`](./sink.md#prechomp)               | Removes one trailing line terminator          |
| [`precrimp t?`](./sink.md#precrimp-terminator) | Appends a line terminator                     |

`premap`/`prefilter` are named for their direction — they act on values headed
*into* the sink. A value that is both `Sinkable` and
[`Iterable`](./iterable.md), such as an [`Array`](./array.md), offers those
alongside `map`/`filter`, which run the opposite way:

```
let acc = []
let out = acc.premap (do |x| x * 2)
out.put 3
assert_eq $acc [6]
assert_eq [...acc.map (do |x| x + 1)] [7]
```
