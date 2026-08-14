# Vfs

`Vfs` runs code in another execution context. Filesystem access, external
programs, environment, working directory, target system information, and
security identity follow the selected context.

## Constructor

### `Vfs func`

Runs `func` on a background stream and connects to a VFS server over its input
and output. The function must launch a program that speaks the VFS protocol.
The `Vfs` retains the background stream for its lifetime.

#### Parameters

| Name   | Type     | Description                 |
| ------ | -------- | --------------------------- |
| `func` | callable | VFS server launcher         |

```
let remote = Vfs do run ssh host dolang-vfs --stdio
```

The `--stdio` mode reads the protocol from standard input and writes it to
standard output. The launched program must not write other output to standard
output; diagnostics may use standard error. If the launcher exits before the
VFS handshake completes, its error is reported instead of the resulting VFS
disconnect.

## Class Methods

### `unix_socket path :key?`

Connects to a running `dolang-vfs` daemon on Unix.

The socket path is resolved in the active VFS context. This allows connecting
to a daemon reachable only from another remote or container VFS. A direct
non-Unix context reports that Unix VFS connections are unsupported.

The working directory in which the `dolang-vfs` process started becomes
the context's initial working directory.

`key` is a pre-shared key that both ends prove knowledge of during the
handshake. Supply it when the socket's permissions cannot identify the peer —
an agent inside a container binds its socket `0666` because the uid that will
connect is not knowable in advance, leaving it reachable by anything that can
traverse the containing directory. The agent must have been
started with `--key-stdin` and given the same key, and the requirement is
mutual: a key here fails against an agent without one, and an agent expecting
one refuses a connection without it. Keys must be at least 16 bytes, and must
carry sufficient entropy on their own — generate one with
[`rand.string`](../rand.md#string-len-alphabet) rather than choosing it.

This authenticates the connection; it does not encrypt or integrity-protect
the session that follows.

#### Parameters

| Name   | Type                                                 | Description       |
| ------ | ---------------------------------------------------- | ----------------- |
| `path` | [`Path`](../fs/path.md)                              | Unix socket path  |
| `key`  | [`str`](../std/str.md)\|[`bin`](../std/bin.md)?      | Pre-shared key    |

#### Returns

`Vfs`

#### Errors

- The agent is not listening, or the path is not a socket
- Either end fails to prove the key, or only one end was given one

```
let a = Vfs.unix_socket /tmp/agent/socket
```

Authenticated, with a freshly generated key:

```
let key = rand.string 32
let a = Vfs.unix_socket /tmp/agent/socket :key
```

### `windows_admin :cd? :env?`

Launches an elevated copy of the active Windows VFS target's current
executable. This can cross an existing VFS connection, such as a connection
from WSL to its Windows host.

The target resolves its own executable; callers cannot select another program.
When the executable is `dolang.exe`, `--vfs` is inserted to select its VFS
entrypoint. A standalone `dolang-vfs.exe` is launched directly.

The calling strand's current working directory becomes the context's initial
working directory unless `cd:` overrides it. `env:` accepts string or symbol
keys; `nil` unsets a variable and `:INHERIT:` captures the calling strand's
current value.

#### Parameters

| Name  | Type                     | Description           |
| ----- | ------------------------ | --------------------- |
| `cd`  | [`Path`](../fs/path.md)? | Initial directory     |
| `env` | [`Dict`](../std/dict.md) | Environment overrides |

Windows displays a User Account Control prompt. Cancelling the prompt raises a
system error.

Windows does not reliably allow the elevated process to use console handles
from the non-elevated caller. Console programs using inherited terminal I/O may
hang, fail, or produce no output. Redirected and captured output uses ordinary
handles and works across the elevated VFS boundary.

#### Returns

`Vfs`

```
let admin = Vfs.windows_admin()
```

## Methods

### `with func ...args`

Executes a callable in the `Vfs` context:

```
a.with do
  # Commands here use the VFS context
  # env and cd also use that context
  run.ls /
  run.cat /etc/os-release
```

Each entry starts from the context's initial working directory. This value is
fixed when the `Vfs` is created, so moving the handle out of its original
working-directory scope does not change it. A `cd` inside the block affects
that entry but not the starting directory of later entries.

Entering a `Vfs` context affects the operations that are routed through it:

- External programs run through the VFS daemon
- [`shell.env`](../shell/index.md#env) reads and writes that context's
  environment
- [`shell.cd`](../shell/index.md#cd-path-func) changes that context's working
  directory
- [`fs`](../fs/index.md) operations use that context's filesystem view
- [`sys`](../sys/index.md) reports that context's operating system and CPU
- [`security`](../security/index.md) reports that context's identity

Only VFS-aware operations follow the context. Other network clients and native
resources remain in the interpreter process unless their documentation says
otherwise.

### `stop()`

Sends a stop request to the connected VFS server. A callable-backed `Vfs` then
closes and joins its launcher stream. On Windows, it also waits for the
elevated process to exit.

```
a.stop()
```
