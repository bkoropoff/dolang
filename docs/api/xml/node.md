# Node

Represents a parentless XML element.

Nodes can be shared or inserted into different trees. Serialization derives
namespace declarations from each node rather than from parent pointers.

## Constructor

### `Node tag ...items :namespace? :prefix? :attrs?`

Creates an element, optionally with attributes and children.

#### Parameters

| Name        | Type    | Description                                                                |
| ----------- | ------- | -------------------------------------------------------------------------- |
| `tag`       | `Str`   | Local element name                                                         |
| `items`     | `*`     | An [`Attr`](./attr.md) becomes an attribute; anything else becomes a child |
| `namespace` | `str?`  | Namespace URI                                                              |
| `prefix`    | `str?`  | Preferred namespace prefix                                                 |
| `attrs`     | `Dict?` | Unnamespaced attributes; keys may be `Sym` or `Str`, values must be `Str`  |

**Errors:**

- `TypeError` if `attrs` is not a `Dict`, or one of its values is not a `Str`.

Children are not type-checked here, matching `children.push`; a tree
containing anything other than `Node` and `Str` children is rejected by
[`verify`](./index.md#verify-node) and by serialization.

`attrs` is applied before any positional `Attr`, regardless of where the
keyword appears in the call.

```
let item = Node "item" namespace: "urn:inventory" prefix: "inv"

let disk = Node disk
  attrs:
    type: file
    device: disk
  - $ Node driver attrs: {name: "qemu", type: "qcow2"}
  - $ Node source
      attrs:
        file: /tmp/disk.qcow2
```

## Fields

### `tag`

Mutable local element name.

### `namespace`

Mutable namespace URI as a `Str`, or `nil` for no namespace.

### `prefix`

Mutable preferred prefix as a `Str`, or `nil`.

### `qname`

Read-only qualified name formed from `prefix` and `tag`.

### `attrs`

Mutable array-like view of [`Attr`](./attr.md) objects in document order.

The view supports indexed assignment and the [`Array`](../std/array.md)
mutation methods `push`, `insert`, `pop`, `delete`, and `clear`. Duplicate
expanded names can exist temporarily, but [`verify`](./index.md#verify-node)
and serialization reject them.

```
let node = Node "item"
node.attrs.push (Attr "id" "123")
node.attrs[0].value = "456"
```

### `children`

Mutable array-like view of child `Node` and `Str` values in document order.

The view supports indexed assignment and the [`Array`](../std/array.md)
mutation methods `push`, `insert`, `pop`, `delete`, and `clear`.

Iterating a node iterates this view directly.

### `namespaces`

Mutable `dict` containing the complete effective namespace snapshot for the
node. Keys are prefix strings; `""` is the default namespace. The reserved
`xml` binding is included.

Parsed descendants retain inherited bindings in their own snapshots, so they
can be detached and serialized independently.

## Methods

### `attr name :namespace? :default? :else?`

Gets the first attribute matching an expanded name.

#### Parameters

| Name        | Type   | Description                       |
| ----------- | ------ | --------------------------------- |
| `name`      | `Str`  | Local attribute name              |
| `namespace` | `str?` | Namespace URI                     |
| `default`   |        | Value returned when absent        |
| `else`      |        | Callable evaluated when absent    |

```
let id = node.attr "id" namespace: "urn:inventory"
```

### `set_attr name value :namespace? :prefix?`

Updates the first matching attribute or appends one.

An omitted `prefix` mutates the existing attribute's value in place. An
explicit prefix, including `prefix: nil`, replaces the matching attribute
because attribute identity fields are immutable.

#### Parameters

| Name        | Type   | Description                         |
| ----------- | ------ | ----------------------------------- |
| `name`      | `Str`  | Local attribute name                |
| `value`     | `Str`  | Attribute value                     |
| `namespace` | `str?` | Namespace URI                       |
| `prefix`    | `str?` | Preferred namespace prefix          |

### `delete_attr name :namespace?`

Deletes all attributes matching an expanded name.

#### Parameters

| Name        | Type   | Description             |
| ----------- | ------ | ----------------------- |
| `name`      | `Str`  | Local attribute name    |
| `namespace` | `str?` | Namespace URI           |

#### Returns

`Bool`, whether an attribute was deleted.

### `push child`

Appends a child.

#### Parameters

| Name    | Type          | Description |
| ------- | ------------- | ----------- |
| `child` | `Node`\|`Str` | Child value |

### `traverse`

Returns a depth-first, parent-first iterator over the node and its descendants.

#### Returns

An iterator of nodes and text values in document order.

## Operators

String indexing accesses unnamespaced attributes. Assignment updates the first
match or appends a new attribute. Integer indexing reads children, including
negative indexes. Use `attr`, `set_attr`, and `delete_attr` for namespaced
attributes.

```
let node = Node "item"
node["id"] = "123"
assert_eq $node["id"] "123"
node.push "content"
assert_eq $node[0] "content"
```
