# `class`

Decorator placing a class member in the type-object namespace, where subclasses
inherit it.

Applies to a `def` or a `field` inside a class body. A class method receives the
class it was reached through as its first parameter. Each class in a hierarchy
gets its own storage for a class field, seeded from the declared initializer.

```
class Counter
  #[class]
  pub field count = 0

  #[class]
  pub def bump cls
    cls.count = (cls.count + 1)
    cls.count

assert_eq $Counter.bump() 1
```

See [`static`](./static.md) for the uninherited counterpart, and
[Class and Static Members](../../language/classes.md#class-and-static-members)
for the full semantics.
