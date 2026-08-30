# Response

HTTP response object returned by HTTP requests.

An HTTP response object might hold open resources such as a connection pending
receipt of the response body. Methods that fetch the response body
automatically close or release the connection. After the response is closed,
most subsequent methods will return errors.

## Fields

### `headers`

A dict-like view over the response headers.

Header values are usually returned as strings. If a header value parses as an
HTTP-date, it is returned as a [`DateTime`](../time/datetime.md) instead.

#### Type

[`Dict`](../std/dict.md)-like

#### Example

```

let response = get https://api.example.com/users
echo $response.headers["content-type"]
echo $response.headers["content-length"]
```

```
import time:
  - DateTime

get https://api.example.com/archive do |response|
  let modified = response.headers["last-modified"]
  assert (type modified DateTime)
```

### `status`

The HTTP status code of the response.

#### Type

[`Int`](../std/int.md)

#### Example

```

let response = get https://api.example.com/users
echo $response.status  # 200 for success
```

### `url`

The final response URL after redirects.

#### Type

[`url.Url`](../url/url.md)

## Methods

### `body`

Reads the response body as binary data. This method consumes the response and
leaves it in a "closed" state.

#### Returns

[`Bin`](../std/bin.md) -- The response body as bytes

#### Example

```

let response = get https://api.example.com/image.png
let data = response.body()
echo "Downloaded $data.len bytes"
```

### `chunks`

Returns an iterator that yields the response body as raw bytes chunks. This
method is useful for processing large responses without loading the entire
body into memory.

#### Returns

An iterator of [`Bin`](../std/bin.md) values

#### Example

```

get https://api.example.com/large-file do |response|
  let total_size = 0
  for chunk = response.chunks()
    total_size = total_size + chunk.len
  echo "Downloaded $total_size bytes"
```

### `close`

Closes the response if it hasn't been already.

### `events`

Returns an iterator that parses the response body as a Server-Sent Events
stream. This is useful for streaming LLM responses, log tails, and other
event feeds delivered as `text/event-stream`.

This method consumes the response body incrementally. Once iteration begins,
the response should be treated as body-owned by the iterator, just like
[`chunks`](#chunks) and [`lines`](#lines).

Each yielded item is an [`Event`](./event.md) with `type`, `data`, `id`,
and `retry` fields.

#### Returns

An iterator of [`Event`](./event.md) values

#### Errors

| Exception             | Condition                               |
| --------------------- | --------------------------------------- |
| `RuntimeError`        | The response has already been closed    |
| [`Error`](./error.md) | The underlying body read fails          |
| `ValueError`          | The event stream contains invalid UTF-8 |

#### Example

```

get https://api.example.com/stream do |response|
  for event = response.events()
    echo "event=$event.type id=$event.id"
    echo $event.data
```

### `json`

Reads the response body and parses it as JSON. This method consumes the response
and leaves it in a "closed" state.

#### Returns

The parsed JSON value as a tree of
[`Int`](../std/int.md),
[`Float`](../std/float.md), [`Str`](../std/str.md),
[`Array`](../std/array.md), and [`Dict`](../std/dict.md), as
appropriate.

#### Errors

| Exception             | Condition                                  |
| --------------------- | ------------------------------------------ |
| `RuntimeError`        | The response has already been closed       |
| [`Error`](./error.md) | An error occurs while reading the response |
| `ValueError`          | The JSON is invalid                        |

#### Example

```

let response = get https://api.example.com/users
let data = response.json()
echo $data["users"][0]["name"]
```

### `lines`

Returns an iterator that yields the response body as lines (split on `\n` or
`\r\n`). Line endings are stripped from the returned values.

#### Returns

An iterator of [`Str`](../std/str.md) values

#### Example

```

get https://api.example.com/logs do |response|
  for line = response.lines()
    if (line.contains "ERROR")
      echo $line
```

### `text`

Reads the response body as text. This method consumes the response and leaves it
in a "closed" state.

#### Returns

[`Str`](../std/str.md) -- The response body as text

#### Errors

| Exception             | Condition                              |
| --------------------- | -------------------------------------- |
| `RuntimeError`        | The response has already been closed   |
| [`Error`](./error.md) | A transport or protocol failure occurs |

#### Example

```

let response = get https://api.example.com/users
echo $response.text()
```
