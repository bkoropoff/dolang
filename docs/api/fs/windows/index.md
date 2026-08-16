# fs.windows

The `fs.windows` module exposes Windows filesystem types.

## Types

| Type                                | Description                 |
| ----------------------------------- | --------------------------- |
| [`Path`](./path.md)                 | Windows path object         |
| [`StreamEntry`](./stream-entry.md)  | Alternate data stream entry |

## Functions

### `sec_desc path :owner? :group? :dacl? :sacl? :resolve?`

Gets selected parts of a Windows security descriptor.

#### Parameters

| Name      | Type                                           | Description                                                                                      |
| --------- | ---------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| `path`    | [`Str`](../../std/str.md)\|[`Path`](./path.md) | Path to query                                                                                    |
| `owner`   | [`Bool`](../../std/bool.md)?                   | Load the owner SID (default: `true`)                                                             |
| `group`   | [`Bool`](../../std/bool.md)?                   | Load the primary group SID (default: `true`)                                                     |
| `dacl`    | [`Bool`](../../std/bool.md)?                   | Load the discretionary ACL (default: `true`)                                                     |
| `sacl`    | [`Bool`](../../std/bool.md)?                   | Load the system ACL (default: `false`)                                                           |
| `resolve` | `:TARGET:`\|`:LINK:`?                          | Resolution mode (default: `:TARGET:`; see [fs's resolution modes](../index.md#resolution-modes)) |

#### Returns

[`security.windows.SecDesc`](../../security/windows/secdesc.md)

### `set_sec_desc path desc :resolve?`

Applies the components selected by a Windows security descriptor's `mask`.

#### Parameters

| Name      | Type                                                            | Description                                                                 |
| --------- | --------------------------------------------------------------- | --------------------------------------------------------------------------- |
| `path`    | [`Str`](../../std/str.md)\|[`Path`](./path.md)                  | Path to update                                                              |
| `desc`    | [`security.windows.SecDesc`](../../security/windows/secdesc.md) | Security descriptor to apply                                                |
| `resolve` | `:TARGET:`\|`:LINK:`                                            | Resolution mode (see [fs's resolution modes](../index.md#resolution-modes)) |
