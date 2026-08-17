# Attr

An XML attribute.

## Constructor

### `Attr name value :namespace? :prefix?`

Creates an attribute.

#### Parameters

| Name        | Type   | Description                  |
| ----------- | ------ | ---------------------------- |
| `name`      | `Str`  | Local attribute name         |
| `value`     | `Str`  | Attribute value              |
| `namespace` | `str?` | Namespace URI                |
| `prefix`    | `str?` | Preferred namespace prefix   |

#### Example

```
let attr = Attr "id" "123" namespace: "urn:inventory" prefix: "inv"
```

## Fields

### `name`

Read-only local attribute name.

### `value`

Mutable attribute value.

### `namespace`

Read-only namespace URI as a `Str`, or `nil` for no namespace.

### `prefix`

Read-only preferred prefix as a `Str`, or `nil`.

### `qname`

Read-only qualified name formed from `prefix` and `name`.
