# Prelude

The following items are available globally in every Do program without any
`import` statement.

The `dolang` executable layers its
[shell prelude](../shell/index.md#shell-prelude) on top of this core prelude.
Embedded runtimes may provide a different additional prelude.

## `std`

| Name                                        | Description                           |
| ------------------------------------------- | ------------------------------------- |
| [`Array`](../api/std/array.md)              | [`Array`](../api/std/array.md) type   |
| [`array`](../api/std/index.md#array-values) | Variadic array factory                |
| [`Bin`](../api/std/bin.md)                  | [`Bin`](../api/std/bin.md) type       |
| [`Bool`](../api/std/bool.md)                | [`Bool`](../api/std/bool.md) type     |
| [`bool`](../api/std/bool.md)                | Truthiness coercion                   |
| [`dbg`](../api/std/index.md#dbg-value)      | Debug representation                  |
| [`Dict`](../api/std/dict.md)                | [`Dict`](../api/std/dict.md) type     |
| [`dict`](../api/std/index.md#dict)          | Variadic dictionary factory           |
| [`Float`](../api/std/float.md)              | [`Float`](../api/std/float.md) type   |
| [`float`](../api/std/float.md)              | Numeric coercion and parsing          |
| [`Func`](../api/std/func.md)                | [`Func`](../api/std/func.md) type     |
| [`getter`](../api/std/index.md#getter-func) | Class field getter decorator          |
| [`Int`](../api/std/int.md)                  | [`Int`](../api/std/int.md) type       |
| [`int`](../api/std/int.md)                  | Integer coercion and parsing          |
| `Module`                                    | Module type                           |
| [`Range`](../api/std/range.md)              | [`Range`](../api/std/range.md) type   |
| [`Record`](../api/std/record.md)            | [`Record`](../api/std/record.md) type |
| [`record`](../api/std/index.md#record)      | Variadic record factory               |
| [`Set`](../api/std/set.md)                  | [`Set`](../api/std/set.md) type       |
| [`setter`](../api/std/index.md#setter-func) | Class field setter decorator          |
| [`Str`](../api/std/str.md)                  | [`Str`](../api/std/str.md) type       |
| [`str`](../api/std/str.md)                  | Textual representation                |
| [`Sym`](../api/std/sym.md)                  | [`Sym`](../api/std/sym.md) type       |
| [`sym`](../api/std/sym.md)                  | Symbol interning                      |
| [`Tuple`](../api/std/tuple.md)              | [`Tuple`](../api/std/tuple.md) type   |
| [`tuple`](../api/std/index.md#tuple-values) | Variadic tuple factory                |
| [`Type`](../api/std/type.md)                | [`Type`](../api/std/type.md) type     |
| [`type`](../api/std/type.md)                | Type query and test function          |

## `strand`

The module itself is imported.
