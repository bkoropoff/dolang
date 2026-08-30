# sys

The `sys` module exposes basic information and types associated with systems
and platforms supported by Do.

## Types

| Type                                                                  | Description                                          |
| --------------------------------------------------------------------- | ---------------------------------------------------- |
| [`AddrInUseError`](./addr-in-use-error.md)                            | Network address already in use                       |
| [`AddrNotAvailableError`](./addr-not-available-error.md)              | Requested network address is unavailable             |
| [`AlreadyExistsError`](./already-exists-error.md)                     | Creating or renaming conflicts with an existing path |
| [`ArgumentListTooLongError`](./argument-list-too-long-error.md)       | System argument list is too long                     |
| [`BrokenPipeError`](./broken-pipe-error.md)                           | Pipe closed by its reader                            |
| [`ConnectionAbortedError`](./connection-aborted-error.md)             | Connection was aborted                               |
| [`ConnectionRefusedError`](./connection-refused-error.md)             | Connection attempt refused                           |
| [`ConnectionResetError`](./connection-reset-error.md)                 | Connection was reset                                 |
| [`CpuInfo`](./cpuinfo.md)                                             | CPU target information                               |
| [`CrossesDevicesError`](./crosses-devices-error.md)                   | Operation crosses device boundaries                  |
| [`DeadlockError`](./deadlock-error.md)                                | Operation would cause a deadlock                     |
| [`DirectoryNotEmptyError`](./directory-not-empty-error.md)            | Directory is not empty                               |
| [`Error`](./error.md)                                                 | Error raised for system and I/O failures             |
| [`ErrorCode`](./error-code.md)                                        | Native system error code                             |
| [`ExecutableFileBusyError`](./executable-file-busy-error.md)          | Executable file is busy                              |
| [`FileTooLargeError`](./file-too-large-error.md)                      | File exceeds size limit                              |
| [`HostUnreachableError`](./host-unreachable-error.md)                 | Network host is unreachable                          |
| [`InterruptedError`](./interrupted-error.md)                          | System operation was interrupted                     |
| [`InvalidDataError`](./invalid-data-error.md)                         | Malformed input data                                 |
| [`InvalidFilenameError`](./invalid-filename-error.md)                 | Filename is invalid                                  |
| [`InvalidInputError`](./invalid-input-error.md)                       | Invalid input to a system operation                  |
| [`IsADirectoryError`](./is-a-directory-error.md)                      | File operation targeted a directory                  |
| [`NetworkDownError`](./network-down-error.md)                         | Network is down                                      |
| [`NetworkUnreachableError`](./network-unreachable-error.md)           | Network is unreachable                               |
| [`NotADirectoryError`](./not-a-directory-error.md)                    | Path component is not a directory                    |
| [`NotConnectedError`](./not-connected-error.md)                       | Operation requires a connected endpoint              |
| [`NotFoundError`](./not-found-error.md)                               | Missing file, path, or program                       |
| [`NotSeekableError`](./not-seekable-error.md)                         | Stream does not support seeking                      |
| [`OsInfo`](./osinfo.md)                                               | Operating system target information                  |
| [`OutOfMemoryError`](./out-of-memory-error.md)                        | Could not allocate memory                            |
| [`PermissionDeniedError`](./permission-denied-error.md)               | Operation not permitted by access rules              |
| [`QuotaExceededError`](./quota-exceeded-error.md)                     | Storage quota exceeded                               |
| [`ReadOnlyFilesystemError`](./read-only-filesystem-error.md)          | Modifying a read-only filesystem                     |
| [`ResourceBusyError`](./resource-busy-error.md)                       | System resource is busy                              |
| [`StaleNetworkFileHandleError`](./stale-network-file-handle-error.md) | Stale network filesystem handle                      |
| [`StorageFullError`](./storage-full-error.md)                         | Storage device is full                               |
| [`TimedOutError`](./timed-out-error.md)                               | System operation timed out                           |
| [`TooManyLinksError`](./too-many-links-error.md)                      | Filesystem object has too many links                 |
| [`UnexpectedEofError`](./unexpected-eof-error.md)                     | Input ended unexpectedly                             |
| [`UnsupportedError`](./unsupported-error.md)                          | Unsupported system operation                         |
| [`WouldBlockError`](./would-block-error.md)                           | Operation would block                                |
| [`WriteZeroError`](./write-zero-error.md)                             | Write wrote zero bytes                               |

## Functions

### `cpu_info()`

Returns CPU information for the current VFS target.

```
let info = sys.cpu_info()
echo "running on $info.arch with $info.logical_count logical CPUs"
```

### `os_info()`

Returns operating system information for the current VFS target.

```
if (sys.os_info().family == :WINDOWS:)
  echo "running on Windows"
```
