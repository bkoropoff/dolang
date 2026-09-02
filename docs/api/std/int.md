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
