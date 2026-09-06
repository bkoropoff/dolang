# MachineInfo

Machine identity and server role, returned by
[`machine_info`](./index.md#machine_info).

Identity comes from the workstation service and the role fields from the
server service. When the server service is not running, `server_started` is
`false`, `comment` is `nil` and every role field is `false`; the identity
fields are still populated.

## Fields

### `name`

NetBIOS computer name as a `Str`.

### `domain`

Domain or workgroup the machine belongs to, as a `Str`.

Use [`join_status`](./index.md#join_status) to tell which of the two it is.

### `version_major`

Major OS version as an `Int`.

### `version_minor`

Minor OS version as an `Int`.

### `comment`

Server comment as a `Str`, or `nil`.

### `server_type`

Advertised server role mask.

#### Returns

[`ServerType`](./server-type.md)

### `server_started`

Whether the server service supplied `server_type` and `comment`.

### `workstation`

Whether the machine advertises the workstation role.

### `server`

Whether the machine advertises the server role.

### `domain_controller`

Whether the machine advertises as a domain controller.

### `backup_domain_controller`

Whether the machine advertises as a backup domain controller.

## Example

```
let machine = winnet.machine_info()
echo "$(machine.name) is running $(machine.version_major).$(machine.version_minor)"
if machine.domain_controller
  echo "skipping: cannot unjoin a domain controller"
```
