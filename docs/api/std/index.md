# std

The `std` module provides core language facilities.

## Types

| Name                                              | Description                                |
| ------------------------------------------------- | ------------------------------------------ |
| [`AbortError`](./abort-error.md)                  | Uncatchable host abort                     |
| [`Args`](./args.md)                               | Argument pack                              |
| [`Array`](./array.md)                             | Mutable ordered sequence                   |
| [`Bin`](./bin.md)                                 | Immutable binary data                      |
| [`BinBuf`](./binbuf.md)                           | Mutable byte buffer                        |
| [`Bool`](./bool.md)                               | Boolean (`true` / `false`)                 |
| [`BytecodeError`](./bytecode-error.md)            | Bytecode verification error                |
| [`CanceledError`](./canceled-error.md)            | Strand cancellation                        |
| [`CompileError`](./compile-error.md)              | Compilation error                          |
| [`ConcurrencyError`](./concurrency-error.md)      | Concurrent access violation                |
| [`CyclicImportError`](./cyclic-import-error.md)   | Cyclic module dependency                   |
| [`Dict`](./dict.md)                               | Mutable ordered dictionary                 |
| [`Error`](./error.md)                             | Abstract base error type                   |
| [`FieldError`](./field-error.md)                  | Nonexistent field access                   |
| [`Float`](./float.md)                             | 64-bit floating point                      |
| [`Fmt`](./fmt.md)                                 | Sequence of text and interpolations        |
| [`FmtValue`](./fmt-value.md)                      | Value bound to formatting options          |
| [`FmtSpec`](./fmt-spec.md)                        | Reusable formatting options                |
| [`Func`](./func.md)                               | Function value                             |
| [`Getter`](./getter.md)                           | Abstract getter protocol type              |
| [`ImmutableError`](./immutable-error.md)          | Mutation of an immutable value             |
| [`ImportError`](./import-error.md)                | Module import failure                      |
| [`IndexError`](./index-error.md)                  | Out-of-bounds index access                 |
| [`Int`](./int.md)                                 | 128-bit signed integer                     |
| [`Iter`](./iter.md)                               | Abstract iterator type                     |
| [`Iterable`](./iterable.md)                       | Abstract iterable type                     |
| [`IterStop`](./iter-stop.md)                      | Error raised when an iterator is exhausted |
| [`MissingKeyError`](./missing-key-error.md)       | Required key argument not provided         |
| [`MissingPosError`](./missing-pos-error.md)       | Required positional argument not provided  |
| [`Nil`](./nil.md)                                 | Type object for `nil`                      |
| [`Null`](./null.md)                               | Empty iterator and discarding sink         |
| [`OverflowError`](./overflow-error.md)            | Integer overflow                           |
| [`Range`](./range.md)                             | Numeric range for iteration                |
| [`Record`](./record.md)                           | Record with dot-syntax access              |
| [`RuntimeError`](./runtime-error.md)              | Ordinary runtime failure supertype         |
| [`Set`](./set.md)                                 | Mutable ordered set                        |
| [`Setter`](./setter.md)                           | Abstract setter protocol type              |
| [`Sink`](./sink.md)                               | Abstract sink type                         |
| [`Sinkable`](./sinkable.md)                       | Abstract sinkable type                     |
| [`SinkStop`](./sink-stop.md)                      | Error raised when a sink is closed         |
| [`StateError`](./state-error.md)                  | Invalid operation for current state        |
| [`Str`](./str.md)                                 | Immutable UTF-8 string                     |
| [`StrBuf`](./strbuf.md)                           | Mutable UTF-8 string buffer                |
| [`Sym`](./sym.md)                                 | Interned symbol                            |
| [`TimedOutError`](./timed-out-error.md)           | Strand timeout                             |
| [`Tuple`](./tuple.md)                             | Immutable ordered sequence                 |
| [`Type`](./type.md)                               | Type of types                              |
| [`TypeError`](./type-error.md)                    | Wrong type for an operation                |
| [`UnexpectedKeyError`](./unexpected-key-error.md) | Unexpected key argument                    |
| [`UnexpectedPosError`](./unexpected-pos-error.md) | Unexpected positional argument             |
| [`UnsupportedError`](./unsupported-error.md)      | Unsupported operation                      |
| [`Value`](./value.md)                             | Abstract supertype of all values           |
| [`ValueError`](./value-error.md)                  | Invalid value for an operation             |
| [`ZeroDivError`](./zero-div-error.md)             | Integer division or modulo by zero         |

