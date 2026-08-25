# dolang-vfs Architecture

`dolang-vfs` is a virtual filesystem and process-spawning layer for the
shell runtime.

It has two backends:

- Direct filesystem access and process spawning
- An RPC client which forwards to a server running the direct backend.

## Extension operations

Crates can add operations without modifying the core request enum by
implementing `VfsExtension` and registering it with `vfs_extension!`. An
extension defines a serializable request type, response type, stable name, and
protocol version. The linker collects registrations through a `linkme`
distributed slice.

Extension calls use the same path as built-in VFS operations. `Direct`
dispatch invokes the registered handler in-process. `Client` serializes the
extension name, version, and request through the VFS RPC protocol; the server
looks up the same registration and serializes its response. Unknown names or
versions return a VFS error.

`ExtContext` gives handlers a dispatch-independent resource table. In direct
mode, `ExtOpaque` handles retain resources with `Arc`; in remote mode, they use
the RPC session's typed opaque-object table. `register`, `acquire`, and
`unregister` therefore have the same API in both modes. Dropping a session
releases remote resources that were not explicitly unregistered.

Remote cancellation is cooperative inside `ExtContext::cancel_guard`. Direct
calls use ordinary future cancellation, so the same guard is a pass-through.
Attachment-capable transports can also carry an `ExtOsHandle`; handlers must
check `ExtContext::native_capable` before returning one. Other transports use
opaque resources instead.

An RPC client may carry an opaque VFS selector. The selector names an `AnyVfs`
retained by the outer session, allowing the same request protocol and handlers
to operate through another remote backend. Every request carries the optional
selector; retained files, stdio ends, and children are associated with that
VFS domain.

Opening a Unix-socket VFS through a native direct session transfers the
connected socket back to the caller, avoiding request forwarding. When the
outer transport cannot transfer handles, the server retains the connected
client and returns an opaque VFS selector instead. Stopping a selected VFS
stops and releases only that backend. Outer-session teardown drops retained
clients without stopping their independent daemons.

`Vfs::close` gracefully tears down a remote session: it closes the RPC input
after draining committed writes, then waits for the server to close its output.
That wait is intentionally unbounded; callers provide a timeout when needed.
`Vfs::abort` stops both transport tasks without waiting for a failed or
uncooperative peer. Direct backends require neither operation.
Windows named pipes cannot half-close, so their graceful path keeps reading
through the write drain and then closes the shared duplex pipe.

A Unix-socket connection may carry a pre-shared key, which both ends prove
knowledge of during the RPC handshake (see dolang-rpc's architecture notes).
This matters because the agent widens its socket to `0666`: the uid that will
connect is not knowable in advance when a container runtime chooses the id
mapping, so the socket's permissions cannot identify the peer and the
containing directory's `0700` mode is the only other barrier. The key is what
distinguishes the intended client, and the intended agent.

Which side authenticates depends on how the nested connection is made. On the
handle-transferring path the peer returns a connected descriptor and
negotiation happens locally, so the key never leaves this process; on the
opaque path the peer establishes the connection on our behalf and the key
travels in the request, over the already-trusted outer session. The request
carries it either way, because which path the peer takes is its decision.

### Single-Session Mode

`--accept <path>` serves exactly one successfully negotiated client and then
exits, unlinking the socket the moment that session is established rather than
when it ends. Combined with a key it bounds exposure to the interval between
`READY` and the first authenticated connection.

Negotiation happens in per-connection tasks, so a failed or stalled attempt
neither consumes the single session slot nor blocks the accept loop: losing the
race to an impostor costs the intended client an attempt, not its session.
Attempts are bounded by a negotiation timeout and a cap on how many may be
in flight; once one succeeds, the rest are abandoned rather than drained.

Unlike `--listen`, this mode binds directly at the final path. The staging
rename `--listen` performs exists only so a client polling for the path's
existence cannot find a socket that is not yet listening, and single-session
clients have the `READY` line — printed after the mode is widened — which is a
better readiness signal than the path's existence.

The key reaches the agent through `--key-stdin`, as a single length byte
followed by that many bytes. stdin is the only channel a launcher can write
that leaves nothing a third party can read afterwards: an argument vector is
world-visible through `/proc`, and an environment variable is both readable by
anything that can reach the process's environ and inherited by every child the
agent goes on to spawn. `--key-stdin` is rejected with `--stdio`, where stdin
carries the transport itself and the channel's creator has already established
who the peer is.

The Unix socket VFS normally exchanges raw file descriptors with `SCM_RIGHTS`.
An opaque-only client instead asks the server to retain regular files and
performs byte I/O, seeking, flushing, and truncation through typed opaque RPC
identities. Generic byte-stream sessions use the same retained-file path.

Open requests prefer native handles on attachment-capable sessions unless the
client explicitly requests an opaque file. A server which cannot transfer
handles falls back to an opaque file even for a native-preferred request.
Cursor-affecting operations on each retained file are serialized by the server.
Explicit close removes the resource and consumes the file when no operation is
racing; otherwise close reports that the resource is busy and the outstanding
guard performs final drop cleanup. Connection teardown drops all resources
which remain in the RPC object table.

On Windows, the RPC client uses the server end of a connected named pipe and
the RPC server uses the client end. The client retains a trusted handle for the
server process so `dolang-rpc` can transfer native handles in both directions.
`shell.Vfs.windows_admin()` creates the pipe and launches the direct Windows
backend's current executable through UAC. The request can be forwarded through
opaque VFS handles, but never includes an executable path. `dolang.exe` receives
the private `--vfs` selector; `dolang-vfs.exe` already uses the VFS argument
syntax directly. The child serves the direct backend until shutdown or
disconnect.

For automated tests, `shell.Vfs.windows_admin(elevate: false)` uses a normal
same-user process launch instead. Windows release validation must also accept a
real UAC prompt, perform an operation requiring administrator access, call
`stop()`, and confirm that the child exits. Cancelling the prompt must return
an error without leaving a child process.

An elevated Windows VFS process cannot reliably use console handles inherited
from its non-elevated parent. Programs which require console input or output
may therefore hang, fail, or produce no output when run through an elevated
VFS session. The VFS still duplicates standard handles because doing so is
harmless and remains useful for non-console handles. In particular, redirected
and captured output works because it uses ordinary handles rather than the
parent console.

Path-based operations execute on the RPC server. Operations whose names begin
with `file_` act locally on a file handle that the server already transferred
to the client.

VFS operations return the crate's owned `Error` type, which carries an
`ErrorKind`, formatted message, and optional raw code with its originating
operating system. The same representation crosses RPC unchanged. A client must
not interpret a foreign raw code using the host platform's error tables.

`io::Error` remains at async stream, standard-library, and transport
boundaries. Converting one into a VFS error captures its formatted message and
current-platform raw code without parsing the message.

The initial VFS query returns a snapshot of the target environment, working
directory, operating system, architecture, logical CPU count, and Wine status.
Operating systems and architectures are closed enums covering the project's
supported ports. The shell stores the target snapshot in strand-local context,
so system information follows nested VFS contexts rather than the interpreter
host.
