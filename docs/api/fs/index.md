# fs

The `fs` module provides functions and types for filesystem operations.

Ordinary metadata such as size, timestamps, ownership, permissions, and file
attributes are available through [`Metadata`](./metadata.md) and
[`set_metadata`](#set_metadata-resolve-paths). Extended attributes use
[`xattrs`](#xattrs-path-namespace-resolve) and related functions. POSIX and
NFSv4 ACLs use [`acl`](#acl-path-kind-posix-default-resolve) and
[`set_acl`](#set_acl-path-acl-kind-default-resolve). Windows security
descriptors can also be fetched and manipulated with full fidelity; see
[`fs.windows.sec_desc`](windows/index.md#sec_desc-path-owner-group-dacl-sacl-resolve)
and the [Security Guide](../../shell/security.md). Windows alternate data
streams are listed with [`streams`](#streams-path-resolve).

## Types

| Type                         | Description                    |
| ---------------------------- | ------------------------------ |
| [DirEntry](direntry.md)      | Directory entry object         |
| [Metadata](metadata.md)      | Immutable filesystem metadata  |
| [Path](path.md)              | Supertype for filesystem paths |
| [XattrEntry](xattr-entry.md) | Extended attribute entry       |

## Modules

| Module                             | Description              |
| ---------------------------------- | ------------------------ |
| [`fs.unix`](unix/index.md)         | Unix filesystem types    |
| [`fs.windows`](windows/index.md)   | Windows filesystem types |

## Resolution modes

Many functions accept a `resolve:` parameter that controls how symbolic links
and other recursive path resolution is handled. Two values are accepted:

- **`:TARGET:`** — Resolve all links to their final target. This is the default
  for most functions.
- **`:LINK:`** — Resolve all links except the final
  component. For example, given a symlink `link -> target`, `metadata link
  resolve: :LINK:` returns the link's own metadata rather than the target's.
  This is the default for `glob`.

On Unix, `:LINK:` corresponds to `lstat`-style behavior. On Windows, it applies
to both symbolic links and other reparse points such as directory junctions.

## Functions

### `absolute path`

Returns the absolute form of a path based on the current working directory.

#### Parameters

| Name   | Type                                      | Description           |
| ------ | ----------------------------------------- | --------------------- |
| `path` | [`Str`](../std/str.md)\|[`Path`](path.md) | Path to make absolute |

#### Returns

[`Path`](path.md) - Absolute path

#### Example

```
let abs = absolute "./config.txt"
echo $abs  # /current/working/dir/config.txt

# Already absolute paths are unchanged
let unchanged = absolute "/etc/passwd"
echo $unchanged  # /etc/passwd
```

### `acl path :kind = :POSIX: :default? :resolve?`

Gets the ACL stored on a path.

#### Parameters

| Name      | Type                                      | Description                                      |
| --------- | ----------------------------------------- | ------------------------------------------------ |
| `path`    | [`Str`](../std/str.md)\|[`Path`](path.md) | Path to query                                    |
| `kind`    | `:POSIX:`\|`:NFS4:`\|`:MACOS:`            | ACL format to query                              |
| `default` | [`Bool`](../std/bool.md)                  | Query the directory's inheritable default ACL    |
| `resolve` | `:TARGET:`\|`:LINK:`                      | Resolution mode (see [above](#resolution-modes)) |

#### Returns

[`security.unix.Acl`](../security/unix/acl.md),
[`security.nfs4.Acl`](../security/nfs4/acl.md), or
[`security.macos.Acl`](../security/macos/acl.md), depending on `kind`, or
`nil` when no ACL metadata is stored.

#### Errors

| Exception              | Condition                                                                                        |
| ---------------------- | ------------------------------------------------------------------------------------------------ |
| `ValueError`           | `kind: :NFS4:` or `:MACOS:` is combined with `default: true`                                     |
| `sys.UnsupportedError` | The target and ACL format combination is unsupported                                             |

POSIX ACLs (`kind: :POSIX:`, the default) are supported on Linux and FreeBSD.
NFSv4 ACLs (`kind: :NFS4:`) are supported on FreeBSD only. macOS ACLs
(`kind: :MACOS:`) are supported on macOS only.

### `append path content`

Appends content to a file, creating it if needed.

#### Parameters

| Name      | Type                                      | Description                 |
| --------- | ----------------------------------------- | --------------------------- |
| `path`    | [`Str`](../std/str.md)\|[`Path`](path.md) | Path to the file to append  |
| `content` | `Str`\|`Bin`                              | Content to append           |

#### Returns

[`Int`](../std/int.md) - Number of bytes written

#### Example

```
append "messages.txt" "another message\n"
append "data.bin" b"\x04\x05"
```

### `cache_dir :app?`

Returns the platform-native user cache directory as a [`Path`](path.md).

When `app` is given, the result is scoped to that application.

#### Parameters

| Name  | Type                    | Description      |
| ----- | ----------------------- | ---------------- |
| `app` | [`Str`](../std/str.md)? | Application name |

#### Platform behavior

Without `app`, the base directories are:

| Platform       | Result                                                                  |
| -------------- | ----------------------------------------------------------------------- |
| Non-macOS Unix | `$XDG_CACHE_HOME`, otherwise `~/.cache`                                 |
| macOS          | `(home_dir() / "Library" / "Caches")`                                   |
| Windows        | `FOLDERID_LocalAppData`, typically `(home_dir() / "AppData" / "Local")` |

With `app: myapp`:

| Platform       | Result                              |
| -------------- | ----------------------------------- |
| Non-macOS Unix | `(cache_dir() / "myapp)"`           |
| macOS          | `(cache_dir() / "myapp)"`           |
| Windows        | `(cache_dir() / "myapp" / "Cache")` |

#### Returns

[`Path`](path.md)

#### Example

```
let cache = cache_dir app: blastinator8000
echo "Cache: $cache"
```

### `canonical path`

Returns the canonical, absolute form of a path with all intermediate components
normalized and symbolic links resolved.

#### Parameters

| Name   | Type                                      | Description          |
| ------ | ----------------------------------------- | -------------------- |
| `path` | [`Str`](../std/str.md)\|[`Path`](path.md) | Path to canonicalize |

#### Returns

[`Path`](path.md) - Canonical path

#### Example

```
let abs = canonical "./foo/../bar"
echo $abs # /current/working/dir/bar (with symlinks resolved)

```

### `copy from to :all?`

Copies a filesystem entry from one location to another.

By default this copies a single file or symlink. With `all: true`, it also
copies directories recursively.

#### Parameters

| Name   | Type                                      | Description                                |
| ------ | ----------------------------------------- | ------------------------------------------ |
| `from` | [`Str`](../std/str.md)\|[`Path`](path.md) | Source path                                |
| `to`   | [`Str`](../std/str.md)\|[`Path`](path.md) | Destination path                           |
| `all`  | [`Bool`](../std/bool.md)                  | If `true`, allows recursive directory copy |

#### Example

```
copy "source.txt" "backup.txt"
copy "project" "project-backup" all: true
```

### `copy_data src dst :range? :size? :offset? :clone?`

Copies data from one open file to another, returning the number of bytes copied.

This attempts to copy data in the most efficient manner possible:

- If both handles are from the same VFS domain, all copying occurs target-side
  rather than being relayed.
- Copy-on-write cloning of blocks/extents will be used by default if the
  platform, filesystem, handle pair, and data size and alignment meet
  relevant requirements.
- Sparsity will be preserved if possible: blocks of all zero bytes which are
    not physically allocated in the source will not be allocated in the
    destination, so long as platform, filesystem, data size and alignment
    requirements permit it.

This operation is not remotely atomic. Concurrent modification of either
source or destination in the relevant range will result in unspecified final
state in terms of the destination's content in the affect range, and possibly
file length if the range overlapped the prior end-of-file. A failed operation
may also leave the destination in an unspecified intermediate state.

Copying between disjoint regions of one file is allowed; overlapping ones are
rejected if detected. Detection requires recognizing when two handles refer to
the same file, which is not always possible, in which case an overlapping copy
has an unspecified result.

#### Parameters

| Name     | Type                        | Description                                    |
| -------- | --------------------------- | ---------------------------------------------- |
| `src`    | [`File`](file.md)           | Handle to read from                            |
| `dst`    | [`File`](file.md)           | Handle to write to                             |
| `range`  | [`Range`](../std/range.md)? | Absolute byte region of the source             |
| `size`   | [`Int`](../std/index.md)?   | Number of bytes to take from the source cursor |
| `offset` | [`Int`](../std/index.md)?   | Absolute byte offset in the destination        |
| `clone`  | [`Sym`](../std/sym.md)?     | `:AUTO:` (default), `:REQUIRE:`, or `:NEVER:`  |

##### Source Addressing

| `range:` | `size:` | Source                                        |
| -------- | ------- | --------------------------------------------- |
| given    | —       | that absolute region                          |
| —        | given   | that many bytes from the cursor               |
| —        | —       | the cursor to the end of the file             |
| given    | given   | an error — a range already carries its length |

`range:` follows the same conventions as
[`lock`](file.md#lock-range-shared-func): half-open, `..` is the whole file, an
open end means "to the end of the file", an omitted start is `0`, `step` must
be `1`, and endpoints counted from the end are rejected.

Using cursor-based source addressing advances the cursor by the amount copied
on success.

##### Destination Addressing

`offset:` writes at an absolute position and leaves the destination cursor
where it was; without it the copy lands at the cursor, which advances. A handle
opened for appending does not supported a specified offset.

As expected, if the destination position would write past the current
end-of-file, the length of the file is extended.

##### Clone Behavior

`clone:` allows specifying how blocks should be shared between the source and
destination.

| Value       | Meaning                                                      |
| ----------- | ------------------------------------------------------------ |
| `:AUTO:`    | share blocks where possible, copy the data otherwise         |
| `:REQUIRE:` | fail rather than copy the data outright                      |
| `:NEVER:`   | copy the data outright even where sharing is available       |

On Linux and FreeBSD, `:AUTO:` opportunistically performs copy-on-write cloning
for local filesystems and server-side copying for network mounts. `:REQUIRE:`
explicitly requests a copy-on-write clone and fails if not possible. On
Windows, `:AUTO:` opportunistically performs ReFS extent duplication or SMB
server-side copy when possible, while `:REQUIRE:` only attempts the ReFS path
(which may also work remotely). `:REQUIRE:` fails with
[`UnsupportedError`](../sys/unsupported-error.md) when the filesystem, file
pair, or range cannot be cloned. Append destinations cannot guarantee cloning,
so they also reject `:REQUIRE:`. macOS does not support range clones, nor
do copies between different VFS domains, and thus `:REQUIRE:` always fails.

##### Sparsity

Positional copies on Linux, FreeBSD, Windows, and macOS targets preserve source
holes (unallocated data blocks containing only logical zero bytes) when the
filesystem exposes them, and replace existing destination data in those holes
with zeroes. Hole deallocation is best effort: when it is unavailable the
zeroes may consume physical storage. On Windows, a file must be explicity
marked sparse to permit unallocated zero blocks; this operation will not do so
automatically.

#### Returns

[`Int`](../std/index.md) (number of bytes copied)

A successful result will be the number of bytes requested unless source
end-of-file was reached.

#### Errors

| Exception                                            | Condition                                           |
| ---------------------------------------------------- | --------------------------------------------------- |
| [`ValueError`](../std/value-error.md)                | `range:` and `size:` together, or a malformed range |
| [`StateError`](../std/state-error.md)                | `offset:` on a handle opened for appending          |
| [`StateError`](../std/state-error.md)                | either handle is closed                             |
| [`StateError`](../std/state-error.md)                | one handle used as both sides through its cursor    |
| [`InvalidInputError`](../sys/invalid-input-error.md) | the two regions overlap within one file             |

#### Example

```
open source.bin rb do |src|
  open dest.bin wb do |dst|
    # Splice a fixed region of the source onto wherever dst happens to be
    copy_data $src $dst range: (0..4096)

    # Copy the rest of src, advancing both cursors
    let count = copy_data $src $dst
    echo "copied $count bytes"

open archive.bin r+b do |file|
  # Both sides positional, so one handle can serve as both
  copy_data $file $file range: (0..64) offset: 8192
```

### `create_dir path :all?`

Creates a directory at the given path.

#### Parameters

| Name   | Type                                      | Description                               |
| ------ | ----------------------------------------- | ----------------------------------------- |
| `path` | [`Str`](../std/str.md)\|[`Path`](path.md) | Path to the directory to create           |
| `all`  | [`Bool`](../std/bool.md)                  | If `true`, creates parent directories too |

#### Example

```
# Create a single directory
create_dir new_dir

# Create directory and all parents
create_dir a/b/c all: true
```

### `create_temp_dir :parent?`

Creates a temporary directory and returns its path. Unlike
[`with_temp_dir`](#with_temp_dir-func), it does not remove the directory --
the caller owns its lifetime.

#### Parameters

| Name     | Type  | Description                                             |
| -------- | ----- | ------------------------------------------------------- |
| `parent` | path? | Parent directory; defaults to [`temp_dir()`](#temp_dir) |

#### Returns

[`Path`](path.md)

#### Example

```
let dir = create_temp_dir()
try
  let file = (dir / "test.txt")
  file.open w do |f|
    f.write "Hello, World!"
finally
  remove_dir $dir all: true
```

### `entries path`

Reads the entries in a directory.

#### Parameters

| Name   | Type                                      | Description           |
| ------ | ----------------------------------------- | --------------------- |
| `path` | [`Str`](../std/str.md)\|[`Path`](path.md) | Path to the directory |

#### Returns

Iterable of [`DirEntry`](direntry.md) objects

#### Example

```
# Iterate over directory entries
for entry = entries /home/user/docs
  echo "$(entry.name) - $(entry.type)"

# Collect into an array
let files = [...entries "."]
echo "Found $(files.len) entries"
```

### `exists path`

Checks whether a file or directory exists at the given path.

#### Parameters

| Name   | Type                                      | Description                 |
| ------ | ----------------------------------------- | --------------------------- |
| `path` | [`Str`](../std/str.md)\|[`Path`](path.md) | Path to check for existence |

#### Returns

[`Bool`](../std/bool.md) - `true` if the path
exists, `false` otherwise

#### Example

```
# Check before removing
if exists "temp.txt"
  remove "temp.txt"
  echo "Removed temp.txt"
else
  echo "temp.txt does not exist"

# Conditional file operations
if exists "config.yaml"
  echo "Found config file"
```

### `fs_metadata path :resolve?`

Gets filesystem metadata for the filesystem containing the given path.

#### Parameters

| Name      | Type                                      | Description                                      |
| --------- | ----------------------------------------- | ------------------------------------------------ |
| `path`    | [`Str`](../std/str.md)\|[`Path`](path.md) | Path to resolve                                  |
| `resolve` | `:TARGET:`\|`:LINK:`                      | Resolution mode (see [above](#resolution-modes)) |

#### Returns

[`FsMetadata`](fs-metadata.md)

#### Errors

| Exception              | Condition                           |
| ---------------------- | ----------------------------------- |
| `sys.UnsupportedError` | On Linux, `resolve: :LINK:` is used |

#### Example

```
let meta = fs_metadata "data.txt"
echo "Capacity: $(meta.capacity)"
echo "Available: $(meta.available)"
echo "Readonly: $(meta.read_only)"
```

### `glob pattern :max_depth? :resolve?`

Returns an iterator over paths matching a glob pattern.

#### Parameters

| Name        | Type                   | Description                                                          |
| ----------- | ---------------------- | -------------------------------------------------------------------- |
| `pattern`   | `Str`                  | Glob pattern (e.g., `"*.txt"`, `"**/*.rs"`)                          |
| `max_depth` | [`Int`](../std/int.md) | Maximum directory depth to traverse (default: unlimited)             |
| `resolve`   | `:TARGET:`\|`:LINK:`   | Resolution mode (see [above](#resolution-modes)) (default: `:LINK:`) |

##### Glob pattern syntax

- `*` - Match any sequence of characters except path separator
- `?` - Match a single character
- `**` - Match any sequence of characters including path separators (recursive)
- `[abc]` - Match any character in the set
- `{a,b,c}` - Match any of the comma-separated patterns

#### Returns

`Iter` of [`Path`](path.md) objects

#### Example

```
# Find all text files
for path = glob "*.txt"
  echo "Found: $path"

# Recursive search with depth limit
for path = glob "**/*.rs" max_depth: 3
  echo "Source: $path"

# Follow symlinks
for path = glob "**/*" resolve: :TARGET:
  echo "Entry: $path"
```

### `hard_link src dst`

Creates a hard link at `dst` pointing to the existing file at `src`.

This uses the platform-native hard-link operation. The source must already
exist, and the link must be created on the same filesystem or volume if the
platform requires it.

#### Parameters

| Name  | Type                                      | Description                         |
| ----- | ----------------------------------------- | ----------------------------------- |
| `src` | [`Str`](../std/str.md)\|[`Path`](path.md) | Existing file to link to            |
| `dst` | [`Str`](../std/str.md)\|[`Path`](path.md) | Path where the hard link is created |

#### Example

```
hard_link "data.txt" "data-copy.txt"
```

### `home_dir()`

Returns the current user's home directory as a [`Path`](path.md).

#### Platform behavior

| Platform | Result                                                         |
| -------- | -------------------------------------------------------------- |
| Unix     | `env["HOME"]`, or home directory from passwd database if unset |
| Windows  | `FOLDERID_Profile`, typically `C:\Users\<user>`                |

#### Returns

[`Path`](path.md)

### `is_absolute path`

Checks whether a path is absolute.

#### Parameters

| Name   | Type                                      | Description   |
| ------ | ----------------------------------------- | ------------- |
| `path` | [`Str`](../std/str.md)\|[`Path`](path.md) | Path to check |

#### Returns

[`Bool`](../std/bool.md) - `true` if the path is absolute,
`false` if relative

#### Example

```
# Check different paths
if is_absolute "/etc/passwd"
  echo "Absolute path"

if !is_absolute "./config.txt"
  echo "Relative path"
```

### `metadata path :resolve?`

Gets file metadata for the given path.

#### Parameters

| Name      | Type                                      | Description                                      |
| --------- | ----------------------------------------- | ------------------------------------------------ |
| `path`    | [`Str`](../std/str.md)\|[`Path`](path.md) | Path to the file or directory                    |
| `resolve` | `:TARGET:`\|`:LINK:`                      | Resolution mode (see [above](#resolution-modes)) |

#### Returns

[`Metadata`](metadata.md)

#### Example

```
let meta = metadata "data.txt"
echo "Size: $(meta.size)"
echo "Type: $(meta.type)"

if (sys.os_info().family != :WINDOWS:)
  echo "Mode: $(meta.mode)"
else
  echo "Attributes: $(meta.win_attrs)"

# Get symlink metadata without following
let link_meta = metadata "link.txt" resolve: :LINK:
echo "Link type: $(link_meta.type)"
```

### `move from to :all?`

Moves a filesystem entry from one location to another.

This first tries a plain rename. If that fails because the source and
destination are on different filesystems, it falls back to copy-and-delete.
By default this moves a single file or symlink. With `all: true`, it also
moves directories recursively.

#### Parameters

| Name   | Type                                      | Description                                |
| ------ | ----------------------------------------- | ------------------------------------------ |
| `from` | [`Str`](../std/str.md)\|[`Path`](path.md) | Source path                                |
| `to`   | [`Str`](../std/str.md)\|[`Path`](path.md) | Destination path                           |
| `all`  | [`Bool`](../std/bool.md)                  | If `true`, allows recursive directory move |

#### Example

```
move "source.txt" "dest.txt"
move "project" "archive/project" all: true
```

### `normalize path`

Returns a normalized path with `.` and `..` components resolved without
accessing the filesystem.

Unresolvable `..` components in relative paths are preserved.

#### Parameters

| Name   | Type                                      | Description       |
| ------ | ----------------------------------------- | ----------------- |
| `path` | [`Str`](../std/str.md)\|[`Path`](path.md) | Path to normalize |

#### Returns

[`Path`](path.md) - Normalized path

#### Example

```
# Remove redundant components
let clean = normalize "./foo/../bar/./baz"
echo $clean  # bar/baz

# Works with Path objects too
let path = Path "a/b/../c"
let norm = normalize $path
echo $norm  # a/c
```

### `open path mode? func?`

Opens a file and returns a File object.

#### Parameters

| Name   | Type                   | Description                                          |
| ------ | ---------------------- | ---------------------------------------------------- |
| `path` | [`Str`](../std/str.md) | Path to the file to open                             |
| `mode` | `Str`                  | File access mode (default: `"r"`)                    |
| `func` | `Func`                 | Function to run with the file; auto-closes when done |

##### File modes

| Mode   | Description                              |
| ------ | ---------------------------------------- |
| `"r"`  | Read-only                                |
| `"w"`  | Write-only (truncates existing file)     |
| `"a"`  | Append to existing file                  |
| `"r+"` | Read and write                           |
| `"w+"` | Read and write (truncates existing file) |
| `"a+"` | Read and append                          |

Add `"b"` suffix for binary mode (e.g., `"rb"`, `"wb"`, `"r+b"`).

#### Returns

File

#### Example

``` 
# Read a file (auto-closed when block finishes)
open config.txt r do |file|
  let content = file.read()
  echo "Content: $content"

# Write with automatic cleanup
open output.txt w do |file|
  file.write "Hello, World!"

# Manual file management
let file = open data.txt w
file.write "some data"
file.close()
```

### `read path mode?`

Reads the entire contents of a file in one call.

By default, returns text as a [`Str`](../std/str.md). If `mode` is
`"b"`, returns raw bytes as [`Bin`](../std/bin.md).

#### Parameters

| Name   | Type                                      | Description                                 |
| ------ | ----------------------------------------- | ------------------------------------------- |
| `path` | [`Str`](../std/str.md)\|[`Path`](path.md) | Path to the file to read                    |
| `mode` | `Str`                                     | Optional mode string; only `"b"` is allowed |

#### Returns

[`Str`](../std/str.md)\|[`Bin`](../std/bin.md)

#### Example

```
let text = read "config.txt"
let data = read "archive.bin" "b"
```

### `read_link path`

Reads the target of a symbolic link.

#### Parameters

| Name   | Type                                      | Description         |
| ------ | ----------------------------------------- | ------------------- |
| `path` | [`Str`](../std/str.md)\|[`Path`](path.md) | Path to the symlink |

#### Returns

[`Path`](path.md) - The path that the symlink points to

#### Errors

| Exception                   | Condition                                          |
| --------------------------- | -------------------------------------------------- |
| `sys.NotFoundError`         | The path does not exist                            |
| `sys.PermissionDeniedError` | Permission denied to read the symlink              |
| `sys.UnsupportedError`      | Reading symlinks is not supported on this platform |
| `sys.Error`                 | Other I/O errors                                   |

#### Example

```
let link = read_link "./my_link"
echo "Link points to: $link"
```

### `relative path base?`

Returns the path relative to a base directory.

#### Parameters

| Name   | Type                                      | Description                   |
| ------ | ----------------------------------------- | ----------------------------- |
| `path` | [`Str`](../std/str.md)\|[`Path`](path.md) | Path to make relative         |
| `base` | [`Str`](../std/str.md)\|[`Path`](path.md) | Base directory (default: cwd) |

#### Returns

[`Path`](path.md) - Relative path, or the original path if it
cannot be made relative

#### Example

```
# Relative to current directory
let rel = relative "/home/user/docs/file.txt"
echo $rel  # docs/file.txt (if cwd is /home/user)

# Relative to specific base
let rel2 = relative "/a/b/c/d" "/a/b"
echo $rel2  # c/d

# Returns original if no common prefix
let unchanged = relative "/etc/passwd" "/home/user"
echo $unchanged  # /etc/passwd
```

### `remove path... :all? :ignore?`

Removes one or more paths from the filesystem.

By default this removes a single file or symlink. With `all: true`, it also
removes directories recursively, similar to `rm -r`. With `ignore: true`,
missing paths are treated as success.

#### Parameters

| Name     | Type                                      | Description                                |
| -------- | ----------------------------------------- | ------------------------------------------ |
| `path`   | [`Str`](../std/str.md)\|[`Path`](path.md) | One or more paths to remove                |
| `all`    | [`Bool`](../std/bool.md)                  | If `true`, removes directories recursively |
| `ignore` | [`Bool`](../std/bool.md)                  | If `true`, ignores a missing path          |

#### Example

```
write "temp.txt" "temporary data"
remove "temp.txt"

remove "missing.txt" ignore: true
remove "build" all: true
remove "a.txt" "b.txt"
```

### `remove_dir path... :all? :ignore?`

Removes one or more directories.

By default this removes only empty directories. With `all: true`, it removes
directories recursively, but only through subtrees that contain directories and
no files or other non-directory entries. Use
[`remove`](index.md#remove-path-all-ignore) to delete directories that contain
files.

#### Parameters

| Name     | Type                                      | Description                                                      |
| -------- | ----------------------------------------- | ---------------------------------------------------------------- |
| `path`   | [`Str`](../std/str.md)\|[`Path`](path.md) | One or more directories to remove                                |
| `all`    | [`Bool`](../std/bool.md)                  | If `true`, recursively prunes only empty directory subtrees      |
| `ignore` | [`Bool`](../std/bool.md)                  | If `true`, ignores missing directories and file-blocked subtrees |

#### Example

```
# Remove an empty directory
remove_dir empty_dir

# Remove an empty directory tree
remove_dir dir_to_remove all: true

# Prune only the empty branches and ignore file-blocked subtrees
remove_dir cache tmp all: true ignore: true
```

### `remove_xattr path name :namespace? :resolve?`

Removes an extended attribute.

#### Parameters

| Name        | Type                                                   | Description                                      |
| ----------- | ------------------------------------------------------ | ------------------------------------------------ |
| `path`      | [`Str`](../std/str.md)\|[`Path`](path.md)              | Path to update                                   |
| `name`      | [`Str`](../std/str.md)\|[`XattrEntry`](xattr-entry.md) | Attribute name or entry from `xattrs`            |
| `namespace` | [`Str`](../std/str.md)\|[`Sym`](../std/sym.md)?        | Namespace to update                              |
| `resolve`   | `:TARGET:`\|`:LINK:`                                   | Resolution mode (see [above](#resolution-modes)) |

#### Example

```
remove_xattr "data.txt" "comment"
```

### `rename from to :replace?`

Renames (moves) a file or directory.

By default, this replaces an existing destination. Set `replace` to `false`
to fail atomically instead.

!!! note
    `replace: false` is not supported on FreeBSD.

#### Parameters

| Name      | Type                                      | Description                                  |
| --------- | ----------------------------------------- | -------------------------------------------- |
| `from`    | [`Str`](../std/str.md)\|[`Path`](path.md) | Source path                                  |
| `to`      | [`Str`](../std/str.md)\|[`Path`](path.md) | Destination path                             |
| `replace` | [`Bool`](../std/bool.md)?                 | Whether to replace an existing destination   |

#### Example

```
rename "old_name.txt" "new_name.txt"

# Move to different directory
rename "file.txt" "subdir/file.txt"

# Fail if the destination exists
rename "draft.txt" "published.txt" replace: false
```

### `set_acl path acl :kind? :default? :resolve?`

Sets or removes an ACL. A built ACL supplies its format; an explicit `kind:`
must match it. An untyped iterable of declarative ACE dictionaries requires
an explicit `kind:` and is coerced by that ACL family. With `nil`, `kind:`
selects the format to remove and defaults to `:POSIX:`.

#### Parameters

| Name      | Type                                        | Description                                      |
| --------- | ------------------------------------------- | ------------------------------------------------ |
| `path`    | [`Str`](../std/str.md)\|[`Path`](path.md)   | Path to update                                   |
| `acl`     | built ACL\|iterable\|[`nil`](../std/nil.md) | ACL or declarative ACE sequence                  |
| `kind`    | `:POSIX:`\|`:NFS4:`\|`:MACOS:`?             | Required for an untyped ACL specification        |
| `default` | [`Bool`](../std/bool.md)                    | Update the directory's inheritable default ACL   |
| `resolve` | `:TARGET:`\|`:LINK:`                        | Resolution mode (see [above](#resolution-modes)) |

#### Errors

| Exception              | Condition                                                                                                        |
| ---------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `ValueError`           | An NFSv4/macOS ACL is combined with `default: true`                                                              |
| `ValueError`           | A built ACL conflicts with the explicit `kind:`                                                                  |
| `TypeError`            | An untyped ACL specification is passed without `kind:`                                                           |
| `sys.UnsupportedError` | An NFSv4 ACL is removed with `kind: :NFS4:` and `acl: nil`; NFSv4 ACLs can be replaced but not cleared to "none" |

POSIX ACLs are supported on Linux and FreeBSD, NFSv4 ACLs on FreeBSD, and
macOS ACLs on macOS. Other target and format combinations raise
`sys.UnsupportedError`.

### `set_metadata :resolve? ...paths ...`

Updates timestamps, permissions, ownership, and filesystem attributes.

Unspecified metadata is left unchanged. Unix targets support `mode`, numeric or
named `user` and `group` values, and applicable filesystem attributes. Windows
targets accept an account name or [`Sid`](../security/windows/sid.md) for `user`
and `group` and support applicable filesystem attributes.
Unix supports `modified` and `accessed` timestamps; Windows also supports
`created`.
Paths are submitted from left to right and processing stops at the first error.
Within each path, ownership, mode, attributes, and timestamps are applied in
that order.
Backends may use multiple system operations; atomicity and rollback behavior
are unspecified.

Clearing `sparse` on Windows may allocate storage for every hole. It can be
expensive, may fail when the volume lacks space, and is not transactional.

#### Parameters

| Name                  | Type                                                                                | Description                                      |
| --------------------- | ----------------------------------------------------------------------------------- | ------------------------------------------------ |
| `paths`               | ([`Str`](../std/str.md)\|[`Path`](path.md))*                                        | Paths to update in order                         |
| `mode`                | [`Int`](../std/int.md)                                                              | Optional Unix permission mode                    |
| `user`                | [`Int`](../std/int.md)\|[`Str`](../std/str.md)\|[`Sid`](../security/windows/sid.md) | Optional owner ID, name, or SID                  |
| `group`               | [`Int`](../std/int.md)\|[`Str`](../std/str.md)\|[`Sid`](../security/windows/sid.md) | Optional group ID, name, or SID                  |
| `modified`            | [`DateTime`](../time/datetime.md)                                                   | Optional new modification time                   |
| `accessed`            | [`DateTime`](../time/datetime.md)                                                   | Optional new access time                         |
| `created`             | [`DateTime`](../time/datetime.md)                                                   | Optional new creation time (Windows only)        |
| `resolve`             | `:TARGET:`\|`:LINK:`                                                                | Resolution mode (see [above](#resolution-modes)) |
| `readonly`            | [`Bool`](../std/bool.md)                                                            | Optional readonly attribute value                |
| `hidden`              | [`Bool`](../std/bool.md)                                                            | Optional hidden attribute/flag                   |
| `system`              | [`Bool`](../std/bool.md)                                                            | Optional system attribute value                  |
| `archive`             | [`Bool`](../std/bool.md)                                                            | Optional archive attribute value                 |
| `compressed`          | [`Bool`](../std/bool.md)                                                            | Optional compressed flag                         |
| `sparse`              | [`Bool`](../std/bool.md)                                                            | Optional Windows sparse attribute                |
| `temporary`           | [`Bool`](../std/bool.md)                                                            | Optional temporary value                         |
| `offline`             | [`Bool`](../std/bool.md)                                                            | Optional offline value                           |
| `not_content_indexed` | [`Bool`](../std/bool.md)                                                            | Optional indexing attribute value                |
| `immutable`           | [`Bool`](../std/bool.md)                                                            | Optional immutable flag                          |
| `append_only`         | [`Bool`](../std/bool.md)                                                            | Optional append-only flag                        |
| `no_dump`             | [`Bool`](../std/bool.md)                                                            | Optional no-dump flag                            |
| `no_atime`            | [`Bool`](../std/bool.md)                                                            | Optional Linux no-atime flag                     |
| `no_copy_on_write`    | [`Bool`](../std/bool.md)                                                            | Optional Linux no-COW flag                       |
| `dir_sync`            | [`Bool`](../std/bool.md)                                                            | Optional Linux dir-sync flag                     |
| `casefold`            | [`Bool`](../std/bool.md)                                                            | Optional Linux casefold flag                     |
| `data_journaling`     | [`Bool`](../std/bool.md)                                                            | Optional Linux journaling flag                   |
| `no_compress`         | [`Bool`](../std/bool.md)                                                            | Optional Linux no-compress flag                  |
| `project_inherit`     | [`Bool`](../std/bool.md)                                                            | Optional Linux project flag                      |
| `secure_delete`       | [`Bool`](../std/bool.md)                                                            | Optional Linux secure-delete flag                |
| `sync`                | [`Bool`](../std/bool.md)                                                            | Optional Linux sync flag                         |
| `no_tail_merge`       | [`Bool`](../std/bool.md)                                                            | Optional Linux no-tail flag                      |
| `top_dir`             | [`Bool`](../std/bool.md)                                                            | Optional Linux top-dir flag                      |
| `undelete`            | [`Bool`](../std/bool.md)                                                            | Optional Linux undelete flag                     |
| `direct_access`       | [`Bool`](../std/bool.md)                                                            | Optional Linux direct-access flag                |
| `extent_format`       | [`Bool`](../std/bool.md)                                                            | Optional Linux extent flag                       |
| `opaque`              | [`Bool`](../std/bool.md)                                                            | Optional macOS opaque flag                       |

#### Errors

| Exception              | Condition                                        |
| ---------------------- | ------------------------------------------------ |
| `sys.UnsupportedError` | The operation is used on an unsupported platform |

#### Example

```
set_metadata "script.sh" mode: 0o755 user: "deploy" group: "deploy"
set_metadata "one.txt" "two.txt" mode: 0o640
set_metadata "data.txt" hidden: true
set_metadata "data.txt" no_dump: true
set_metadata "link" group: "www-data" resolve: :LINK:
set_metadata "artifact.tar" modified: $DateTime.from_unix(1700000000)
set_metadata "cache.db" accessed: $DateTime.now()
```

### `set_size path size`

Truncates the file at the given path to the specified byte length, creating it
if needed.

#### Parameters

| Name   | Type                                      | Description              |
| ------ | ----------------------------------------- | ------------------------ |
| `path` | [`Str`](../std/str.md)\|[`Path`](path.md) | Path to the file         |
| `size` | [`Int`](../std/index.md)                  | New file length in bytes |

#### Example

```
set_size "output.txt" 0
set_size (Path "archive.bin") 1024
```

### `set_xattr path name value :namespace? :resolve?`

Sets an extended attribute value.

On Windows, empty values are rejected. NTFS deletes the attribute instead of
storing an empty value.

#### Parameters

| Name        | Type                                                   | Description                                      |
| ----------- | ------------------------------------------------------ | ------------------------------------------------ |
| `path`      | [`Str`](../std/str.md)\|[`Path`](path.md)              | Path to update                                   |
| `name`      | [`Str`](../std/str.md)\|[`XattrEntry`](xattr-entry.md) | Attribute name or entry from `xattrs`            |
| `value`     | [`Str`](../std/str.md)\|[`Bin`](../std/bin.md)         | Attribute bytes; strings use UTF-8               |
| `namespace` | [`Str`](../std/str.md)\|[`Sym`](../std/sym.md)?        | Namespace to update                              |
| `resolve`   | `:TARGET:`\|`:LINK:`                                   | Resolution mode (see [above](#resolution-modes)) |

#### Example

```
set_xattr "data.txt" "comment" "ready"
set_xattr "data.txt" "raw" b"\x00\x01"
```

### `streams path :resolve?`

Lists alternate data streams for the given path.

This is only supported on Windows.

#### Parameters

| Name      | Type                                      | Description                                      |
| --------- | ----------------------------------------- | ------------------------------------------------ |
| `path`    | [`Str`](../std/str.md)\|[`Path`](path.md) | Path to query                                    |
| `resolve` | `:TARGET:`\|`:LINK:`                      | Resolution mode (see [above](#resolution-modes)) |

#### Returns

`Iter` of [`fs.windows.StreamEntry`](windows/stream-entry.md)

#### Example

```
let path = Path data.txt
open $path r do |file|
  for stream = file.streams()
    echo "$(stream.name) $(stream.type)"
    echo (path / stream)
```

### `symlink src dst`

Creates a symbolic link at `dst` pointing to `src`.

#### Platform Notes

- **Unix:** Creates a standard symbolic link
- **Windows:** Attempts to determine if the target is a file or directory by
  reading its metadata. If the target cannot be accessed, the operation fails.
  For explicit control, use `symlink_file` or `symlink_dir`.

#### Parameters

| Name  | Type                                      | Description                       |
| ----- | ----------------------------------------- | --------------------------------- |
| `src` | [`Str`](../std/str.md)\|[`Path`](path.md) | Target path the symlink points to |
| `dst` | [`Str`](../std/str.md)\|[`Path`](path.md) | Path where the symlink is created |

#### Errors

| Exception           | Condition                                |
| ------------------- | ---------------------------------------- |
| `sys.NotFoundError` | The target cannot be accessed on Windows |

#### Example

```
symlink "/path/to/target" "link_name"
```

### `symlink_dir src dst`

Creates a directory symbolic link at `dst` pointing to `src`.

#### Platform Notes

- **Unix:** Equivalent to `symlink`
- **Windows:** Creates a directory symlink (requires appropriate permissions on
  some Windows versions)

#### Parameters

| Name  | Type                                      | Description                       |
| ----- | ----------------------------------------- | --------------------------------- |
| `src` | [`Str`](../std/str.md)\|[`Path`](path.md) | Target directory path             |
| `dst` | [`Str`](../std/str.md)\|[`Path`](path.md) | Path where the symlink is created |

#### Example

```
symlink_dir "/path/to/dir" "dir_link"
```

### `symlink_file src dst`

Creates a file symbolic link at `dst` pointing to `src`.

#### Platform Notes

- **Unix:** Equivalent to `symlink`
- **Windows:** Creates a file symlink (may require appropriate permissions on
  some Windows versions)

#### Parameters

| Name  | Type                                      | Description                       |
| ----- | ----------------------------------------- | --------------------------------- |
| `src` | [`Str`](../std/str.md)\|[`Path`](path.md) | Target file path                  |
| `dst` | [`Str`](../std/str.md)\|[`Path`](path.md) | Path where the symlink is created |

#### Example

```
symlink_file "/path/to/file" "file_link"
```

### `sync path :data?`

Flushes the file at the given path to durable storage, returning once the
device reports it committed.

The file must already exist — unlike [`set_size`](#set_size-path-size), this
does not create it.

Flushing the contents says nothing about the directory entry naming the file,
which is a separate inode with its own flush. Nor is it a substitute for the
guarantee on a filesystem with delayed allocation or write cancellation, where
data written to a file that is removed before it is flushed may never be
written at all.

#### Parameters

| Name   | Type                                      | Description                                 |
| ------ | ----------------------------------------- | ------------------------------------------- |
| `path` | [`Str`](../std/str.md)\|[`Path`](path.md) | Path to the file                            |
| `data` | [`Bool`](../std/index.md)?                | Flush data only, skipping unneeded metadata |

##### `data:`

A data-only flush (`fdatasync`) omits metadata a reader does not need to find
the contents — notably the modification time — and so can avoid a second write
to the inode. A size change is still flushed either way. Defaults to `false`.

#### Example

```
write scratch.bin $payload
sync scratch.bin
```

### `temp_dir()`

Returns the platform-native directory for temporary files as a
[`Path`](path.md).

#### Platform behavior

| Platform | Result                                                      |
| -------- | ----------------------------------------------------------- |
| Unix     | `$TMPDIR`, otherwise `/tmp`                                 |
| Windows  | `%TMP%`, otherwise `%TEMP%`, otherwise the platform default |

#### Returns

[`Path`](path.md)

### `with_temp_dir func`

Creates a temporary directory, invokes a function with the directory path, then
removes the directory recursively upon return or error.

#### Parameters

| Name     | Type   | Description                                             |
| -------- | ------ | ------------------------------------------------------- |
| `func`   | `Func` | Called with a [`Path`](path.md) to the temp dir         |
| `parent` | path?  | Parent directory; defaults to [`temp_dir()`](#temp_dir) |

#### Example

```
# Use the temporary directory in the default location
with_temp_dir do |dir|
  let file = (dir / "test.txt")
  file.open w do |f|
    f.write "Hello, World!"
  echo "Wrote to: $file"

# Directory is automatically cleaned up

# Use a custom parent directory
with_temp_dir parent: my_temp do |dir|
  # ...
```

### `write path content`

Writes the entire contents of a file in one call, creating or truncating the
file.

Binary values are written as raw bytes and strings as UTF-8 text.

#### Parameters

| Name      | Type                                      | Description               |
| --------- | ----------------------------------------- | ------------------------- |
| `path`    | [`Str`](../std/str.md)\|[`Path`](path.md) | Path to the file to write |
| `content` | `Str`\|`Bin`                              | Value to write            |

#### Returns

[`Int`](../std/int.md) - Number of bytes written

#### Example

```
write "message.txt" "hello"
write "data.bin" b"\x01\x02\x03"
```

### `xattr path name :namespace? :resolve?`

Gets an extended attribute value.

#### Parameters

| Name        | Type                                                   | Description                                      |
| ----------- | ------------------------------------------------------ | ------------------------------------------------ |
| `path`      | [`Str`](../std/str.md)\|[`Path`](path.md)              | Path to query                                    |
| `name`      | [`Str`](../std/str.md)\|[`XattrEntry`](xattr-entry.md) | Attribute name or entry from `xattrs`            |
| `namespace` | [`Str`](../std/str.md)\|[`Sym`](../std/sym.md)?        | Namespace to query                               |
| `resolve`   | `:TARGET:`\|`:LINK:`                                   | Resolution mode (see [above](#resolution-modes)) |

#### Returns

[`Bin`](../std/bin.md)

#### Example

```
let value = xattr "data.txt" "comment"
```

### `xattrs path :namespace? :resolve?`

Lists extended attributes for the given path.

On Windows, this uses NTFS extended attributes. Returned names may differ in
case from the requested name.

#### Parameters

| Name        | Type                                            | Description                                                                                              |
| ----------- | ----------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `path`      | [`Str`](../std/str.md)\|[`Path`](path.md)       | Path to query                                                                                            |
| `namespace` | [`Str`](../std/str.md)\|[`Sym`](../std/sym.md)? | Namespace to query; `:USER:` and `:SYSTEM:` name well-known namespaces, and `:ANY:` lists all namespaces |
| `resolve`   | `:TARGET:`\|`:LINK:`                            | Resolution mode (see [above](#resolution-modes))                                                         |

#### Returns

iterator of [`XattrEntry`](xattr-entry.md)

#### Example

```
for attr = xattrs "data.txt"
  echo $attr.name
```
