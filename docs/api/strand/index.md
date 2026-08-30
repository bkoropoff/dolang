# strand

Concurrency primitives.

## Types

| Type                        | Description                         |
| --------------------------- | ----------------------------------- |
| [`Key`](./key.md)           | Strand-local storage key            |
| [`Resource`](./resource.md) | Scoped concurrency admission limit  |
| [`Strand`](./strand.md)     | Background strand handle            |
| [`Stream`](./stream.md)     | Strand handle with stream endpoints |

## Functions

### `channel buffer?`

Creates a new channel for communication between strands.

#### Parameters

| Name     | Type  | Description                                |
| -------- | ----- | ------------------------------------------ |
| `buffer` | `Int` | Buffer capacity (default: 1, unbuffered)   |

#### Returns

`(`[`Sender`](sender.md)`, `[`Receiver`](receiver.md)`)`

#### Example

```
let send recv = channel()  # unbuffered channel
let send recv = channel 10 # buffered with capacity 10
```

See [Sender](sender.md) and [Receiver](receiver.md) for the types returned.

### `collect target?`

A pipeline stage that collects all input values into an array (or another
target).

#### Parameters

| Name     | Type   | Description                                      |
| -------- | ------ | ------------------------------------------------ |
| `target` | output | collection to add to (defaults to a new `array`) |

#### Returns

The collection.

### `each func`

A pipeline stage that transforms values. Reads from its input, calls `func` on
each value, and writes the result to its output.

#### Parameters

| Name   | Type   | Description                               |
| ------ | ------ | ----------------------------------------- |
| `func` | `Func` | a function that transforms a single value |

### `fork ...blocks`

Executes multiple blocks concurrently and returns their results as an array.

#### Parameters

| Name        | Type   | Description                       |
| ----------- | ------ | --------------------------------- |
| `...blocks` | `Func` | functions to execute concurrently |

#### Returns

`array` -- results in the same order as the blocks

#### Example

```
let results = fork
  - do 42
  - do "hello"
  - do (1 + 2)

assert_eq $results [42, "hello", 3]
```

Use [`map`](#map-count-func-input-output) for bounded concurrent work over an
iterator and [`Resource`](./resource.md) for application-defined admission
limits.

### `from value`

A pipeline stage that emits all values from an iterable to its output.

#### Parameters

| Name    | Type  | Description                     |
| ------- | ----- | ------------------------------- |
| `value` | input | an iterable to emit values from |

### `map count func :input? :output?`

Applies `func` concurrently to values pulled lazily from an iterator.

#### Parameters

| Name     | Type                   | Description                                    |
| -------- | ---------------------- | ---------------------------------------------- |
| `count`  | [`Int`](../std/int.md) | number of worker strands                       |
| `func`   | `Func`                 | function applied to each input value           |
| `input`  | input?                 | source; defaults to the strand-local iterator  |
| `output` | output?                | destination; defaults to the strand-local sink |

Results are sent as workers complete. The function returns `nil` after the
input is exhausted and every worker has finished.

#### Example

```
let results = []
map 4 input: (Range 20) output: $results do |value|
  fetch $value
```

As a pipeline stage:

```
let results = pipeline
  do from (Range 20)
  do map 4 do |value| fetch $value
  do collect()
```

### `pipeline stage ...stages :input? :output?`

Creates a data processing pipeline by connecting multiple stages together. Each
stage runs concurrently in its own strand, with channels connecting the output
of one stage to the input of the next.

#### Parameters

| Name              | Type   | Description                                    |
| ----------------- | ------ | ---------------------------------------------- |
| `stage`, `stages` | `Func` | Pipeline stages to execute                     |
| `input`           | input  | Optional input source for the first stage      |
| `output`          | output | Optional output destination for the last stage |

Pipeline stages are functions that read from their implicit input and write to
their implicit output. The `from`, `where`, `each`, and `collect` functions are
designed to work as pipeline stages.

#### Example

```
let result = pipeline
  do from [1, 2, 3, 4, 5]
  do where do |x| (x > 2)
  do each do |x| (x * 2)
  do collect()

assert_eq $result [6, 8, 10]
```

With explicit input and output:

```
import fs:
  - open

# Process lines from a file, writing results to another file
open input.txt r do |in| open output.txt w do |out|
  pipeline input: $in output: $out
    do where do |line| line.contains "ERROR"
```

Lines read from a file keep their terminators and values written to one are
written verbatim, so the terminators carry straight through. To work with
unterminated lines in between, [`chomp`](../std/iter.md#chomp) the input and
[`precrimp`](../std/sink.md#precrimp-terminator) the output:

```
open input.txt r do |in| open output.txt w do |out|
  pipeline input: (in.chomp()) output: (out.precrimp())
    do where do |line| line.contains "ERROR"
    do each do |line| line.trim()
```

### `pool count input? func`

Executes `func` over an iterator with a fixed number of scoped worker strands.

#### Parameters

| Name    | Type                   | Description                                   |
| ------- | ---------------------- | --------------------------------------------- |
| `count` | [`Int`](../std/int.md) | number of worker strands                      |
| `input` | input?                 | source; defaults to the strand-local iterator |
| `func`  | `Func`                 | function applied to each input value          |

Block results are discarded. The function returns `nil` after the input is
exhausted and every worker has finished.

#### Example

```
pool 4 $urls do |url|
  download $url
```

As a pipeline stage, omit `input` to use the stage's input:

```
pipeline
  do from $urls
  do pool 4 do |url| download $url
```

### `put value`

Writes `value` to the strand-local output.

#### Parameters

| Name    | Type  | Description      |
| ------- | ----- | ---------------- |
| `value` | input | value to write   |

#### Returns

`nil`

#### Example

```
let values = pipeline
  do put 42
  do collect()

assert_eq $values [42]
```

### `spawn func`

Runs `func` concurrently in a background strand, returning a
[Strand](strand.md) handle for managing it.

#### Parameters

| Name   | Type   | Description             |
| ------ | ------ | ----------------------- |
| `func` | `Func` | the function to execute |

#### Returns

[Strand](strand.md) -- a handle to the background strand

#### Example

```
let worker = spawn do
  echo "Background task running"
  42

echo "Main task continuing"
let result = worker.join()
echo "Result: $result"
```

Use the returned `Strand`'s [`join`](strand.md#join) method to wait for
completion and get the result, or [`cancel`](strand.md#cancel) to request early
termination.

Background strands do not inherit active [`Resource`](./resource.md)
reservations or the strand-local values of `Key`s.

### `stream func`

Runs `func` in a background strand with its strand-local input `Iter` and
output `Sink` connected to channels. The returned [Stream](./stream.md) handle
can be used to communicate with it.

#### Parameters

| Name   | Type   | Description             |
| ------ | ------ | ----------------------- |
| `func` | `Func` | the function to execute |

#### Returns

[Stream](./stream.md)

#### Example

```
let s = stream do each do |x| (x * 2)
let input = s.sink()
let output = s.iter()

let results = fork
  do
    input.put 1
    input.put 2
    input.put 3
  do
    let r1 = output.next()
    let r2 = output.next()
    let r3 = output.next()
    [r1, r2, r3]

s.join()
assert_eq $results[1] [2, 4, 6]
```

### `where predicate`

A pipeline stage that filters values. Reads from its input, tests each value
with the predicate, and writes passing values to ts output.

#### Parameters

| Name        | Type   | Description                               |
| ----------- | ------ | ----------------------------------------- |
| `predicate` | `Func` | a function returning a truthy/falsy value |
