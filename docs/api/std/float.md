# Float

64-bit floating point numbers.

## Constructor

`Float` accepts a float or an integer that can be represented exactly.

```
assert_eq (Float 42) 42.0
```

The lowercase `float` function performs numeric coercion and parsing:

```
assert_eq (float "3.14") 3.14
assert_eq (float true) 1.0
```

## Operators

### Arithmetic

| Operator | Description                  | Result  |
| -------- | ---------------------------- | ------- |
| `+`      | Addition                     | `Float` |
| `-`      | Subtraction                  | `Float` |
| `*`      | Multiplication               | `Float` |
| `/`      | Division                     | `Float` |
| `//`     | Euclidean (integer) division | `Int`   |
| `%`      | Euclidean remainder          | `Float` |
| `-x`     | Negation                     | `Float` |

`//` always returns an `Int`. `//` and `%` satisfy the identity
`x == (x // y) * y + (x % y)`.

### Comparison

`==`, `!=`, `<`, `>`, `<=`, `>=`

Mixed int/float comparisons are supported.
