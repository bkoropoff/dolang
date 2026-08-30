# Sink

Abstract type for sinks. Values that implement the sink protocol
are instances of `Sink`, which can be used for type testing.

`Sink` is not constructible directly.

```
assert_eq (type $ [].sink()) $Sink
```

## Inherits

- [`Sinkable`](./sinkable.md)

## Methods

### `prechomp`

Creates a wrapper `Sink` which removes a trailing line terminator (`\n` or
`\r\n`) from each item, if present, before it is written.

#### Errors

Putting an item raises [`TypeError`](./type-error.md) if it is neither
neither a `Str` nor a `Bin`.

#### Example

```
let acc = []
let out = acc.prechomp()
out.put "line\n"
assert_eq $acc ["line"]
```

### `precrimp terminator?`

Creates a wrapper `Sink` that appends a terminator to each value before it is
written. The append is unconditional, so an item that already ends in one gets
a second.

#### Parameters

| Name         | Type                                  | Description                              |
| ------------ | ------------------------------------- | ---------------------------------------- |
| `terminator` | [`Str`](./str.md)\|[`Bin`](./bin.md)? | Appended to each value (default: `"\n"`) |

#### Errors

Raises [`TypeError`](./type-error.md) for a value, or a terminator, that is
neither a `Str` nor a `Bin`, or if a `Bin` terminator would leave a `Str` value
invalid UTF-8.

#### Example

```
let out = shell.stdout.precrimp()
out.put "hello"
# writes "hello\n"

open $path w do |file|
  file.precrimp(shell.line_ending()).put "native"
```

### `prefilter pred`

Wraps the sink so only values for which `pred` is truthy are written.

Because each wrapper transforms values on the way in, the outermost wrapper
sees a value first — the reverse of an iterator chain.

#### Parameters

| Name   | Type | Description                            |
| ------ | ---- | -------------------------------------- |
| `pred` |      | decides whether a value is written     |

#### Example

```
let acc = []
let out = acc.prefilter (do |x| x % 2 == 0)
out.put 1
out.put 2
assert_eq $acc [2]
```

### `premap func`

Wraps the sink so each value is transformed before it is written.

The `pre` prefix marks the direction: unlike [`Iter.map`](./iter.md#map-func),
which transforms values on their way *out*, `premap` transforms values on their
way *into* the sink. A value that is both [`Iterable`](./iterable.md) and
[`Sinkable`](./sinkable.md) — an [`Array`](./array.md), say — therefore offers
`map` and `premap` as separate methods running in opposite directions.

#### Parameters

| Name   | Type | Description                            |
| ------ | ---- | -------------------------------------- |
| `func` |      | applied to each value before it is put |

#### Example

```
let acc = []
let out = acc.premap (do |x| x * 2)
out.put 3
assert_eq $acc [6]
```

### `put value`

Writes a value to the sink.

#### Parameters

| Name    | Type | Description        |
| ------- | ---- | ------------------ |
| `value` |      | the value to write |
