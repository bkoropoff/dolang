# Resource

Limits concurrent entry into application-defined scopes.

## Constructor

### `Resource count`

Creates a resource with `count` concurrent reservations.

#### Parameters

| Name    | Type                   | Description                   |
| ------- | ---------------------- | ----------------------------- |
| `count` | [`Int`](../std/int.md) | maximum concurrent admissions |

#### Errors

- Raises `ValueError` if `count` is zero.

#### Example

```
let network = Resource 8
```

## Methods

### `with block`

Runs `block` while holding one reservation.

Entering the same resource again in the same strand is reentrant. Scoped child
strands inherit the reservation and share it with their parent. Background
strands created with `spawn` or `stream` do not inherit reservations.

Acquiring multiple resources in inconsistent orders can deadlock. Programs
must use a consistent acquisition order.

Resources limit admission only and must not be used to protect critical-section
invariants.

#### Parameters

| Name    | Type   | Description            |
| ------- | ------ | ---------------------- |
| `block` | `Func` | scoped work to execute |

#### Returns

the block result

#### Example

```
network.with do
  fetch_data()
```
