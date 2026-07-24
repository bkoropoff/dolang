# HTTP Mock Extension

Mock HTTP server for testing code that makes requests through the
[HTTP extension](../http/index.md).

## Types

| Type                      | Description                                            |
| ------------------------- | ------------------------------------------------------ |
| [`Server`](./server.md)   | A running mock HTTP server                             |
| [`Mock`](./mock.md)       | Handle to the mock(s) registered by one `.mock()` call |
| [`Request`](./request.md) | A captured request                                     |

```
import http
import http.mock

http.mock.Server do |server|
  server.mock
    - method: GET
      path: /users/42
      respond:
        status: 200
        json: {id: 42, name: "Alice"}
    do |handle|
      let resp = http.get (server.url / "/users/42")
      assert_eq $resp.json()["name"] "Alice"
```

Matchers and responses can also be built from the request itself via a
callback:

```
server.mock
  - path: /echo
    respond: do |req|
      let resp =
        status: 200
        body: $req.body
      resp
```
