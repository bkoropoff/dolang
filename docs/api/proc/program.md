# `Program`

Function proxy for an external program.

## Constructor

### `Program value`

Creates a program proxy.

#### Parameters

| Name    | Type                                                                       | Description          |
| ------- | -------------------------------------------------------------------------- | -------------------- |
| `value` | [`str`](../std/str.md)\|[`sym`](../std/sym.md)\|[`fs.Path`](../fs/path.md) | Program name or path |

#### Returns

A `Program`.

#### Example

```
let clang = Program "clang++"
clang --version
```

## Operators

### `(call) ...args :stdin? :stdout? :stderr? :policy? :mode?`

Runs the program with the supplied command-line arguments.

Output is written to the configured streams. See
[External Programs](../../shell/external-programs.md) for lookup, stream
handling, and pipeline behavior.

#### Parameters

| Name      | Type                                                        | Description                                                                    |
| --------- | ----------------------------------------------------------- | ------------------------------------------------------------------------------ |
| `...args` |                                                             | Command-line arguments                                                         |
| `stdin`   | [`fs.Path`](../fs/path.md)\|[`Iter`](../std/iter.md)        | Source for standard input                                                      |
| `stdout`  | [`fs.Path`](../fs/path.md)\|[`Sink`](../std/sink.md)        | Destination for standard output                                                |
| `stderr`  | [`fs.Path`](../fs/path.md)\|[`Sink`](../std/sink.md)\|`sym` | Destination for standard error, or `:STDOUT:` to merge it with standard output |
| `policy`  | [`Dict`](../std/dict.md)                                    | Per-launch termination overrides: `signal`, `grace`, and `force`               |
| `mode`    | [`sym`](../std/sym.md)                                      | `:LINE:` (default) or `:CHUNK:` output framing                                 |

#### Errors

Raises [`proc.Error`](./error.md) when the program exits unsuccessfully.

#### Example

```
let git = Program git
git status --short
```

## Methods

### `which()`

Resolves the program without running it.

#### Returns

[`fs.Path`](../fs/path.md), or `nil` when the program is not found.

#### Example

```
let git = Program :git:
echo $git.which()
```
