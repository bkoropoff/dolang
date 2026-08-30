# `static`

Decorator placing a class member in the type-object namespace without making it
inheritable.

Applies to a `def` or a `field` inside a class body. Because a static is not
inherited, it shadows an inherited class member of the same name without
propagating that shadowing to subclasses — which is what makes a static
`(call)` usable as a factory that cannot recurse into itself:

```
class Shape
  pub field kind = ""

  def (init) self kind
    self.kind = kind

  #[static]
  pub def (call) cls kind
    Type.(call) $cls $kind

class Circle: Shape

let c = Circle "arc"   # default instantiation: the factory is not inherited
```

See [`class`](./class.md) for the inherited counterpart, and
[Class and Static Members](../../language/classes.md#class-and-static-members)
for the full semantics.