## Values

### `null`

The singleton [`Null`](./null.md) value.

- As an iterator, `null` immediately ends.
- As a sink, `null` silently discards all values.

## Functions

### `array ...values`

Creates an array from positional arguments.

### `bool value`

Converts a value to [`Bool`](./bool.md) according to its truthiness.

### `dbg value`

Converts a value to its debug representation. Shows internal structure (e.g.
quotes strings, shows type tags).

#### Parameters

| Name    | Type | Description          |
| ------- | ---- | -------------------- |
| `value` |      | the value to convert |

#### Returns

[`Str`](./str.md)

### `dict ...`

Creates a dictionary from positional and key arguments.

Positional arguments receive incrementing integer keys starting at `0`.
Key arguments become symbol keys. The function-call syntax cannot specify
other key types; use a [horizontal dictionary
literal](../../language/data-structures.md#literals) or [vertical
data](../../language/vertical-layout.md#vertical-data) instead.

### `float value`

Coerces or parses a value as a [`Float`](./float.md).

### `getter func`

Builds a getter object from a function.

#### Parameters

| Name   | Type   | Description                   |
| ------ | ------ | ----------------------------- |
| `func` | `Func` | function used for field reads |

#### Returns

[`Getter`](./getter.md)

#### Example

```
class Config
  field port = 8080

  #[getter]
  pub def port obj
    obj.#port
```

### `hash ...values`

Returns a hash code computed over all supplied values in sequence. Passing
multiple values is useful for combining fields in a `(hash)` implementation:

```
def (hash) self
  hash $self.x $self.y $self.z
```

#### Parameters

| Name        | Type | Description                |
| ----------- | ---- | -------------------------- |
| `...values` |      | one or more values to hash |

#### Returns

`Int`

### `int value`

Coerces or parses a value as an [`Int`](./int.md).

### `record ...`

Creates a record from positional and key arguments.

Positional arguments receive incrementing integer keys starting at `0`.
Key arguments become symbol fields.

#### Parameters

| Name                 | Type | Description                       |
| -------------------- | ---- | --------------------------------- |
| positional arguments | *    | Receive incrementing integer keys |
| key arguments        |      | Become symbol fields              |

#### Returns

[`Record`](./record.md)

#### Example

```
let r = record name: Alice age: 30
echo $r.name  # Alice
```

### `setter func`

Builds a setter object from a function.

#### Parameters

| Name   | Type   | Description                    |
| ------ | ------ | ------------------------------ |
| `func` | `Func` | function used for field writes |

#### Returns

[`Setter`](./setter.md)

#### Example

```
class Config
  #[setter]
  pub def port obj value
    obj.#_port = value
```

### `str value`

Returns the general-purpose [`Str`](./str.md) representation of a value.

### `sym value`

Interns a string as a [`Sym`](./sym.md).

### `tuple ...values`

Creates a tuple from positional arguments.

### `type value type?`

Returns the value's type, or tests whether the value is an instance of
`type`. See [`Type`](./type.md).

### `verbatim value`

Converts a value to its verbatim representation. Preserves the literal textual
form of values where possible, which is useful for passing values as
command-line arguments to external programs.

#### Parameters

| Name    | Type | Description          |
| ------- | ---- | -------------------- |
| `value` |      | the value to convert |

#### Returns

[`Str`](./str.md)
