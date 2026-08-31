# `Status`

How a non-child process exited.

Returned by [`Proc.wait()`](./proc.md#wait).

## Fields

### `code`

The exit code as an [`Int`](../std/int.md). Only Windows supports obtaining
it for a non-child process; all other platforms report `nil`.

## Example

```
let p = proc.open $pid
p.terminate()
let status = p.wait()
if (status.code != nil)
  echo "exited with $(status.code)"
```
