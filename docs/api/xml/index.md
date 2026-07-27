# xml

Parses, edits, validates, and serializes XML element trees.

## Types

| Type                  | Description              |
| --------------------- | ------------------------ |
| [`Attr`](./attr.md)   | XML attribute            |
| [`Node`](./node.md)   | XML element node         |

## Functions

### `from_str xml`

Parses an XML document into a parentless element tree.

**Parameters:**

| Name  | Type  | Description         |
| ----- | ----- | ------------------- |
| `xml` | `str` | XML document        |

**Returns:** [`Node`](./node.md), the root element.

**Errors:**

- `ValueError` if the document is invalid, has no root element, or uses an
  unsupported entity.

Predefined entities and numeric character references are expanded into text
and attribute values. Custom DTD entities are not resolved.

```
let doc = from_str "<root><child>text</child></root>"
assert_eq $doc.tag "root"
```

### `to_str node`

Validates and serializes an XML tree.

Namespace declarations are generated from expanded names and namespace
snapshots. Prefix spelling and declaration placement can differ from the input,
but element and attribute namespace semantics are preserved.

**Parameters:**

| Name   | Type                         | Description          |
| ------ | ---------------------------- | -------------------- |
| `node` | [`Node`](./node.md)\|`str`   | XML node or text     |

**Returns:** `str`, the serialized XML.

**Errors:**

- `ValueError` if the tree is invalid or cyclic.

```
let n = Node "greeting"
n.push "hello"
assert_eq (to_str $n) "<greeting>hello</greeting>"
```

### `verify node`

Checks an entire XML tree without serializing it.

Validation includes names, namespace bindings, attribute uniqueness by
expanded name, child and attribute types, and cycles. A node can temporarily
contain invalid data while it is being edited.

**Parameters:**

| Name   | Type                    | Description  |
| ------ | ----------------------- | ------------ |
| `node` | [`Node`](./node.md)     | Root element |

**Returns:** `nil`.

**Errors:**

- `ValueError` if the tree is invalid or cyclic.

```
let doc = Node "root"
verify $doc
```
