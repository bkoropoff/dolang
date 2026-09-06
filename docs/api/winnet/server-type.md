# ServerType

Native Windows server-role flags returned by
[`MachineInfo`](./machine-info.md).

Unknown native bits are preserved. The roles worth testing individually are
available as boolean fields on [`MachineInfo`](./machine-info.md).

## Fields

### `int`

Returns the complete native mask as an integer, including unknown bits.

#### Returns

[`Int`](../std/int.md)
