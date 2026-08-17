# IterStop

Error raised to signal that an iterator is exhausted. This is used
internally by the iteration protocol and can be caught in `try`/`catch`
statements.

`IterStop` can be constructed directly.

## Inherits

- [`Error`](./error.md)

## Example

```
let err = IterStop()
throw err
```
