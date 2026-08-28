# IterStop

Error raised to signal that an iterator is exhausted. This is used
internally by the iteration protocol and can be caught in `try`/`catch`
statements.

## Inherits

- [`Error`](./error.md)

## Constructor

### `IterStop`

Builds the signal that an iterator is exhausted.

#### Example

```
throw IterStop()
```
