# File

File objects are returned by [`open`](./index.md#open-path-mode-func) and
provide methods for file operations. All operations on closed files raise a
runtime error.

## Inherits

- [`Iter`](../std/iter.md)
- [`Sink`](../std/sink.md)

## Methods

### `write data :offset?`

Writes data to the file.

#### Parameters

| Name     | Type                     | Description                                                                |
| -------- | ------------------------ | -------------------------------------------------------------------------- |
| `data`   | `Str`\|`Bin`             | Data to write. Strings are written as UTF-8 text.                          |
| `offset` | [`Int`](../std/index.md) | Byte offset to write at. Without it, writes at the cursor and advances it. |

##### Writing at an offset

`offset:` writes at an absolute position and leaves the cursor where it was,
so it can be used alongside streaming writes on the same handle, and several
regions of a file can be written without seeking between them. Writing past the
end extends the file, zero-filling the gap.

As with [`read`](#read-size-offset), any number of positional writes may be in
flight on one handle at once.

All of `data` is written, however many transfers that takes.

#### Returns

[`Int`](../std/index.md) (number of bytes written)

#### Errors

| Exception                                  | Condition                                  |
| ------------------------------------------ | ------------------------------------------ |
| [`StateError`](../std/state-error.md)      | `offset:` on a file opened for appending   |

An append handle writes at the end of the file no matter what offset the
platform is given, so an explicit one cannot be honored rather than merely
being unimplemented.

#### Example

```
open output.txt w do |file|
  let bytes_written = file.write "Hello, World!"
  echo "Wrote $bytes_written bytes"

  # Write binary data
  let binary = b"Hello"
  file.write binary

# Patch a record in place without disturbing the cursor
open data.bin r+b do |file|
  file.write $record offset: (index * 64)
```

### `copy_data dst :range? :size? :offset? :clone?`

The method form of
[`fs.copy_data`](./index.md#copy_data-src-dst-range-size-offset-clone).

### `set_size size`

Truncates the file to the given byte length.

If the file has buffered unread data, the logical cursor position is preserved
after truncation.

#### Parameters

| Name   | Type                     | Description                 |
| ------ | ------------------------ | --------------------------- |
| `size` | [`Int`](../std/index.md) | New file length in bytes    |

#### Example

```
open data.bin r+ do |file|
  file.set_size 8
```

### `sync :data?`

Flushes the file to durable storage, returning once the device reports it
committed.

#### Parameters

| Name   | Type                       | Description                                 |
| ------ | -------------------------- | ------------------------------------------- |
| `data` | [`Bool`](../std/index.md)? | Flush data only, skipping unneeded metadata |

See [`fs.sync`](index.md#sync-path-data) for what `data:` selects and what a
flush does and does not guarantee.

#### Example

```
open journal.bin w do |file|
  file.write $entry
  file.sync()
```

### `lock range :shared? func`

Acquires a byte-range lock while `func` runs.

#### Parameters

| Name     | Type                       | Description                                    |
| -------- | -------------------------- | ---------------------------------------------- |
| `range`  | [`Range`](../std/range.md) | Half-open byte range; `..` is total            |
| `shared` | [`Bool`](../std/bool.md)?  | Acquire a shared lock                          |
| `func`   | [`Func`](../std/func.md)   | Block receiving a [`FileLock`](./file-lock.md) |

#### Returns

the block's result

The lock is exclusive unless `shared` is true. It is released before the
method returns, including when the block raises an error or is canceled.
Release runs with interruption masked and may wait indefinitely.

Locks are mandatory on Windows, where they prevent conflicting file access.
On Unix they are advisory and affect only programs that cooperate through file
locking.

Native blocking acquisition cannot be canceled. Canceling the strand may
leave a blocking worker waiting for a conflicting lock and delay shutdown.
Use `try_lock` with async retry for bounded or cancellable waiting.

Overlapping active lock ranges on the same `File` are unsupported, including
identical shared ranges.

Finite zero-length ranges are invalid on Unix. On Windows they conflict only
with positive-length ranges that start before and end after their offset. They
do not conflict with another zero-length range or a range starting at the same
offset. The zero-length range `0..0` is invalid.

#### Example

```
file.lock (0..128) do |lock|
  update_header()
```

### `try_lock range :shared? func`

Attempts to acquire a byte-range lock without waiting.

#### Parameters

| Name     | Type                       | Description                                    |
| -------- | -------------------------- | ---------------------------------------------- |
| `range`  | [`Range`](../std/range.md) | Half-open byte range; `..` is total            |
| `shared` | [`Bool`](../std/bool.md)?  | Acquire a shared lock                          |
| `func`   | [`Func`](../std/func.md)   | Block receiving a [`FileLock`](./file-lock.md) |

#### Returns

the block's result

The block always runs. `lock.held` is false when another handle holds a
conflicting lock. Other acquisition failures raise errors.

#### Example

```
file.try_lock (..) do |lock|
  if lock.held
    update_index()
```

### `read size? :offset?`

Reads data from the file.

#### Parameters

| Name     | Type                     | Description                                                                |
| -------- | ------------------------ | -------------------------------------------------------------------------- |
| `size`   | [`Int`](../std/index.md) | Number of bytes to read. If [`nil`](../std/index.md), reads to the end.    |
| `offset` | [`Int`](../std/index.md) | Byte offset to read from. Without it, reads at the cursor and advances it. |

##### Reading at an offset

`offset:` reads from an absolute position and leaves the cursor where it was,
so it can be used alongside streaming reads on the same handle, and several
regions of a file can be read without seeking between them.

Because a positional read touches nothing shared, any number of them may be in
flight on one handle at once — several strands can read different regions of
the same open file concurrently. Streaming reads still take the handle
exclusively, since they move the cursor.

`size` bytes are read however many transfers that takes; a shorter result means
the end of the file was reached, and reading entirely past the end gives an
empty result rather than an error.

In text mode the bytes read must be complete UTF-8. There is no cursor for a
positional read to carry a split character forward on, so unlike a streaming
read it cannot hold a partial character back for next time.

#### Returns

[`Str`](../std/str.md) in text mode, binary blob in binary
mode

#### Example

```
# Read entire file
open input.txt r do |file|
  let content = file.read()
  echo "File contents: $content"

# Read specific number of bytes
open data.bin rb do |file|
  let header = file.read 4
  let rest = file.read()

# Read a record without moving the cursor
open data.bin rb do |file|
  let record = file.read 64 offset: (index * 64)
```

### `metadata()`

Gets file metadata.

#### Returns

[`Metadata`](metadata.md)

##### Fields

| Field  | Type                     | Description                                                                                                        |
| ------ | ------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `len`  | [`Int`](../std/index.md) | File size in bytes                                                                                                 |
| `type` | [`Sym`](../std/sym.md)   | File type: `:FILE:`, `:DIR:`, `:SYMLINK:`, `:FIFO:`, `:CHAR_DEVICE:`, `:BLOCK_DEVICE:`, `:SOCKET:`, or `:UNKNOWN:` |

###### Optional timestamps

 (platform-dependent):

| Field      | Type                              | Description            |
| ---------- | --------------------------------- | ---------------------- |
| `modified` | [`DateTime`](../time/datetime.md) | Last modification time |
| `accessed` | [`DateTime`](../time/datetime.md) | Last access time       |
| `created`  | [`DateTime`](../time/datetime.md) | Creation/change time   |

###### Unix-only

 (these fields do not exist on Windows):

| Field     | Type                     | Description                           |
| --------- | ------------------------ | ------------------------------------- |
| `mode`    | [`Int`](../std/index.md) | File permissions and type (stat mode) |
| `dev`     | [`Int`](../std/index.md) | Device ID                             |
| `ino`     | [`Int`](../std/index.md) | Inode number                          |
| `nlink`   | [`Int`](../std/index.md) | Number of hard links                  |
| `uid`     | [`Int`](../std/index.md) | User ID of owner                      |
| `gid`     | [`Int`](../std/index.md) | Group ID of owner                     |
| `rdev`    | [`Int`](../std/index.md) | Device ID (if special file)           |
| `blksize` | [`Int`](../std/index.md) | Preferred block size for I/O          |
| `blocks`  | [`Int`](../std/index.md) | Number of 512-byte blocks allocated   |

###### Windows-only

 (these fields do not exist on Unix):

| Field       | Type                     | Description                           |
| ----------- | ------------------------ | ------------------------------------- |
| `win_attrs` | [`Int`](../std/index.md) | Raw Windows file attribute bitmask    |

#### Example

```
open data.txt r do |file|
  let meta = file.metadata()
  echo "Size: $(meta.size)"
  echo "Type: $(meta.type)"
  echo "Modified: $(meta.modified)"
  echo "Modified seconds: $(meta.modified.unix_secs)"

  if (sys.os_info().family != :WINDOWS:)
    echo "Mode: $(meta.mode)"
    echo "Owner: UID=$(meta.uid), GID=$(meta.gid)"
  else
    echo "Attributes: $(meta.win_attrs)"
```

### `fs_metadata()`

Gets filesystem metadata for the filesystem backing this open file.

#### Returns

[`FsMetadata`](fs-metadata.md)

#### Example

```
open data.txt r do |file|
  let meta = file.fs_metadata()
  echo "Capacity: $(meta.capacity)"
  echo "Available: $(meta.available)"
```

### `sec_desc :owner? :group? :dacl? :sacl?`

Gets selected parts of the Windows security descriptor through this file's
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

The operation raises a permission error if the file was opened without the
necessary Windows access rights. Other platforms raise `UnsupportedError`.

### `set_sec_desc desc? ...options`

Applies the components selected by a security descriptor's `mask` through
this file's existing handle.

#### Parameters

| Name   | Type                                                                                                            | Description                 |
| ------ | --------------------------------------------------------------------------------------------------------------- | --------------------------- |
| `desc` | [`security.windows.SecDesc`](../security/windows/secdesc.md)\|[`Bin`](../std/bin.md)\|[`Dict`](../std/dict.md)? | Descriptor, packet, or spec |

The descriptor's
[component options](../security/windows/secdesc.md#component-options) may be
passed as keyword arguments instead of, or alongside, `desc`, exactly as
[`sec_desc`](../security/windows/index.md#sec_desc-desc-options) accepts them.

The operation raises a permission error if the file was opened without the
necessary Windows access rights. Windows may normalize the resulting
descriptor. Other platforms raise `UnsupportedError`.

### `acl :kind = :POSIX: :default?`

Gets the ACL stored on the open file.

#### Parameters

| Name      | Type                           | Description                                   |
| --------- | ------------------------------ | --------------------------------------------- |
| `kind`    | `:POSIX:`\|`:NFS4:`\|`:MACOS:` | ACL format to query                           |
| `default` | [`Bool`](../std/bool.md)?      | Query the directory's inheritable default ACL |

#### Returns

The matching portable ACL type, depending on `kind`, or `nil`.

### `set_acl acl :kind? :default?`

Sets or removes an ACL on the open file. A built ACL supplies its format and
must match an explicit `kind:`. An untyped sequence of declarative ACE
dictionaries requires `kind:`. With `nil`, the omitted kind remains POSIX.

#### Parameters

| Name      | Type                                        | Description                                    |
| --------- | ------------------------------------------- | ---------------------------------------------- |
| `acl`     | built ACL\|iterable\|[`nil`](../std/nil.md) | ACL or declarative ACE sequence                |
| `kind`    | `:POSIX:`\|`:NFS4:`\|`:MACOS:`?             | Required for an untyped ACL specification      |
| `default` | [`Bool`](../std/bool.md)?                   | Update the directory's inheritable default ACL |

### `xattrs :namespace?`

Lists extended attributes for this file.

On Windows, this uses NTFS extended attributes. Returned names may differ in
case from the requested name.

#### Parameters

| Name        | Type                                            | Description                                                                                              |
| ----------- | ----------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `namespace` | [`Str`](../std/str.md)\|[`Sym`](../std/sym.md)? | Namespace to query; `:USER:` and `:SYSTEM:` name well-known namespaces, and `:ANY:` lists all namespaces |

#### Returns

iterator of [`XattrEntry`](xattr-entry.md)

#### Example

```
open data.txt r do |file|
  for attr = file.xattrs()
    echo $attr.name
```

### `streams`

Lists alternate data streams for this file.

This is only supported on Windows.

#### Returns

iterator of [`fs.windows.StreamEntry`](windows/stream-entry.md)

#### Example

```
let path = Path data.txt
open $path r do |file|
  for stream = file.streams()
    echo "$(stream.name) $(stream.type)"
    echo (path / stream)
```

### `xattr name :namespace?`

Gets an extended attribute value.

#### Parameters

| Name        | Type                                                   | Description                           |
| ----------- | ------------------------------------------------------ | ------------------------------------- |
| `name`      | [`Str`](../std/str.md)\|[`XattrEntry`](xattr-entry.md) | Attribute name or entry from `xattrs` |
| `namespace` | [`Str`](../std/str.md)\|[`Sym`](../std/sym.md)?        | Namespace to query                    |

#### Returns

[`Bin`](../std/bin.md)

#### Example

```
open data.txt r do |file|
  let value = file.xattr "comment"
```

### `set_xattr name value :namespace?`

Sets an extended attribute value.

On Windows, empty values are rejected. NTFS deletes the attribute instead of
storing an empty value.

#### Parameters

| Name        | Type                                                   | Description                           |
| ----------- | ------------------------------------------------------ | ------------------------------------- |
| `name`      | [`Str`](../std/str.md)\|[`XattrEntry`](xattr-entry.md) | Attribute name or entry from `xattrs` |
| `value`     | [`Str`](../std/str.md)\|[`Bin`](../std/bin.md)         | Attribute bytes; strings use UTF-8    |
| `namespace` | [`Str`](../std/str.md)\|[`Sym`](../std/sym.md)?        | Namespace to update                   |

#### Example

```
open data.txt r+ do |file|
  file.set_xattr "comment" "ready"
```

### `remove_xattr name :namespace?`

Removes an extended attribute.

#### Parameters

| Name        | Type                                                   | Description                           |
| ----------- | ------------------------------------------------------ | ------------------------------------- |
| `name`      | [`Str`](../std/str.md)\|[`XattrEntry`](xattr-entry.md) | Attribute name or entry from `xattrs` |
| `namespace` | [`Str`](../std/str.md)\|[`Sym`](../std/sym.md)?        | Namespace to update                   |

#### Example

```
open data.txt r+ do |file|
  file.remove_xattr "comment"
```

### `seek offset`

Moves the file cursor by a relative byte offset.

Buffered unread data is discarded before the seek so subsequent reads use the
new cursor position.

#### Parameters

| Name     | Type                     | Description                       |
| -------- | ------------------------ | --------------------------------- |
| `offset` | [`Int`](../std/index.md) | Relative byte offset from current |

#### Returns

[`Int`](../std/index.md) - New absolute byte position

#### Example

```
open data.bin rb do |file|
  file.seek 10
  file.seek (0 - 4)
```

### `seek start: ofs`

Moves the file cursor to an absolute byte offset from the start of the file.

Buffered unread data is discarded before the seek so subsequent reads use the
new cursor position.

#### Parameters

| Name  | Type                     | Description                          |
| ----- | ------------------------ | ------------------------------------ |
| `ofs` | [`Int`](../std/index.md) | Absolute byte offset from file start |

#### Returns

[`Int`](../std/index.md) - New absolute byte position

#### Example

```
open data.bin rb do |file|
  file.seek start: 10
  let pos = file.tell()
```

### `seek end: ofs`

Moves the file cursor to a byte offset relative to the end of the file.

Buffered unread data is discarded before the seek so subsequent reads use the
new cursor position.

#### Parameters

| Name  | Type                     | Description                      |
| ----- | ------------------------ | -------------------------------- |
| `ofs` | [`Int`](../std/index.md) | Byte offset relative to file end |

#### Returns

[`Int`](../std/index.md) - New absolute byte position

#### Example

```
open data.bin rb do |file|
  file.seek end: (0 - 1)
```

### `tell()`

Returns the current file cursor position in bytes.

#### Returns

[`Int`](../std/index.md)

#### Example

```
open data.txt r do |file|
  assert_eq (file.tell()) 0
  file.read 5
  assert_eq (file.tell()) 5
```

### `close()`

Explicitly closes the file. Required if you didn't use the `func` parameter to
`open()`. Closing a file that is already closed does nothing.

Passing a file to a child process as `stdin:`, `stdout:`, or `stderr:` also
closes it: the child receives the file itself, and the seek position is kept by
this process rather than by the operating system, so two live handles would each
believe a cursor the other moves. Use the file after the handoff and it raises
the ordinary closed-file error.

#### Example

```
let file = open data.txt r
let data = file.read()
file.close()
```

## Iterator and Sink Protocols

Files implement the iterator and sink protocols, allowing them to be used
with `for` loops, `.next()`, `.put()`, and `strand.redirect`.

### `input`/`output`

Returns the file as its own iterator and sink.

#### Returns

The file object itself

The open mode determines whether iteration yields lines or binary chunks for
the file's lifetime. In either mode, the values read from a file concatenate
back to exactly the file's bytes.

### `next`

Fetches the next item from the file.

#### Text mode

 Reads the next line as a [`Str`](../std/str.md), **including its
terminator**, so a `\r\n` file stays `\r\n` and a final line without one yields
a value without one. Use [`chomp`](../std/iter.md#chomp) to strip them:

```
for line = file.chomp()
  echo $line
```

#### Binary mode

 Reads a chunk of data of arbitrary length.

### `put`

Writes a value to the file, verbatim: a [`Str`](../std/str.md) or
[`Bin`](../std/bin.md) contributes its own bytes and nothing else, and anything
else is converted to a string first. No line ending is appended in either mode,
and none is translated.

Use [`precrimp`](../std/sink.md#precrimp-terminator) to terminate written
values, with [`shell.line_ending()`](../shell/index.md#line_ending) for the
target's native ending:

```
open $path w do |file|
  let lines = file.precrimp()
  lines.put "first"
  lines.put "second"
```
