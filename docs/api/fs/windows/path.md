# Path

[`fs.Path`](../path.md) using Windows path syntax.

## Constructor

### `Path path`

#### Parameters

| Name   | Type                                               | Description |
| ------ | -------------------------------------------------- | ----------- |
| `path` | [`Str`](../../std/str.md)\|[`fs.Path`](../path.md) | Path value  |

Converting a Unix path is allowed only when it is relative.

See [`fs.Path`](../path.md) for shared fields, methods, and operators.

## Fields

### `disk`

Drive letter for `C:`-style and `\\?\C:`-style prefixes, or `nil` otherwise.

#### Example

```
let path = Path "C:/work/file.txt"
echo $path.disk  # C
```

### `server`

UNC server name, or `nil` if the path does not use a UNC prefix.

#### Example

```
let path = Path "//server/share/file.txt"
echo $path.server  # server
```

### `share`

UNC share name, or `nil` if the path does not use a UNC prefix.

#### Example

```
let path = Path "//server/share/file.txt"
echo $path.share  # share
```

### `device`

Device namespace name for `\\.\name` paths, or `nil` otherwise.

#### Example

```
let path = Path r"\\.\COM42"
echo $path.device  # COM42
```

### `is_verbatim`

Returns whether the path uses a verbatim `\\?\...` prefix.

#### Example

```
let path = Path r"\\?\C:\work\file.txt"
echo $path.is_verbatim  # true
```

### `stream_name`

Alternate data stream name, or `nil` if no stream is specified.

#### Example

```
let path = Path "file.txt:zone"
echo $path.name         # file.txt
echo $path.stream_name  # zone
```

### `stream_type`

Alternate data stream type without the leading `$`, or `nil` if no alternate
data stream was specified, or an alternate data stream was specified without an
explicit type.

#### Example

```
let path = Path "file.txt:zone:$DATA"
echo $path.stream_type  # DATA
```

## Methods

### `sec_desc :owner? :group? :dacl? :sacl? :resolve?`

Gets selected parts of the Windows security descriptor.

#### Parameters

| Name      | Type                         | Description                                                                   |
| --------- | ---------------------------- | ----------------------------------------------------------------------------- |
| `owner`   | [`Bool`](../../std/bool.md)? | Load the owner SID (default: `true`)                                          |
| `group`   | [`Bool`](../../std/bool.md)? | Load the primary group SID (default: `true`)                                  |
| `dacl`    | [`Bool`](../../std/bool.md)? | Load the discretionary ACL (default: `true`)                                  |
| `sacl`    | [`Bool`](../../std/bool.md)? | Load the system ACL (default: `false`)                                        |
| `resolve` | `:TARGET:`\|`:LINK:`?        | Resolution mode (default: `:TARGET:`; see [fs](../index.md#resolution-modes)) |

#### Returns

[`security.windows.SecDesc`](../../security/windows/secdesc.md)

SACL access requires `SeSecurityPrivilege`.

### `set_sec_desc desc? :resolve? ...options`

Applies the components selected by a `SecDesc`'s `mask`.

#### Parameters

| Name      | Type                                                                                                                     | Description                                                                   |
| --------- | ------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------- |
| `desc`    | [`security.windows.SecDesc`](../../security/windows/secdesc.md)\|[`Bin`](../../std/bin.md)\|[`Dict`](../../std/dict.md)? | Descriptor, packet, or spec                                                   |
| `resolve` | `:TARGET:`\|`:LINK:`?                                                                                                    | Resolution mode (default: `:TARGET:`; see [fs](../index.md#resolution-modes)) |

The descriptor's
[component options](../../security/windows/secdesc.md#component-options) may be
passed as keyword arguments instead of, or alongside, `desc`, exactly as
[`sec_desc`](../../security/windows/index.md#sec_desc-desc-options) accepts
them.

#### Example

```
path.set_sec_desc
  owner: :BUILTIN_ADMINISTRATORS:
  dacl_protected: true
  dacl:
    - allow: :LOCAL_SYSTEM:
      mask: :GENERIC_ALL:
    - allow: :BUILTIN_ADMINISTRATORS:
      mask: :GENERIC_ALL:
```

Windows may normalize the descriptor when associating it with the
filesystem object.

### `streams :resolve?`

Lists alternate data streams for this path.

#### Parameters

| Name      | Type                  | Description                                                                   |
| --------- | --------------------- | ----------------------------------------------------------------------------- |
| `resolve` | `:TARGET:`\|`:LINK:`? | Resolution mode (default: `:TARGET:`; see [fs](../index.md#resolution-modes)) |

#### Returns

iterator of [`StreamEntry`](stream-entry.md)

#### Example

```
let path = Path "data.txt"
for stream = path.streams()
  echo "$(stream.name) $(stream.type)"
  echo (path / stream)
```
