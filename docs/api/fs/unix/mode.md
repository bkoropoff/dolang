# `Mode`

Unix `st_mode` bits: permissions, the set-user-ID/set-group-ID/sticky bits,
and the file type.

## Constructor

### `Mode ...bits`

Constructs a mode from symbols or one iterable of symbols.

#### Permission symbols

| Symbol             | Meaning                 |
| ------------------ | ----------------------- |
| `:OWNER_READ:`     | Owner may read          |
| `:OWNER_WRITE:`    | Owner may write         |
| `:OWNER_EXECUTE:`  | Owner may execute       |
| `:GROUP_READ:`     | Group may read          |
| `:GROUP_WRITE:`    | Group may write         |
| `:GROUP_EXECUTE:`  | Group may execute       |
| `:OTHER_READ:`     | Everyone may read       |
| `:OTHER_WRITE:`    | Everyone may write      |
| `:OTHER_EXECUTE:`  | Everyone may execute    |
| `:SET_UID:`        | Set user ID on execute  |
| `:SET_GID:`        | Set group ID on execute |
| `:STICKY:`         | Sticky bit              |

#### File type symbols

| Symbol          | File type        |
| --------------- | ---------------- |
| `:IFREG:`       | Regular file     |
| `:IFDIR:`       | Directory        |
| `:IFLNK:`       | Symbolic link    |
| `:IFIFO:`       | FIFO             |
| `:IFCHR:`       | Character device |
| `:IFBLK:`       | Block device     |
| `:IFSOCK:`      | Socket           |

#### Example

```
let rw = Mode(:OWNER_READ:, :OWNER_WRITE:)
```

## Fields

### `group`

Group permissions as a
[`security.unix.Permission`](../../security/unix/permission.md).

### `int`

The raw `st_mode` value, including the file type bits. Mask with `0o7777` for
the permission bits alone.

```
let mode = metadata("script.sh").mode
assert_eq (mode.int & 0o7777) 0o755
```

### `other`

Permissions for everyone else, as a
[`security.unix.Permission`](../../security/unix/permission.md).

### `owner`

Owner permissions as a
[`security.unix.Permission`](../../security/unix/permission.md).

## Class Methods

### `from_int value`

Constructs a mode from a raw `st_mode` value, preserving unknown bits.

#### Parameters

| Name    | Type                        | Description    |
| ------- | --------------------------- | -------------- |
| `value` | [`Int`](../../std/int.md)   | Raw mode bits  |

#### Returns

`Mode`

#### Example

```
let mode = Mode.from_int 0o644
```

## Methods

### `contains bit`

Tests whether all bits represented by a symbol are set.

#### Returns

[`Bool`](../../std/bool.md)

## Operators

`|`, `&`, and `^` combine modes. `~` complements a mode within the supported
bit set. `==` compares modes. Iteration yields the symbols represented by a
mode.

## Example

Reading a file's mode and applying it elsewhere:

```
let mode = metadata("template.sh").mode
update_metadata "script.sh" mode: $mode
```
