# Int

128-bit signed integers.

## Constructor

`Int` accepts an integer, a boolean, or an integral float. It rejects
fractional floats and interpretive conversions such as parsing strings.

```
assert_eq (Int true) 1
assert_eq (Int 3.0) 3
```

The lowercase `int` function performs coercion and parsing:

```
assert_eq (int "42") 42
assert_eq (int 3.14) 3
```

## Methods

### `binary()`

Formats the integer in base 2.

#### Returns

[`Str`](./str.md)

```
assert_eq ((10).binary()) "1010"
assert_eq ((-10).binary()) "-1010"
```

### `octal()`

Formats the integer in base 8.

#### Returns

[`Str`](./str.md)

```
assert_eq ((10).octal()) "12"
assert_eq ((-10).octal()) "-12"
```

### `hex()`

Formats the integer in lowercase base 16.

#### Returns

[`Str`](./str.md)

```
assert_eq ((255).hex()) "ff"
assert_eq ((-255).hex()) "-ff"
```

## Operators

### Arithmetic

| Operator | Description                  | Result  |
| -------- | ---------------------------- | ------- |
| `+`      | Addition                     | `Int`   |
| `-`      | Subtraction                  | `Int`   |
| `*`      | Multiplication               | `Int`   |
| `/`      | Division                     | `Float` |
| `//`     | Euclidean (integer) division | `Int`   |
| `%`      | Euclidean remainder          | `Int`   |
| `-x`     | Negation                     | `Int`   |

`/` always produces a `Float`. `//` and `%` satisfy the identity
`x == (x // y) * y + (x % y)`.

### Bitwise

| Operator | Description |
| -------- | ----------- |
| `&`      | AND         |
| `\|`     | OR          |
| `^`      | XOR         |
| `~x`     | NOT         |
| `<<`     | Left shift  |
| `>>`     | Right shift |

### Comparison

`==`, `!=`, `<`, `>`, `<=`, `>=`

Mixed int/float comparisons are supported.
