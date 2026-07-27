# Node

Represents a parentless XML element.

Nodes can be shared or inserted into different trees. Serialization derives
namespace declarations from each node rather than from parent pointers.

## Constructor

### `Node tag :namespace? :prefix?`

Creates an empty element.

**Parameters:**

| Name        | Type   | Description                         |
| ----------- | ------ | ----------------------------------- |
| `tag`       | `str`  | Local element name                  |
| `namespace` | `str?` | Namespace URI                       |
| `prefix`    | `str?` | Preferred namespace prefix          |

**Returns:** `Node`.

```
let item = Node "item" namespace: "urn:inventory" prefix: "inv"
```

## Fields

### `tag`

Mutable local element name.

### `namespace`

Mutable namespace URI as a `str`, or `nil` for no namespace.

### `prefix`

Mutable preferred prefix as a `str`, or `nil`.

### `qname`

Read-only qualified name formed from `prefix` and `tag`.

### `attrs`

Mutable array-like view of [`Attr`](./attr.md) objects in document order.

The view supports indexed assignment and the [`array`](../std/array.md)
mutation methods `push`, `insert`, `pop`, `delete`, and `clear`. Duplicate
expanded names can exist temporarily, but [`verify`](./index.md#verify-node)
and serialization reject them.

```
let node = Node "item"
node.attrs.push (Attr "id" "123")
node.attrs[0].value = "456"
```

### `children`

Mutable array-like view of child `Node` and `str` values in document order.

The view supports indexed assignment and the [`array`](../std/array.md)
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

**Parameters:**

| Name        | Type   | Description                       |
| ----------- | ------ | --------------------------------- |
| `name`      | `str`  | Local attribute name              |
| `namespace` | `str?` | Namespace URI                     |
| `default`   |        | Value returned when absent        |
| `else`      |        | Callable evaluated when absent    |

**Returns:** The attribute value, `nil`, or the selected fallback.

```
let id = node.attr "id" namespace: "urn:inventory"
```

### `set_attr name value :namespace? :prefix?`

Updates the first matching attribute or appends one.

An omitted `prefix` mutates the existing attribute's value in place. An
explicit prefix, including `prefix: nil`, replaces the matching attribute
because attribute identity fields are immutable.

**Parameters:**

| Name        | Type   | Description                         |
| ----------- | ------ | ----------------------------------- |
| `name`      | `str`  | Local attribute name                |
| `value`     | `str`  | Attribute value                     |
| `namespace` | `str?` | Namespace URI                       |
| `prefix`    | `str?` | Preferred namespace prefix          |

**Returns:** `nil`.

### `delete_attr name :namespace?`

Deletes all attributes matching an expanded name.

**Parameters:**

| Name        | Type   | Description             |
| ----------- | ------ | ----------------------- |
| `name`      | `str`  | Local attribute name    |
| `namespace` | `str?` | Namespace URI           |

**Returns:** `bool`, whether an attribute was deleted.

### `push child`

Appends a child.

**Parameters:**

| Name    | Type          | Description |
| ------- | ------------- | ----------- |
| `child` | `Node`\|`str` | Child value |

**Returns:** `nil`.

### `traverse`

Returns a depth-first, parent-first iterator over the node and its descendants.

**Returns:** An iterator of nodes and text values in document order.

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
