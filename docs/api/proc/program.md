# `Program`

Callable proxy for an external program.

## Constructor

### `Program(value)`

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
