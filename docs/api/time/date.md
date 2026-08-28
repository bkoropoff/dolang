# Date

A calendar date without a time or time zone.

## Type Methods

### `today()`

Returns the current UTC date.

### `from_ymd year month day`

Creates a date from calendar components.

#### Parameters

| Name    | Type                                          | Description                           |
| ------- | --------------------------------------------- | ------------------------------------- |
| `year`  | [`Int`](../std/int.md)                        | Calendar year                         |
| `month` | [`Month`](./month.md)\|[`Int`](../std/int.md) | Month or its number from 1 through 12 |
| `day`   | [`Int`](../std/int.md)                        | Day of month                          |

### `parse_rfc text`

Parses an RFC full-date (`YYYY-MM-DD`).

## Fields

| Field     | Type                      | Description    |
| --------- | ------------------------- | -------------- |
| `year`    | [`Int`](../std/int.md)    | Calendar year  |
| `month`   | [`Month`](./month.md)     | Calendar month |
| `day`     | [`Int`](../std/int.md)    | Day of month   |
| `weekday` | [`Weekday`](./weekday.md) | Day of week    |

## Methods

### `rfc()`

Returns the RFC full-date representation.

### `datetime()`

Returns midnight UTC as a [`DateTime`](./datetime.md).

### `add_days days`

Returns the date offset by a signed number of days.

### `sub_days days`

Returns the date offset backward by a signed number of days.

## Operators

- `Date - Date -> Duration`
