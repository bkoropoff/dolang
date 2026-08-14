# `Null`

Acts as an empty iterator and a sink that discards every value.

The [`std.null`](./index.md#null) singleton is the only `Null` value. Use it to
provide an empty input or discard output:

```
run tool stdin: $null stdout: $null
```

## Operators

### Iteration

Ends immediately without yielding a value.

### Sink

Accepts and discards every value.
