# Record

Records are similar to [`Dict`](dict.md)s but support only symbol and integer
keys. They allow direct field access with dot syntax.

## Constructor

### `Record source`

Builds a record from one spreadable source of key-value pairs.

```
let r = Record {name: Alice, age: 30}
```

The lowercase `record` factory constructs a record verbatim from all
arguments, with positional arguments receiving incrementing integer keys and
key arguments becoming fields. Order and key multiplicity are preserved.

```
let r = record first second name: Alice
```

```
let r = record name: Alice age: 30
echo $r.name  # Alice
echo $r.age   # 30
```

## Field Access

Records support direct dot-syntax for symbol keys:

```
let r = record name: Alice age: 30
echo $r.name
r.age = 31
```

And indexing for both symbol and integer keys:

```
echo $r[:name:]
r[0] = "first"
```

## Ordering and Multi-Map

Like dicts, records preserve insertion order and support multi-map semantics
where applicable.

## Iteration

Records are iterable, yielding `[key, value]` pairs:

```
for k v = record name: Alice age: 30
  echo "$k: $v"
```

## Unpacking

Records support destructuring like dicts:

```
let :name :age = record name: Alice age: 30
```

## Type Methods

For programmatic access, the `Record` type object provides methods. These are
called on the type, not on instances, as the instance field namespace is
entirely reserved for the user.

### `Record.clear rec`

Clears all fields.

### `Record.contains rec key value?`

Tests whether the record contains the given key. If a value is provided,
tests whether any value associated with that key matches.

#### Parameters

| Name    | Type           | Description                             |
| ------- | -------------- | --------------------------------------- |
| `rec`   | `record`       | the record to check                     |
| `key`   | `Int` or `Sym` | the key to check                        |
| `value` |                | optional value to check for (multi-map) |

#### Returns

`Bool`

#### Example

```
let r = record 1 2 3 a: "first"
r.insert :a: "second"

# Key-only check
assert (record.contains $r 0)
assert (record.contains $r :a:)
assert (!record.contains $r 10)

# Key + value check
assert (record.contains $r :a: "first")
assert (record.contains $r :a: "second")
```

### `Record.count rec key?`

Returns a count derived from the record's multi-map structure.

With no `key`, it returns the number of distinct keys. With a `key`, it returns
the number of values associated with that key.

Missing keys return `0`.

### `Record.delete rec key`

Removes all values for a field.

Returns [`Bool`](./index.md) indicating whether any values were removed.

### `Record.get rec key :instance? :default? :else?`

Gets a field value with optional default. Supports `instance:` for multi-map
access. Negative `instance:` indexes count from the end.

### `Record.insert rec key value`

Sets a field. Key must be a symbol or integer.

### `Record.keys rec`

Returns an iterator of keys. Each distinct key is yielded exactly once, in the
order its first pair was inserted.

If duplicate-key iteration is needed, use ordinary record iteration.

### `Record.len rec`

Returns the number of fields.

#### Returns

`Int`

### `Record.pop rec key :instance? :default? :else?`

Removes and returns a value for a field. Supports `instance:` for multi-map
access to remove a specific value by its position among values for that field.
Negative `instance:` indexes count from the end.

### `Record.values rec key?`

Returns an iterator of values. With no `key`, it yields all stored values in
pair insertion order. With a `key`, it yields only the values associated with
that key, in that key's insertion order.

Missing keys return an empty iterator.
