# DateTime

A UTC instant represented as Unix nanoseconds.

## Type Methods

### `now()`

Returns the current UTC time.

```
echo $DateTime.now()
```

### `from_unix seconds? :nanos?`

Creates a `DateTime` from a Unix timestamp.

#### Parameters

| Name      | Type                                                | Description                                            |
| --------- | --------------------------------------------------- | ------------------------------------------------------ |
| `seconds` | [`Int`](../std/int.md)\|[`Float`](../std/float.md)? | Optional seconds since Unix epoch                      |
| `nanos`   | [`Int`](../std/int.md)?                             | Optional nanoseconds since Unix epoch or offset to add |

#### Example

```
echo $ DateTime.from_unix 1700000000
echo $ DateTime.from_unix 1.25
echo $ DateTime.from_unix nanos: 1700000000123000000
echo $ DateTime.from_unix 1700000000 nanos: 123000000
```

### `parse_rfc3339 text`

Parses an RFC3339 timestamp.

#### Parameters

| Name   | Type                   | Description         |
| ------ | ---------------------- | ------------------- |
| `text` | [`Str`](../std/str.md) | RFC3339 input text  |

#### Returns

[`DateTime`](./datetime.md)

#### Errors

| Exception    | Condition                                  |
| ------------ | ------------------------------------------ |
| `ValueError` | The input is not a valid RFC3339 timestamp |

#### Example

```
let dt = DateTime.parse_rfc3339("2024-01-02T03:04:05Z")
echo $dt.rfc3339()
```

## Fields

| Field        | Type                       | Description                   |
| ------------ | -------------------------- | ----------------------------- |
| `unix_secs`  | [`Float`](../std/float.md) | Approximate Unix seconds view |
| `unix_nanos` | [`Int`](../std/int.md)     | Exact Unix nanoseconds view   |

## Operators

- `DateTime - DateTime -> Duration`

## Methods

### `rfc3339()`

Returns the RFC3339 representation.

#### Returns

[`Str`](../std/str.md)

#### Example

```
let dt = DateTime.from_unix(1700000000)
echo $dt.rfc3339()
```

## String Form

`str(datetime)` renders the same UTC RFC3339 timestamp as `datetime.rfc3339()`.
