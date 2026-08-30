# XattrEntry

Extended attribute entry returned by
[`xattrs`](index.md).

## Fields

### `name`

Attribute name.

Windows may report the name with different casing than as original set.

```
for attr = xattrs "data.txt"
  echo $attr.name
```

## Linux and FreeBSD Fields

### `namespace`

Extended attribute namespace, such as `user`.

```
for attr = xattrs "data.txt" namespace: :ANY:
  echo "$(attr.namespace):$(attr.name)"
```

## Windows Fields

### `flags`

Attribute flags.

### `size`

Attribute value size in bytes.
