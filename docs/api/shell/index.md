# shell

The `shell` module provides shell-context values and functions.

## Types

| Name                            | Description                                              |
| ------------------------------- | -------------------------------------------------------- |
| [`Vfs`](./vfs.md)               | Execution context handle                                 |

## Functions

### `exit code?`

Exits the current shell with the given status code.

#### Parameters

| Name   | Type                   | Description              |
| ------ | ---------------------- | ------------------------ |
| `code` | [`Int`](../std/int.md) | exit status (default: 0) |

#### Returns

never returns; raises an interrupt error

### `cd path? func?`

With no arguments, returns the current working directory. With a path, changes
the current working directory. If a callable is also provided, the directory is
changed only for the duration of that call, then restored.

#### Parameters

| Name   | Type                                            | Description                          |
| ------ | ----------------------------------------------- | ------------------------------------ |
| `path` | [`Str`](../std/str.md)\|[`Path`](../fs/path.md) | directory path                       |
| `func` |                                                 | callable to run in the new directory |

#### Returns

Current working directory (no arguments), or result of `func`.

### `with_host func ...args`

Executes a callable in the interpreter's original host context, regardless of
the current or nested VFS contexts.

#### Parameters

| Name   | Type | Description                            |
| ------ | ---- | -------------------------------------- |
| `func` | func | Block to execute in fresh host context |
| `args` |      | Additional arguments to pass to `func` |

#### Returns

Return value of the executed callable

### `with_override func :args? :program?`

Runs `func` with scoped command-line arguments or program identity. Strands
created within the call inherit the overrides.

**Parameters:**

| Name      | Type                                              | Description                                              |
| --------- | ------------------------------------------------- | -------------------------------------------------------- |
| `func`    | callable                                          | Block to execute                                         |
| `args`    | [`Iterable`](../std/iterable.md)?                 | Values converted with [`arg`](../std/index.md#arg-value) |
| `program` | [`Str`](../std/str.md)?\|[`Path`](../fs/path.md)? | Program identity                                         |

**Returns:** Return value of `func`.

`args:` and `program:` are independent. An omitted argument retains its
current value, so nested calls can override only one part of the invocation
identity. The previous values are restored when `func` returns or raises an
error.

**Errors:**

- Raises [`TypeError`](../std/type-error.md) if `args` is not iterable.
- Raises [`TypeError`](../std/type-error.md) if `program` is not a string or
  path.

### `vfs_exe()`

Returns the current executable reported by the active VFS context, or `nil`
when running on the host filesystem.

#### Returns

[`fs.Path`](../fs/path.md) or `nil`.

### `env overrides func`

Runs `func` with scoped environment overrides. Keys may be strings or symbols.
`nil` unsets a variable and `:INHERIT:` captures its current strand value.

**Parameters:**

| Name        | Type                     | Description           |
| ----------- | ------------------------ | --------------------- |
| `overrides` | [`Dict`](../std/dict.md) | Environment overrides |
| `func`      | callable                 | Block to run          |

## Values

### `env`

An object for accessing environment variables.

### `args`

An immutable [`Args`](./args.md) sequence containing the command-line arguments
for the current invocation.

### `program`

Identifies what `dolang` is executing.

- For `dolang script.dol`, this is an [`fs.Path`](../fs/path.md) for
  `script.dol`.
- For `dolang -m foo.bar`, this is the string `"foo.bar"`.
- In the REPL, this is `nil`.

### `exe`

An [`fs.Path`](../fs/path.md) containing the path returned by the host for the
current `dolang` executable. The path is not automatically canonicalized.

### `VERSION`

A `(major, minor, patch)` [`Tuple`](../std/tuple.md) with the version of the
running `dolang` build.
