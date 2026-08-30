# Request

A request captured by the mock server — returned by
[`Mock.received`](./mock.md#received)/[`Server.received_requests`](./server.md#received_requests),
and passed as the argument to a `match:`/`respond: do |req| ...` callback (see
[`Server.mock`](./server.md#mock-do)).

## Fields

### `body`

The raw request body.

#### Type

[`Bin`](../std/bin.md)

### `headers`

The request headers. Lazily projected from the underlying request — reading one
header doesn't materialize the rest. Header names can repeat; `.get()` accepts
an `instance` position to select among them (`-1`, the default, selects the
last), matching [`dict.get`](../std/dict.md#get-key-instance-default-else).

#### Type

[`Dict`](../std/dict.md)-like

#### Example

```
server.mock
  - match: do |req|
      req.headers["x-trace"] == "abc"
    respond:
      status: 200
```

### `method`

The request method, e.g. `"GET"`.

#### Type

[`Str`](../std/str.md)

### `url`

The full request URL.

#### Type

[`Str`](../std/str.md)
