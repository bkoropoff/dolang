# `Geometry`

The dimensions of a terminal-backed [`Console`](./console.md).

Returned by [`geometry()`](./console.md#geometry).

## Fields

### `rows`

Height in character cells, or `nil` if unknown.

**Returns:** [`Int`](../std/int.md)?

### `cols`

Width in character cells, or `nil` if unknown.

**Returns:** [`Int`](../std/int.md)?

`rows` and `cols` are independently `nil`: a console may know one dimension
without the other. The host console (`term.console`) always returns a
`Geometry`, never `nil` itself — see
[`Console.geometry()`](./console.md#geometry).

```
let g = term.console.geometry()
if g.cols
  echo $ term.preformat $ "-" * g.cols
```

## Subclassing

`Geometry` is the interface; reading `rows` or `cols` on a bare instance throws
[`UnsupportedError`](../std/unsupported-error.md). A Do console that implements
`geometry()` subclasses this to describe itself:

```
class FixedGeometry: term.Geometry
  pub field rows
  pub field cols

  def (init) self rows cols
    term.Geometry.(init) $self
    self.rows = rows
    self.cols = cols
```
