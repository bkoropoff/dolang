# patch

The `patch` module parses, creates, encodes, and applies unified and git-style
patches.

## Types

| Type                             | Description                          |
| -------------------------------- | ------------------------------------ |
| [`ApplyError`](./applyerror.md)  | Error while applying a patch         |
| [`ParseError`](./parseerror.md)  | Patch parsing error                  |
| [`Patch`](./patch.md)            | One file-level patch operation       |
| [`PatchIter`](./patchiter.md)    | Iterator over a patch stream         |

## Functions

### `decode input`

Parses a patch stream.

#### Parameters

| Name    | Type                                              | Description                |
| ------- | ------------------------------------------------- | -------------------------- |
| `input` | [`Str`](../std/str.md)\|[`Bin`](../std/bin.md)    | Unified or git-style patch |

#### Returns

[`PatchIter`](./patchiter.md)

#### Errors

| Exception                       | Condition                              |
| ------------------------------- | -------------------------------------- |
| [`ParseError`](./parseerror.md) | Iteration reaches malformed patch data |

#### Example

```
let patches = [...patch.decode diff_text]
```

### `diff before after :source? :target?`

Builds a text patch from two versions of the same content.

`before` and `after` must both be [`Str`](../std/str.md) or both be
[`Bin`](../std/bin.md).

#### Parameters

| Name     | Type                                                                               | Description                            |
| -------- | ---------------------------------------------------------------------------------- | -------------------------------------- |
| `before` | [`Str`](../std/str.md)\|[`Bin`](../std/bin.md)                                     | Original content                       |
| `after`  | [`Str`](../std/str.md)\|[`Bin`](../std/bin.md)                                     | Modified content                       |
| `source` | [`Path`](../fs/path.md)\|[`Str`](../std/str.md)?                                   | Source filename for the patch headers  |
| `target` | [`Path`](../fs/path.md)\|[`Str`](../std/str.md)?                                   | Target filename for the patch headers  |

#### Returns

[`Patch`](./patch.md)

#### Errors

| Exception   | Condition                                                                       |
| ----------- | ------------------------------------------------------------------------------- |
| `TypeError` | `before` and `after` are not both text or both binary                           |
| `TypeError` | `source` or `target` is not a [`Path`](../fs/path.md) or [`Str`](../std/str.md) |

#### Example

```
let p = patch.diff "alpha\n" "beta\n" source: old.txt target: new.txt
echo (patch.encode p)
```

### `encode value`

Encodes a [`Patch`](./patch.md) or iterable of patches back to patch text.

When every encoded byte is valid UTF-8, this returns a
[`Str`](../std/str.md). Otherwise it returns [`Bin`](../std/bin.md).

#### Parameters

| Name    | Type                              | Description                          |
| ------- | --------------------------------- | ------------------------------------ |
| `value` | [`Patch`](./patch.md)\|iterable   | One patch or an iterable of patches  |

#### Returns

[`Str`](../std/str.md)\|[`Bin`](../std/bin.md)

#### Errors

| Exception   | Condition                                |
| ----------- | ---------------------------------------- |
| `TypeError` | An iterable contains a non-`Patch` value |

#### Example

```
let patches = [...patch.decode diff_text]
write output.patch (patch.encode patches)
```
