# Metadata

Filesystem metadata object returned by [`metadata`](./index.md),
[`Path.metadata()`](./path.md), and [`File.metadata()`](./file.md).

For filesystem-level capacity and mount metadata, use
[`FsMetadata`](./fs-metadata.md).

Accessing a field that does not apply to the target platform raises a field
error. `freebsd_attrs`, `linux_attrs`, `macos_attrs`, and related attribute
fields are `nil` when the filesystem or file type does not support querying
file attributes.

## Fields

### `accessed`

Last access time as [`DateTime`](../time/datetime.md).

### `created`

Creation or status-change time as [`DateTime`](../time/datetime.md).

### `group`

Owning group: the gid as an [`Int`](../std/int.md) on Unix, or the primary group
[`Sid`](../security/windows/sid.md) on Windows. This representation matches the
`group` argument accepted by
[`update_metadata`](./index.md#update_metadata-resolve-paths).

### `modified`

Last modification time as [`DateTime`](../time/datetime.md).

### `size`

File size in bytes.

### `type`

File type as a [`Sym`](../std/sym.md): `:FILE:`, `:DIR:`, `:SYMLINK:`,
`:FIFO:`, `:CHAR_DEVICE:`, `:BLOCK_DEVICE:`, `:SOCKET:`, or `:UNKNOWN:`.

### `owner`

Owner: the uid as an [`Int`](../std/int.md) on Unix, or the owner
[`Sid`](../security/windows/sid.md) on Windows. This representation matches the
`owner` argument accepted by
[`update_metadata`](./index.md#update_metadata-resolve-paths).

## Windows-Only Fields

### `archive`

Whether the archive attribute is set.

### `encrypted`

Whether the encrypted attribute is set.

### `not_content_indexed`

Whether the not-content-indexed attribute is set.

### `offline`

Whether the offline attribute is set.

### `readonly`

Whether the readonly attribute is set.

### `reparse_point`

Whether the reparse-point attribute is set.

### `sparse`

Whether sparse allocation is enabled.

### `system`

Whether the system attribute is set.

### `temporary`

Whether the temporary attribute is set.

### `win_attrs`

Raw Windows file attribute bitmask.

## Linux-Only Fields

### `casefold`

Whether the case-insensitive-directory-lookups flag is set.

### `data_journaling`

Whether the data-journaling flag is set.

### `dir_sync`

Whether the synchronous-directory-updates flag is set.

### `direct_access`

Whether the direct-access flag is set.

### `extent_format`

Whether the extent-format flag is set.

### `linux_attrs`

Raw Linux attribute flags.

### `no_atime`

Whether the no-atime flag is set.

### `no_compress`

Whether the don't-compress flag is set.

### `no_copy_on_write`

Whether the no-copy-on-write flag is set.

### `no_tail_merge`

Whether the no-tail-merging flag is set.

### `project_inherit`

Whether the project-hierarchy flag is set.

### `secure_delete`

Whether the secure-deletion flag is set.

### `sync`

Whether the synchronous-updates flag is set.

### `top_dir`

Whether the top-of-directory-hierarchy flag is set.

### `undelete`

Whether the undeletable flag is set.

## macOS-Only Fields

### `macos_attrs`

Raw macOS file flags.

### `opaque`

Whether the opaque flag is set.

## FreeBSD-Only Fields

### `freebsd_attrs`

Raw FreeBSD file flags.

## Platform Attribute Fields

### `append_only`

Whether the platform append-only flag is set.

### `compressed`

Whether the platform compressed flag is set.

### `hidden`

Whether the platform hidden flag is set.

### `immutable`

Whether the platform immutable flag is set.

### `no_dump`

Whether the platform no-dump flag is set.

## Unix-Only Fields

### `blksize`

Preferred block size for I/O.

### `blocks`

Number of allocated 512-byte blocks.

### `dev`

Device ID.

### `gid`

Owner group ID.

### `ino`

Inode number.

### `mode`

Permissions and file type as a [`fs.unix.Mode`](./unix/mode.md).

### `nlink`

Hard-link count.

### `rdev`

Special-device ID.

### `uid`

Owner user ID.
