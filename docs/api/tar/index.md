# tar

Streams TAR archives with optional gzip or zstd compression.

## Types

| Type                               | Description                        |
| ---------------------------------- | ---------------------------------- |
| [`Entry`](./entry.md)              | Metadata and content for one entry |
| [`EntryWriter`](./entry-writer.md) | Scoped entry content sink          |
| [`Reader`](./reader.md)            | Destructive archive entry iterator |
| [`Writer`](./writer.md)            | Sequential archive writer          |

## Functions

### `read path func`

Opens an archive and calls `func` with a [`Reader`](./reader.md).
Compression is detected from gzip or zstd magic bytes.

#### Parameters

| Name   | Type                                            | Description  |
| ------ | ----------------------------------------------- | ------------ |
| `path` | [`Str`](../std/str.md)\|[`Path`](../fs/path.md) | Archive path |
| `func` | `Func`                                          | Reader scope |

#### Returns

the result of `func`.

#### Example

```
read "archive.tar.gz" do |archive|
  for entry = archive
    echo "$entry.path: $entry.size bytes"
```

### `write path :compression? func`

Creates an archive and calls `func` with a [`Writer`](./writer.md).

#### Parameters

| Name          | Type                                            | Description                     |
| ------------- | ----------------------------------------------- | ------------------------------- |
| `path`        | [`Str`](../std/str.md)\|[`Path`](../fs/path.md) | Archive path                    |
| `compression` | [`Sym`](../std/sym.md)?                         | `:NONE:`, `:GZIP:`, or `:ZSTD:` |
| `func`        | `Func`                                          | Writer scope                    |

When `compression` is omitted, `.gz` and `.tgz` select gzip, `.zst` and
`.tzst` select zstd, and other extensions select no compression. Extension
matching is case-insensitive. An explicit `compression` overrides the path.

#### Returns

the result of `func`.

#### Example

```
write "archive.tar.zst" do |archive|
  archive.entry greeting.txt size: 5 do |entry|
    entry.write hello
```
