# Error

The abstract base type for all errors. All error types in `std` inherit
from `Error`, so it can be used as a catch-all in typed `catch` handlers.

`Error` is abstract: it has no representation of its own, so it can be neither
constructed nor inherited from directly. Subclass one of its concrete subtypes
instead — usually [`RuntimeError`](./runtime-error.md).
