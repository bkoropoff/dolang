# Event

Server-Sent Event item yielded by [`Response.events()`](./response.md#events).

## Fields

### `data`

The event payload text. Multiple `data:` lines are joined with `\n`.

#### Type

[`Str`](../std/str.md)

### `id`

The event identifier, if present.

#### Type

[`Str`](../std/str.md) or `nil`

### `retry`

The reconnection delay hint from the stream, if present.

#### Type

[`Int`](../std/int.md) or `nil`

#### Example

```

get https://api.example.com/stream do |response|
  for event = response.events()
    echo "[$event.type] $event.data"
```

### `type`

The event type. When the stream omits `event:`, or provides an empty `event:`
field, this defaults to `"message"`.

#### Type

[`Str`](../std/str.md)
