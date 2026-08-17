# sys

The `sys` module exposes basic information and types associated with systems
and platforms supported by Do.

## Types

| Type                                                                  | Description                                          |
| --------------------------------------------------------------------- | ---------------------------------------------------- |
| [`OsInfo`](./osinfo.md)                                               | Operating system target information                  |
| [`CpuInfo`](./cpuinfo.md)                                             | CPU target information                               |
| [`ErrorCode`](./error-code.md)                                        | Native system error code                             |
| [`Error`](./error.md)                                                 | Error raised for system and I/O failures             |
| [`NotFoundError`](./not-found-error.md)                               | Missing file, path, or program                       |
| [`PermissionDeniedError`](./permission-denied-error.md)               | Operation not permitted by access rules              |
| [`AlreadyExistsError`](./already-exists-error.md)                     | Creating or renaming conflicts with an existing path |
| [`InvalidInputError`](./invalid-input-error.md)                       | Invalid input to a system operation                  |
| [`TimedOutError`](./timed-out-error.md)                               | System operation timed out                           |
| [`UnsupportedError`](./unsupported-error.md)                          | Unsupported system operation                         |
| [`ConnectionRefusedError`](./connection-refused-error.md)             | Connection attempt refused                           |
| [`ConnectionResetError`](./connection-reset-error.md)                 | Connection was reset                                 |
| [`HostUnreachableError`](./host-unreachable-error.md)                 | Network host is unreachable                          |
| [`NetworkUnreachableError`](./network-unreachable-error.md)           | Network is unreachable                               |
| [`ConnectionAbortedError`](./connection-aborted-error.md)             | Connection was aborted                               |
| [`NotConnectedError`](./not-connected-error.md)                       | Operation requires a connected endpoint              |
| [`AddrInUseError`](./addr-in-use-error.md)                            | Network address already in use                       |
| [`AddrNotAvailableError`](./addr-not-available-error.md)              | Requested network address is unavailable             |
| [`NetworkDownError`](./network-down-error.md)                         | Network is down                                      |
| [`BrokenPipeError`](./broken-pipe-error.md)                           | Pipe closed by its reader                            |
| [`WouldBlockError`](./would-block-error.md)                           | Operation would block                                |
| [`NotADirectoryError`](./not-a-directory-error.md)                    | Path component is not a directory                    |
| [`IsADirectoryError`](./is-a-directory-error.md)                      | File operation targeted a directory                  |
| [`DirectoryNotEmptyError`](./directory-not-empty-error.md)            | Directory is not empty                               |
| [`ReadOnlyFilesystemError`](./read-only-filesystem-error.md)          | Modifying a read-only filesystem                     |
| [`StaleNetworkFileHandleError`](./stale-network-file-handle-error.md) | Stale network filesystem handle                      |
| [`WriteZeroError`](./write-zero-error.md)                             | Write wrote zero bytes                               |
| [`StorageFullError`](./storage-full-error.md)                         | Storage device is full                               |
| [`NotSeekableError`](./not-seekable-error.md)                         | Stream does not support seeking                      |
| [`QuotaExceededError`](./quota-exceeded-error.md)                     | Storage quota exceeded                               |
| [`FileTooLargeError`](./file-too-large-error.md)                      | File exceeds size limit                              |
| [`ResourceBusyError`](./resource-busy-error.md)                       | System resource is busy                              |
| [`ExecutableFileBusyError`](./executable-file-busy-error.md)          | Executable file is busy                              |
| [`DeadlockError`](./deadlock-error.md)                                | Operation would cause a deadlock                     |
| [`CrossesDevicesError`](./crosses-devices-error.md)                   | Operation crosses device boundaries                  |
| [`TooManyLinksError`](./too-many-links-error.md)                      | Filesystem object has too many links                 |
| [`InvalidFilenameError`](./invalid-filename-error.md)                 | Filename is invalid                                  |
| [`ArgumentListTooLongError`](./argument-list-too-long-error.md)       | System argument list is too long                     |
| [`InvalidDataError`](./invalid-data-error.md)                         | Malformed input data                                 |
| [`InterruptedError`](./interrupted-error.md)                          | System operation was interrupted                     |
| [`UnexpectedEofError`](./unexpected-eof-error.md)                     | Input ended unexpectedly                             |
| [`OutOfMemoryError`](./out-of-memory-error.md)                        | Could not allocate memory                            |

## Functions

### `os_info()`

Returns operating system information for the current VFS target.

```
if (sys.os_info().family == :WINDOWS:)
  echo "running on Windows"
```

### `cpu_info()`

Returns CPU information for the current VFS target.

```
let info = sys.cpu_info()
echo "running on $info.arch with $info.logical_count logical CPUs"
```
