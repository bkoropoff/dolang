# Mock

Handle returned by [`Server.mock`](./server.md#mock-do). Represents every
matcher/response item registered by a single `.mock()` call, whether that
was one item or several — its methods act on all of them together.

## Methods

### `received()`

Returns every request matched by any of this handle's mocks, in the order
received.

#### Returns

Array of [`Request`](./request.md).

#### Example

```
server.mock
  - method: POST
    path: /users
    respond:
      status: 201
  do |handle|
    http.post (server.url / "/users") body: hello
    let reqs = handle.received()
    assert_eq $reqs.len 1
    assert_eq $reqs[0].method "POST"
```

### `unmount()`

Unmounts every mock this handle registered. They stop matching further
requests. Has no effect on items already unmounted.

### `verify()`

Re-checks the `expect:` condition of every item registered by this handle
(if any were given) against requests received so far.

#### Errors

Raises if any item's `expect:` is unsatisfied, same as at scoped-block exit
— the message lists every unsatisfied item, one per line.
