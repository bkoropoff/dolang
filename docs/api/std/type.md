# Type

`Type` is the type of all types and cannot be constructed directly. The
lowercase `type` function queries and tests types.

## `type value`

Returns the type object representing the value's type.

### Parameters

| Name    | Type | Description        |
| ------- | ---- | ------------------ |
| `value` |      | the value to query |

#### Returns

type object

#### Example

```
assert_eq (type 42) $Int
assert_eq (type "hello") $Str
assert_eq (type [1, 2]) $Array
assert_eq (type nil) $Nil
```

## `type value type`

Tests whether a value is an instance of the given type, including through
inheritance.

### Parameters

| Name    | Type | Description        |
| ------- | ---- | ------------------ |
| `value` |      | the value to test  |
| `type`  |      | the type to check  |

#### Returns

`Bool`

#### Example

```
assert (type 42 Int)
assert (type "hello" Str)
assert (type nil Nil)

class Animal
class Dog: Animal

let d = Dog()
assert (type d Dog)
assert (type d Animal)
```
