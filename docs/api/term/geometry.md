# `Geometry`

The dimensions of a terminal-backed [`Console`](./console.md).

Returned by [`geometry()`](./console.md#geometry), which answers `nil` for a
console that is just a stream. So the presence of a `Geometry` — not the
identity of the console — is the test for whether there is a layout to fit.

## Fields

### `rows`

Height in character cells.

**Returns:** [`Int`](../std/int.md)

### `cols`

Width in character cells.

**Returns:** [`Int`](../std/int.md)

```
let g = term.console.geometry()
if g
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
