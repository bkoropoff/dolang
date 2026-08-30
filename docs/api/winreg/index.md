# winreg

The `winreg` module provides functions and types for reading and writing the
Windows registry.

This API is VFS-aware: it operates through the VFS in scope for the current
strand, so it works transparently under remote/elevated contexts (e.g.
`admin.with`). It is only supported on Windows targets; on other platforms
every operation throws [`sys.UnsupportedError`](../sys/unsupported-error.md).

## Types

| Type                             | Description                                        |
| -------------------------------- | -------------------------------------------------- |
| [`AccessMask`](./access-mask.md) | Registry key access rights                         |
| [`Key`](./key.md)                | An open registry key                               |
| [`LinkTarget`](./link-target.md) | A registry link's native and canonical target      |
| [`Value`](./value.md)            | One named value read from a key (name, kind, data) |

## Enumeration values

### Link resolution values

| Value      | Meaning                              |
| ---------- | ------------------------------------ |
| `:TARGET:` | Follow a registry link (the default) |
| `:LINK:`   | Open the link key itself             |

### Registry root values

| Value              | Meaning                              |
| ------------------ | ------------------------------------ |
| `:CLASSES_ROOT:`   | File associations and class settings |
| `:CURRENT_USER:`   | Current user's profile               |
| `:LOCAL_MACHINE:`  | Computer-wide configuration          |
| `:USERS:`          | All user profiles                    |
| `:CURRENT_CONFIG:` | Current hardware profile             |

### Registry value kind values

| Value                 | Stored value type            |
| --------------------- | ---------------------------- |
| `:SZ:`                | UTF-16 string                |
| `:EXPAND_SZ:`         | Expandable UTF-16 string     |
| `:MULTI_SZ:`          | Sequence of UTF-16 strings   |
| `:DWORD:`             | Little-endian 32-bit integer |
| `:DWORD_BIG_ENDIAN:`  | Big-endian 32-bit integer    |
| `:QWORD:`             | Little-endian 64-bit integer |
| `:BINARY:`            | Raw bytes                    |
| `:NONE:`              | No data                      |

### Registry view values

| Value       | Meaning                                   |
| ----------- | ----------------------------------------- |
| `:NATIVE:`  | The target process's native registry view |
| `:WOW32:`   | The 32-bit registry view                  |
| `:WOW64:`   | The 64-bit registry view                  |

## Functions

### `open root :view? :access? func?`

Opens a predefined registry root and returns a [`Key`](./key.md).

#### Parameters

| Name     | Type                                             | Description                                                  |
| -------- | ------------------------------------------------ | ------------------------------------------------------------ |
| `root`   | sym                                              | [Registry root](#registry-root-values)                       |
| `view`   | sym?                                             | [Registry view](#registry-view-values) (default: `:NATIVE:`) |
| `access` | [`AccessMask`](./access-mask.md)\|sym\|iterable? | Access rights (default: `:READ:`)                            |
| `func`   | `Func`?                                          | Function to run with the key; auto-closes when done          |

#### Returns

[`Key`](./key.md) when no `func` is given, otherwise the result
of calling `func`

#### Example

```
winreg.open :CURRENT_USER: do |root|
  echo (root.open("Environment").get "TEMP")

let root = winreg.open :LOCAL_MACHINE: access: :READ_WRITE:
root.close()
```
