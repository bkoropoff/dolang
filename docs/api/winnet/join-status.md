# JoinStatus

The machine's current workgroup or domain membership, returned by
[`join_status`](./index.md#join_status).

## Fields

### `kind`

How the machine is named on the network: `:DOMAIN:`, `:WORKGROUP:`,
`:UNJOINED:`, or `:UNKNOWN:` when the state could not be determined.

### `name`

The domain or workgroup name as a `Str`, or `nil` when `kind` is `:UNJOINED:`
or `:UNKNOWN:`.

## Example

```
let status = winnet.join_status()
if (status.kind == :DOMAIN:)
  echo "joined to $(status.name)"
else
  echo "not domain-joined"
```
