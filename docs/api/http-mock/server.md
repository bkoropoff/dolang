# Server

A running mock HTTP server, bound to a random local port.

## Constructor

### `Server ... func?`

#### Parameters

| Name   | Type | Description                                            |
| ------ | ---- | ------------------------------------------------------ |
| `func` | func | Callable to run with the server; auto-closes when done |

#### Returns

`Server` when no `func` is provided, otherwise the result of calling `func`

```
let server = http.mock.Server()
```

```
http.mock.Server do |server|
  echo $(server.url)
```

## Fields

### `address`

The server's bound socket address, e.g. `127.0.0.1:41823`.

#### Type

[`str`](../std/str.md)

### `url`

The server's base URL as a [`url.Url`](../url/url.md). Coerces to a string
when needed; use `/` to build request paths.

#### Type

[`url.Url`](../url/url.md)

```
http.mock.Server do |server|
  http.get (server.url / "/users/42")
```

## Methods

### `mock ... do?`

Registers one or more mocks on the server. Each item is a dict describing a
request matcher and the response to return when it matches.

#### Parameters

Each item accepts:

| Name         | Type                           | Description                                                              |
| ------------ | ------------------------------ | ------------------------------------------------------------------------ |
| `method`     | [`str`](../std/str.md)         | HTTP method to match, case-insensitive                                   |
| `path`       | [`str`](../std/str.md)         | Exact request path to match                                              |
| `path_regex` | [`str`](../std/str.md)         | Regex the request path must match                                        |
| `headers`    | [`dict`](../std/dict.md)       | Header name/value pairs that must all be present and match exactly       |
| `query`      | [`dict`](../std/dict.md)       | Query parameter name/value pairs that must all match exactly             |
| `body_json`  | any                            | JSON value the request body must deserialize to and equal                |
| `match`      | func                           | Callback matcher — see below                                             |
| `respond`    | [`dict`](../std/dict.md)\|func | Response to return, or a callback that builds one — see below. Required. |
| `expect`     | `int`\|range                   | Number (or range) of matching requests expected                          |
| `name`       | [`str`](../std/str.md)         | Name used in `expect:` failure messages                                  |

If none of `method`/`path`/`path_regex`/`headers`/`query`/`body_json`/`match`
are given, the item matches any request. All given matchers on one item must
match for that item to match (logical AND); multiple items each register a
separate mock (first-registered wins on overlap), useful for a specific
matcher plus a fallback with a different response on the same path.

`match`, if given, is a `do |req| ...` callback returning a truthy value
when the request matches. `req` is a [`Request`](./request.md). It's ANDed
with any other matchers on the same item.

`respond` accepts either a dict, described below, or a `do |req| ...`
callback that returns one — built dynamically from the matched request
(`req`, again a [`Request`](./request.md)):

| Name      | Type                                           | Description                         |
| --------- | ---------------------------------------------- | ----------------------------------- |
| `status`  | `int`                                          | HTTP status code, defaults to `200` |
| `headers` | [`dict`](../std/dict.md)                       | Response headers                    |
| `body`    | [`str`](../std/str.md)\|[`bin`](../std/bin.md) | Raw response body                   |
| `json`    | any                                            | Response body, JSON-serialized      |

```
server.mock
  - method: POST
    path: /echo
    respond: do |req|
      let resp =
        status: 200
        body: $req.body
      resp
```

A trailing `do` block scopes the mocks it registers to the block's duration —
they're unmounted automatically when the block exits, whether it returns
normally or raises. Without a block, the mocks persist for the life of the
`Server` (or until [`Mock.unmount`](./mock.md#unmount) is called).

#### Returns

A [`Mock`](./mock.md) handle covering every item registered by this call —
passed to the `do` block if one is given, otherwise returned directly.

#### Errors

- Raises if no items are given, if an item is missing `respond`, or if
  `path_regex` isn't a valid regular expression.
- If an item's `expect:` is unsatisfied by the time it's unmounted (block
  exit, or explicit `.unmount()`), raises with that item's `name:` (or a
  generic label) and the expected vs. actual request count.

```
http.mock.Server do |server|
  server.mock
    - method: GET
      path: /users/42
      respond:
        status: 200
        json: {id: 42}
    do |handle|
      http.get (server.url / "/users/42") do |res|
        assert_eq $res.status 200
```

Registering more than one item in a single call is useful for a specific
matcher plus a fallback on the same path — the more specific item, mounted
first, takes precedence:

```
server.mock
  - method: GET
    path: /users/42
    headers:
      authorization: "Bearer valid"
    respond:
      status: 200
  - method: GET
    path: /users/42
    respond:
      status: 401
```

Without a trailing block, `.mock()` returns a handle the script manages
explicitly:

```
let handle = server.mock
  - method: GET
    path: /users/42
    respond:
      status: 200

http.get (server.url / "/users/42")
handle.unmount()
```

### `reset`

Removes all mocks registered on the server and forgets all received
requests.

#### Returns

`nil`

### `received_requests`

Returns every request the server has received, regardless of which (if any)
mock matched it. See [`Mock.received`](./mock.md#received) to scope this to
a specific `.mock()` call instead.

#### Returns

Array of request dicts — see [`Mock.received`](./mock.md#received) for the
shape.

### `close`

Shuts down the server. Further requests to its address will fail.

#### Returns

`nil`
