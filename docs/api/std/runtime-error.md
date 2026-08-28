# RuntimeError

Supertype of ordinary catchable runtime failures, and a generic runtime error
in its own right when no more specific type applies.

## Inherits

- [`Error`](./error.md)

## Constructor

### `RuntimeError message`

Builds a runtime error carrying `message` verbatim — unlike the more specific
types, it adds no prefix of its own.

#### Parameters

| Name      | Type              | Description                      |
| --------- | ----------------- | -------------------------------- |
| `message` | [`Str`](./str.md) | the error's complete string form |

#### Example

```
throw RuntimeError "the widget came loose"
```

## Subclassing

A class may inherit from `RuntimeError` or any of its subtypes. The subclass
must chain to the supertype's constructor, which is what gives the instance
its string form:

```
class NoDomainError: RuntimeError
  pub field name = nil

  def (init) self name
    RuntimeError.(init) $self "domain does not exist: $name"
    self.name = name

try
  throw NoDomainError "guest0"
catch RuntimeError: e
  assert_eq (str e) "domain does not exist: guest0"
```

See [Error handling](../../language/error-handling.md#subclassing-error-types).
