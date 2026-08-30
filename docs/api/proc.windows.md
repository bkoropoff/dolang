# proc.windows

Encodes and decodes arguments for Windows native process launches. These
functions follow the MSVC-compatible convention used by Rust process launching;
they do not parse `cmd.exe` or PowerShell syntax.

## Functions

### `join iterable`

Encodes an iterable of arguments as one Windows process command line.

#### Parameters

| Name       | Type | Description                                                           |
| ---------- | ---- | --------------------------------------------------------------------- |
| `iterable` |      | Values converted with [`std.verbatim`](./std/index.md#verbatim-value) |

#### Returns

[`str`](./std/str.md)

#### Example

```
assert_eq (join ["program", "two words"]) r#"program "two words""#
```

### `quote arg`

Encodes one argument for a Windows process command line.

#### Parameters

| Name  | Type | Description                                                          |
| ----- | ---- | -------------------------------------------------------------------- |
| `arg` |      | Value converted with [`std.verbatim`](./std/index.md#verbatim-value) |

#### Returns

[`str`](./std/str.md)

#### Example

```
assert_eq (quote "two words") r#""two words""#
```

### `split command_line`

Decodes a Windows process command line into an iterator of arguments.

#### Parameters

| Name           | Type                  | Description            |
| -------------- | --------------------- | ---------------------- |
| `command_line` | [`str`](./std/str.md) | Command line to decode |

#### Returns

`Iter` yielding [`str`](./std/str.md) arguments.

#### Example

```
assert_eq [...(split r#"program "two words""#)] ["program", "two words"]
```
