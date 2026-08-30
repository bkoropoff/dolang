# security.nfs4

The `security.nfs4` module exposes NFSv4 access-control-list types.

## Types

| Type                  | Description                       |
| --------------------- | --------------------------------- |
| [`Ace`](./ace.md)     | NFSv4 access-control entry        |
| [`Acl`](./acl.md)     | NFSv4 access-control list         |
| [`Flags`](./flags.md) | NFSv4 ACE inheritance/audit flags |
| [`Mask`](./mask.md)   | NFSv4 ACE permission mask         |

## Functions

### `ace :allow? :deny? :audit? :alarm? mask: :flags?`

Constructs an NFSv4 entry from declarative arguments. Pass exactly one type
key. Its value is `:OWNER:`, `:OWNING_GROUP:`, `:EVERYONE:`, `{user: id}`, or
`{group: id}`. `mask:` is required; `flags:` defaults to empty. Masks and flags
accept their built type, one flag symbol, or an iterable of flag symbols.

```
ace allow: :OWNER: mask: [:READ_DATA:, :READ_ACL:]
ace deny: {user: 1000} mask: :WRITE_DATA:
```

### `acl ...aces`

Constructs an NFSv4 ACL from [`Ace`](./ace.md) values and declarative ACE
dictionaries. Pass collections with `...` to spread their entries. An empty
ACL is valid.
