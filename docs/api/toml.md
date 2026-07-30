# toml

TOML serialization and deserialization.

## Functions

### `encode value`

Serializes a Do value to a TOML string.

#### Parameters

| Name    | Type | Description            |
| ------- | ---- | ---------------------- |
| `value` |      | the value to serialize |

#### Returns

`Str` -- TOML string

#### Errors

| Exception   | Condition                                                                                                                   |
| ----------- | --------------------------------------------------------------------------------------------------------------------------- |
| `TypeError` | A value is not TOML-representable, including `nil`, binary data, custom objects, or a table with non-string/non-symbol keys |

Type mapping:

| Do Type  | TOML Value |
| -------- | ---------- |
| `Bool`   | boolean    |
| `Int`    | integer    |
| `Float`  | float      |
| `Str`    | string     |
| `Sym`    | string     |
| `array`  | array      |
| `tuple`  | array      |
| `dict`   | table      |
| `record` | table      |

Top-level `dict` and `record` values serialize as TOML documents. Other values
serialize as TOML values.

```
assert_eq (encode 42) "42"
assert_eq (decode $ encode [1, 2, 3]) [1, 2, 3]

let doc = encode {"name": "alice", "enabled": true}
assert_eq (decode $doc) {"name": "alice", "enabled": true}
```

### `decode toml`

Parses a TOML string into a Do value.

#### Parameters

| Name   | Type                  | Description         |
| ------ | --------------------- | ------------------- |
| `toml` | [`Str`](./std/str.md) | TOML input to parse |

#### Returns

The parsed Do value.

#### Errors

| Exception    | Condition                                                                              |
| ------------ | -------------------------------------------------------------------------------------- |
| `ValueError` | The input is not valid TOML                                                            |
| `ValueError` | The input uses TOML datetime/date/time values, which are not mapped in this module yet |

Type mapping:

| TOML Value | Do Type |
| ---------- | ------- |
| boolean    | `Bool`  |
| integer    | `Int`   |
| float      | `Float` |
| string     | `Str`   |
| array      | `array` |
| table      | `dict`  |

`decode` accepts both full TOML documents and bare TOML values such as
numbers, arrays, and inline tables.

```
assert_eq (decode "42") 42
assert_eq (decode "[1, 2, 3]") [1, 2, 3]

let doc = decode |
  title = "Example"
  [server]
  port = 8080

assert_eq $doc["title"] "Example"
assert_eq $doc["server"]["port"] 8080
```
