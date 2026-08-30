# Status

`Status` is raised when an HTTP request completes but returns a status
outside `200..=299`.

```
try
  get "https://example.com/missing"
catch Status: err
  echo $err.status
```

The error stores response metadata and the first 64 KiB of the response body.

## Inherits

- [`Error`](./error.md)

## Fields

### `headers`

A dict-like view of the response headers. See
[`Response.headers`](./response.md#headers) for details.

#### Type

[`Dict`](../std/dict.md)-like

### `status`

The HTTP status code.

### `truncated`

`true` if the saved body excerpt was cut off either by the 64 KiB limit or by a
read error while buffering it.

### `url`

The response URL as [`url.Url`](../url/index.md), or `nil` if none
was attached.

## Methods

### `body`

Returns the saved body excerpt as [`Bin`](../std/bin.md).

### `json`

When the `json` feature is enabled, parses the saved body excerpt as JSON.

### `text`

Returns the saved body excerpt as [`Str`](../std/str.md), failing on
invalid UTF-8.
