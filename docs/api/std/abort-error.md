# AbortError

Raised when execution is aborted by the host trap mechanism.

`AbortError` is sealed: it carries host state no script can supply, it is
deliberately uncatchable, and it can be neither constructed nor subclassed.

## Inherits

- [`Error`](./error.md)
