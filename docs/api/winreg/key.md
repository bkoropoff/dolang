# Key

An open registry key, returned by
[`winreg.open`](./index.md#open-root-view-access-func) or one of `Key`'s own
`open`/`create` methods.

## Methods

### `open subpath :view? :access? :resolve? func?`

Opens a subkey relative to this key.

#### Parameters

| Name      | Type                                             | Description                                                            |
| --------- | ------------------------------------------------ | ---------------------------------------------------------------------- |
| `subpath` | [`Str`](../std/str.md)                           | Path to the subkey, relative to this key                               |
| `view`    | sym?                                             | [Registry view](./index.md#registry-view-values) (default: `:NATIVE:`) |
| `access`  | [`AccessMask`](./access-mask.md)\|sym\|iterable? | Access rights (default: `:READ:`)                                      |
| `resolve` | sym?                                             | `:TARGET:` follows links; `:LINK:` opens the link key                  |
| `func`    | func?                                            | Function to run with the key; auto-closes when done                    |

#### Returns

[`Key`](./key.md) when no `func` is given, otherwise the result
of calling `func`

#### Errors

- [`sys.NotFoundError`](../sys/not-found-error.md) — the subkey does not exist
- [`sys.PermissionDeniedError`](../sys/permission-denied-error.md) — access
    was denied

#### Example

```
winreg.open :CURRENT_USER: do |root|
  root.open "Environment" do |env|
    echo (env.get "TEMP")
```

### `link target_root target_subpath link_subpath :view?`

Creates a registry link. The target may not exist. Arguments follow `ln -s`
ordering: target first, link destination last.

**Parameters:**

| Name             | Type                   | Description                                        |
| ---------------- | ---------------------- | -------------------------------------------------- |
| `target_root`    | sym                    | [Predefined root](./index.md#registry-root-values) |
| `target_subpath` | [`Str`](../std/str.md) | Target path relative to `target_root`              |
| `link_subpath`   | [`Str`](../std/str.md) | New link path relative to this key                 |
| `view`           | sym?                   | View used for target mapping and destination       |

**Errors:**

- [`sys.AlreadyExistsError`](../sys/already-exists-error.md) — the destination
  already exists and was not modified
- `sys.InvalidInputError` — a path contains NUL

```
root.link :CURRENT_USER: r"Software\MyApp" "MyAppLink"
```

### `read_link subpath :view?`

Reads a registry link without following it.

**Parameters:**

| Name      | Type                   | Description                         |
| --------- | ---------------------- | ----------------------------------- |
| `subpath` | [`Str`](../std/str.md) | Link path relative to this key      |
| `view`    | sym?                   | Registry view (default: `:NATIVE:`) |

**Returns:** [`LinkTarget`](./link-target.md)

**Errors:**

- `sys.InvalidInputError` — the key is not a link
- `sys.InvalidDataError` — the link value is malformed

Aliases such as `:CLASSES_ROOT:` and `:CURRENT_CONFIG:` may project to their
physical `:LOCAL_MACHINE:` or `:USERS:` backing path.

### `create subpath :view? :access? func?`

Creates a subkey relative to this key, or opens it if it already exists.

#### Parameters

Same as [`open`](#open-subpath-view-access-resolve-func).

#### Returns

[`Key`](./key.md) when no `func` is given, otherwise the result
of calling `func`

#### Example

```
winreg.open :CURRENT_USER: access: :READ_WRITE: do |root|
  root.create "Software/MyApp" do |app|
    app.set "installed" true
```

### `delete subpath :view? :all? :ignore?`

Deletes a subkey. By default, the subkey must have no children. With `all:
true`, its values and descendants are deleted recursively. With `ignore: true`,
a missing subkey is ignored. Recursive deletion removes a registry link itself
without traversing or modifying its target.

#### Parameters

| Name      | Type                     | Description                                                            |
| --------- | ------------------------ | ---------------------------------------------------------------------- |
| `subpath` | [`Str`](../std/str.md)   | Path to the subkey, relative to this key                               |
| `view`    | sym?                     | [Registry view](./index.md#registry-view-values) (default: `:NATIVE:`) |
| `all`     | [`Bool`](../std/bool.md) | If `true`, deletes values and descendants recursively                  |
| `ignore`  | [`Bool`](../std/bool.md) | If `true`, ignores a missing subkey                                    |

#### Errors

- [`sys.NotFoundError`](../sys/not-found-error.md) — the subkey does not exist
- Without `all: true`, deleting a subkey that still has children raises

#### Example

```
winreg.open :CURRENT_USER: access: :READ_WRITE: do |root|
  root.delete "Software/MyApp"
  root.delete "Software/MyAppTree" all: true
  root.delete "Software/Missing" ignore: true
```

### `close()`

Closes the key. Keys not explicitly closed are closed when garbage
collected. Idempotent — closing an already-closed key is a no-op.

### `subkeys()`

Opens a live forward enumeration of immediate child-key names. Entries are
fetched as iteration advances.

#### Returns

Iterable of [`Str`](../std/str.md). `.len` is the count captured when the
enumeration is opened.

#### Example

```
for name = key.subkeys()
  echo $name
```

### `values()`

Opens a live forward enumeration of this key's values. Entries are fetched as
iteration advances.

#### Returns

An iterable sequence of [`Value`](./value.md) entries. `.len` is the count
captured when the enumeration is opened.

#### Example

```
for entry = key.values()
  echo "$(entry.name) ($(entry.kind)): $(entry.value)"
```

### `get name`

Reads a value and returns its coerced Do representation.

#### Parameters

| Name   | Type                   | Description                                        |
| ------ | ---------------------- | -------------------------------------------------- |
| `name` | [`Str`](../std/str.md) | Value name; `""` refers to the key's default value |

#### Returns

The value's natural Do representation — see
[`Value.value`](./value.md#value) for the kind mapping

#### Errors

- [`sys.NotFoundError`](../sys/not-found-error.md) — no value with that name
    exists

#### Example

```
let temp = env.get "TEMP"
```

### `get_value name`

Reads a value and returns it as a [`Value`](./value.md), or `nil` if it
doesn't exist. Unlike [`get`](#get-name), this never raises for a missing
value, and it preserves the value's kind rather than just its coerced data —
use it to test for existence or to inspect a value's raw `REG_*` kind.

#### Parameters

Same as [`get`](#get-name).

#### Returns

[`Value`](./value.md) or `nil`

#### Example

```
let entry = env.get_value "TEMP"
if entry
  echo "TEMP is a $entry.kind value: $entry.value"
else
  echo "TEMP is not set"
```

### `set name value :kind?`

Writes a value.

Without `kind`, the Do value is coerced into whatever kind is already stored
under `name`, if any (an unrecognized raw `REG_*` kind — see
[`Value.kind`](./value.md#kind) — round-trips by writing `value` back as the
same raw kind, so it must be a [`Bin`](../std/bin.md)). If no value exists yet,
a kind is inferred from `value`'s own Do type: [`Str`](../std/str.md) → `:SZ:`,
an iterable of `Str` → `:MULTI_SZ:`, [`Bool`](../std/bool.md) → `:DWORD:`
with data `0` or `1`, [`Int`](../std/int.md) → `:DWORD:` if it fits in 32 bits,
else `:QWORD:`, [`Bin`](../std/bin.md) → `:BINARY:`, `nil` → `:NONE:`.
Coercing a `Bool` to an existing or explicitly selected DWORD/QWORD kind
preserves that kind. Reading it back returns an `Int`.

#### Parameters

| Name    | Type                   | Description                                                |
| ------- | ---------------------- | ---------------------------------------------------------- |
| `name`  | [`Str`](../std/str.md) | Value name; `""` refers to the key's default value         |
| `value` |                        | The Do value to write                                      |
| `kind`  | sym?                   | [Stored value type](./index.md#registry-value-kind-values) |

#### Example

```
key.set "installed" true
key.set "path" "C:\\Program Files\\MyApp"
key.set "tags" ["a", "b"]
key.set "raw" b"\x01\x02" kind: :BINARY:
```

### `delete_value name`

Deletes a value.

#### Parameters

| Name   | Type                   | Description                                        |
| ------ | ---------------------- | -------------------------------------------------- |
| `name` | [`Str`](../std/str.md) | Value name; `""` refers to the key's default value |

#### Errors

- [`sys.NotFoundError`](../sys/not-found-error.md) — no value with that name
    exists

#### Example

```
key.delete_value "installed"
```

### `sec_desc :owner? :group? :dacl? :sacl?`

Gets selected parts of the key's Windows security descriptor through its
existing handle.

#### Parameters

| Name    | Type                      | Description                                  |
| ------- | ------------------------- | -------------------------------------------- |
| `owner` | [`Bool`](../std/bool.md)? | Load the owner SID (default: `true`)         |
| `group` | [`Bool`](../std/bool.md)? | Load the primary group SID (default: `true`) |
| `dacl`  | [`Bool`](../std/bool.md)? | Load the discretionary ACL (default: `true`) |
| `sacl`  | [`Bool`](../std/bool.md)? | Load the system ACL (default: `false`)       |

#### Returns

[`security.windows.SecDesc`](../security/windows/secdesc.md)

The key must have the access rights required for the requested components.

### `set_sec_desc desc`

Applies the components selected by a Windows security descriptor's `mask`
through the key's existing handle.

#### Parameters

| Name   | Type                                                         | Description                  |
| ------ | ------------------------------------------------------------ | ---------------------------- |
| `desc` | [`security.windows.SecDesc`](../security/windows/secdesc.md) | Security descriptor to apply |

The key must have the access rights required for the selected components.
