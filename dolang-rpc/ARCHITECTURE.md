# dolang-rpc Architecture

`dolang-rpc` is a framed, multiplexed RPC session library.

The crate has two independent facilities:

- Request/response framing and correlation over an arbitrary asynchronous byte
  stream.
- Platform-specific direct transfer of native operating-system handles when a
  session can support it.

An application can use the first facility over separate stdio pipes, a TCP
connection, or a local socket. Direct handles are an optional optimization;
remote protocols use [`Opaque`](#opaque-objects) references instead.

## Protocol Endpoints

A protocol is a marker type. It keeps endpoint type signatures short and is
the future home for protocol-wide static configuration.

```rust
trait Protocol: Send + Sync + 'static {
    type Request: Serialize + DeserializeOwned + Send + 'static;
    type Response: Serialize + DeserializeOwned + Send + 'static;
}

struct Vfs;

impl Protocol for Vfs {
    type Request = VfsRequest;
    type Response = VfsResponse;
}
```

The public endpoint types are `Client<P>` and `Server<P>`. Their roles are
semantically significant for call direction and native-handle transfer. A
client sends `P::Request` and receives the correlated `P::Response`; the server
receives requests and sends responses.

Neither type has a public constructor. Both are obtained only by binding an
already-negotiated [`UnboundClient`/`UnboundServer`](#session-establishment)
to a concrete `P`, which guarantees every `Client<P>`/`Server<P>` has already
completed the handshake before an application can call or serve through it.

```rust
impl<P: Protocol> Client<P> {
    fn call(&self, request: P::Request) -> Call<P>;
    fn call_with_trailer(&self, request: P::Request) -> TrailerSend<Call<P>>;
}

impl<P: Protocol> Call<P> {
    fn cancel(&mut self);
}

impl<P: Protocol> Future for Call<P> {
    type Output = Result<CallResult<P::Response>, Error>;
    // ...
}
```

`CallResult<R>` wraps the response together with an optional inbound trailer:
`into_response(self) -> R` discards any trailer, `into_response_trailer(self)
-> (R, Option<TrailerRecv>)` retains it. See
[Payload Trailers](#payload-trailers-and-fragmentation).

Per-variant request/response typing is intentionally deferred. An
application-level macro can later generate a dispatch enum and a trait that
maps an individual request struct to its response type without complicating
the session core.

Callbacks and server-initiated requests are also deferred. They should use an
explicitly separate reverse-direction protocol rather than assuming that
`Response` is a callback request type.

## Session Establishment

`Client<P>`/`Server<P>` are reached only through `Builder`, which runs the
handshake described in [Framing And Multiplexing](#framing-and-multiplexing)
before an application ever picks a `P` to bind to:

```rust
let unbound = Builder::new("vfs", &[1])
    .max_payload_size(4 * 1024 * 1024)
    .client(stream)     // or .client_split/.client_unix/.client_named_pipe_*
    .await?;

assert_eq!(unbound.name(), "vfs");
let client: Client<Vfs> = unbound.bind();
```

`Builder::new(name, versions)` takes the mandatory application-protocol
descriptor as two plain arguments. Chainable setters
(`max_fragment_size`, `max_payload_size`, `max_trailer_size`,
`trailer_recv_copy_threshold`, `trailer_recv_demand_copy_threshold`,
`trailer_send_copy_threshold`, `max_incomplete_messages`,
`max_incomplete_trailers`) override individual size and concurrency limits;
these compose the crate-private `Limits` struct, which is not itself public.
Terminal `async` methods consume the builder and negotiate over a specific
transport shape, one set for each endpoint role:

- Client: `client`, `client_split`, `client_unix`, and, on Windows, the
  `unsafe` `client_named_pipe_server`/`client_named_pipe_client` (peer-process
  trust, see [Direct Native Handles](#direct-native-handles)).
- Server: `server`, `server_split`, `server_unix`, and, on Windows,
  `server_named_pipe_server`/`server_named_pipe_client`.

Each returns `Result<UnboundClient, Error>` or `Result<UnboundServer, Error>`.
An `Unbound*` value has already completed the handshake and always carries a
resolved application-protocol name and negotiated version.

```rust
impl UnboundClient {
    fn name(&self) -> &str;      // negotiated application-protocol name
    fn version(&self) -> u16;    // negotiated application-protocol version
    fn bind<P: Protocol>(self) -> Client<P>;
}

impl UnboundServer {
    fn name(&self) -> &str;
    fn version(&self) -> u16;
    fn bind<P: Protocol>(self) -> Server<P>;
}
```

`version()` reports the negotiated **application**-protocol version, not the
RPC framing version — the framing version is an implementation detail of
`dolang-rpc` itself, uninteresting to a caller choosing which `P` to bind to.

Deferring the choice of `P` this way lets a listener accept a connection,
inspect which application protocol and version the peer actually offered, and
only then decide how to bind it — without requiring the caller to guess `P`
before the handshake runs.

## Server Dispatch

`Server::serve` owns the receive loop and dispatches incoming requests to an
application handler. The session runs each request independently and can
cancel it later. The handler receives its `CallContext<P>` by value, tied to
that request:

```rust
impl<P: Protocol> Server<P> {
    async fn serve<H>(self, handler: H) -> Result<(), Error>
    where
        H: AsyncFn(CallContext<P>, P::Request) + Send + Sync + 'static;
}
```

The handler is shared between independently dispatched requests and may be
called concurrently. The session owns each returned future, so an unguarded
request remains abortable. The handler responds by consuming its context —
`context.respond(response)` or `context.respond_with_trailer(response)` — not
by returning a value; application-level failures still belong in
`P::Response`. A handler that drops its context without responding causes the
session to send `Error { id }` for that request.

`CallContext<P>` is not `Server<P>` and is not cloneable. Its exclusive
ownership makes request-scoped state linear. It provides session services
appropriate to request processing:

```rust
impl<P: Protocol> CallContext<P> {
    fn request_trailer(&mut self) -> Option<&mut TrailerRecv>;
    fn respond(self, response: P::Response);
    fn respond_with_trailer(self, response: P::Response) -> TrailerSend<()>;
    fn shutdown(&self);
    async fn cancel_guard<T, F>(&mut self, operation: F) -> Result<T, RequestCancelled>
    where
        F: AsyncFnOnce(&mut CallContext<P>) -> T;
    fn register<T: OpaqueResource>(&self, value: T) -> Opaque<T::Marker>;
    fn acquire<T: OpaqueResource>(&self, value: Opaque<T::Marker>) -> Result<OpaqueGuard<T>, InvalidOpaque>;
    fn unregister<T: OpaqueResource>(&self, value: Opaque<T::Marker>) -> Result<Option<T>, InvalidOpaque>;
}
```

`shutdown` stops the server from accepting new requests and lets `serve`
return once in-flight ones finish, without severing the transport out from
under them.

## Cancellation Guards

`CallContext::cancel_guard` lets a handler intercept cancellation for a
particular asynchronous operation. It reborrows the context exclusively into
an async closure:

```rust
let result = context
    .cancel_guard(async |context| {
        perform_operation(context).await
    })
    .await;
```

If the request is cancelled while that closure is running, the guard drops the
closure's future and returns `Err(RequestCancelled)` instead. The handler's
future is not dropped. It can use normal Rust error handling to clean up with
the re-acquired `&mut CallContext<P>` and return an ordinary protocol
response, including an application-defined cancellation error.

The mutable reborrow is intentional: a context cannot be used outside the
guard while the guarded operation owns it, and the error path regains the same
linear context. This avoids concurrent session-control operations from one
request.

## Framing And Multiplexing

The session owns frame writes. This prevents concurrent callers from
interleaving frame bytes.

The framing format carries a frame kind, a monotonically increasing `u64`
message ID, and serialized payload bytes. A process will not exhaust a `u64`
counter in practice, so IDs are never reused during a session. When both peers
originate a class of ID, the frame format identifies the origin role, or the
protocol uses independent directional ID spaces.

Frame kinds are:

```text
Request   { id, payload }
Response  { id, payload }
Error     { id, kind }
Notify    { id, payload }
Cancel    { id }
Discard   { id }
Negotiate { id, payload }
```

`Request` and `Response` provide ordinary RPC correlation. `Notify` is for
one-way protocol messages. `Error` is a terminal session-level failure for the
correlated request, initially including cancellation. `Cancel` controls a
request already in flight. `Discard` is an advisory, non-fatal signal that the
receiver no longer wants a request's or response's trailer; see
[Payload Trailers](#payload-trailers-and-fragmentation). `Negotiate` is the
handshake message described below, and must run to completion before any
other kind is valid on a connection.

The exact binary envelope is an implementation detail, but it must preserve
frame boundaries and associate native-handle attachment serialization state
with precisely one frame. Attachment counts are not part of the envelope:
attachment representations in the serialized payload implicitly determine
which handles its deserializer consumes.

### Header Layout

Every frame (fragment) is preceded by a 16-byte header:

```rust
struct RawFragmentHeader {
    flags: [u8; 1],
    kind: [u8; 1],
    reserved: [u8; 2],
    payload_len: [u8; 4],
    id: [u8; 8],
}
```

`reserved` is always zero on write and is read but never validated on
receive — a peer sending non-zero reserved bytes today must not be rejected,
which keeps it available for a future header-level extension without a
breaking wire change.

### Handshake And Application-Protocol Negotiation

Session establishment negotiates two independent things over a single
`Negotiate` message exchange, before any other frame kind is valid:

- The RPC framing version itself: each peer's `id` field (normally an 8-byte
  message ID) is repurposed during negotiation to carry that peer's sorted
  ascending, zero-terminated list of supported `u8` framing versions (up to 8
  fit). Each peer independently selects the maximum mutually supported
  version from the intersection — no acknowledgement round trip is needed. A
  process implementing this crate today only ever offers version `1`.
- A mandatory application-protocol name and version, carried in the payload
  rather than the `id` field: `{ version_blobs: Vec<Vec<u8>>, app_protocol:
  (String, Vec<u16>) }` (a struct — postcard serializes it identically to a
  plain tuple of its field types, so this is purely a readability choice, not
  a wire format one), where `version_blobs` is one length-prefixed handshake
  blob per offered RPC version (reserved for future per-version handshake
  data; not yet consulted by either endpoint) and `app_protocol` is this
  peer's application protocol name plus a sorted ascending list of supported
  `u16` versions. Application-protocol versions are `u16` rather than `u8`
  because application protocols built on top of `dolang-rpc` are expected to
  revise much more often than the RPC framing itself, and they aren't
  limited by the `id` field's 8-slot capacity.

There is no way to negotiate a session without an application protocol —
`negotiate()` requires both peers to supply a name and version list, with no
skip/opt-out path, since [`Builder`](#session-establishment) is the only way
to reach it and always has one to offer. This keeps `NegotiationResult` and
every caller of it free of an `Option` that could never actually be `None`
in practice.

A mismatch — no mutually supported RPC version, a mismatched application
protocol name, or no mutually supported application-protocol version — makes
the detecting side send a zero-payload `Negotiate` fragment with `FIRST |
ABORT` set, best-effort, then fail locally. The three cases produce distinct
local error messages (though the wire-level abort signal itself is
undifferentiated, matching the ordinary fragment abort mechanism below), so a
caller can tell an RPC-framing incompatibility from an application-protocol
name or version mismatch.

Each side drives its own write and its peer's read concurrently rather than
sequentially, so a handshake payload large enough to need more than one
read/write pass on either end cannot deadlock waiting for the other side to
finish sending first.

## Payload Trailers And Fragmentation

Bulk byte data does not have to pass through postcard when its structure is
already described by the request or response. A message envelope can
therefore carry a raw payload trailer after the postcard payload, streamed
through `AsyncWrite`/`AsyncRead` rather than buffered whole:

```rust
impl<P: Protocol> Client<P> {
    fn call_with_trailer(&self, request: P::Request) -> TrailerSend<Call<P>>;
}
impl<R> CallResult<R> {
    fn into_response_trailer(self) -> (R, Option<TrailerRecv>);
}
impl<P: Protocol> CallContext<P> {
    fn request_trailer(&mut self) -> Option<&mut TrailerRecv>;
    fn respond_with_trailer(self, response: P::Response) -> TrailerSend<()>;
}
```

`TrailerSend<T>` implements `AsyncWrite`; `finish(self) -> T` completes the
trailer and returns the wrapped value (`Call<P>` or `()`). `TrailerRecv`
implements `AsyncRead`.

Internally, sends stage through a `BytesMut` buffer below
`trailer_send_copy_threshold` and switch to zero-copy vectored writes above
it; receives use `trailer_recv_copy_threshold` and
`trailer_recv_demand_copy_threshold` the same way on the read side. All three
thresholds, plus `max_trailer_size`, `max_incomplete_messages`, and
`max_incomplete_trailers`, are `Limits` fields configured through
[`Builder`](#session-establishment). Per-message and connection-wide limits
cover both the postcard payload and the trailer.

Large messages and trailers are fragmented and interleaved by message ID.
Each fragment header carries a flags byte:

```text
FIRST   0b0001  # first fragment of a message
LAST    0b0010  # commits/completes the message or trailer
ABORT   0b0100  # discards an incomplete message or trailer with an error
TRAILER 0b1000  # fragment carries trailer bytes, not postcard payload
```

The first fragment of a message carries its kind and postcard payload;
`FIRST | LAST` together is a complete unfragmented message, so ordinary small
calls keep the one-frame fast path with no reassembly bookkeeping at all. A
trailer cannot complete within the message's `FIRST` fragment (`FIRST | LAST
| TRAILER` together is rejected as malformed) — a trailer-bearing message is
always dispatched to the application (request handler or waiting `Call`)
before its trailer, if any, is known to be complete, and the trailer itself
always ends with a separate zero-length `TRAILER | LAST` fragment even when
the preceding data fragment contained the trailer's last byte, so the
message is never committed while a caller-visible `TrailerSend`/`TrailerRecv`
transfer could still be outstanding.

The sender-side scheduler sends one bounded fragment from each active message
before revisiting the queue, so a bulk transfer cannot starve small calls or
control messages; `Cancel`/`Error`/`ABORT` fragments are queued ahead of
ordinary ones. It also tracks whether each write completed atomically and
shrinks its target fragment size after a short write, growing it back after
subsequent atomic ones — this keeps fragment sizes adaptive to what the
transport can actually accept in one write, and is intentionally
future-extensible to a peer-signaled throttling hint.

The receiver retains incomplete assemblies by message ID, bounded by
`max_incomplete_messages`/`max_incomplete_trailers`/`max_fragment_size`/
`max_payload_size`/`max_trailer_size`. `LAST` dispatches a request or
completes a response; `ABORT` discards an incomplete request or completes an
incomplete response with an error. Unknown, duplicate, and
terminally-completed fragment sequences are protocol errors or defined
late-message no-ops as appropriate.

A receiver that no longer wants an in-progress trailer (e.g. the application
dropped it) does not need to send anything immediately — it becomes an issue
only if the peer keeps sending `TRAILER` fragments for that ID, at which point
the receiver sends a `Discard { id }` notice once. `Discard` is advisory: it
never changes the outcome of the request or response it names, it only tells
the sender it can stop spending bandwidth on a trailer nobody will read.

Unix messages carrying `SCM_RIGHTS` ancillary data remain unfragmented. The
association between descriptors, stream fragments, and interleaved message
assemblies is too platform-dependent to make fragmentation apply there. The
sender queries whether serialization attached descriptors and selects the
atomic path instead. Such messages remain subject to the ordinary maximum
frame size.

## Cancellation

A client may cancel any of its still-pending request IDs through
`Call::cancel(&mut self)`. The operation is non-consuming and idempotent: it
sends at most one `Cancel { id }`, while the same `Call` future remains
awaitable. Cancellation is advisory. Its result is observed through that future,
not through a separate acknowledgement channel: it resolves as an error just as
a connection failure would. Dropping `Call` sends the same cancellation request
best-effort, but leaves no caller to observe its result.

`Server::serve` tracks each handler task by its request ID. On `Cancel`, it
signals the request context. If a `cancel_guard` is active, that guard drops
its inner future and returns `RequestCancelled` to the handler. The handler
then decides how to clean up and what response to send. If no guard intercepts
the cancellation, the server aborts the handler task and writes
`Error { id, Cancelled }` only after the task has finished, so its future has
been dropped before the caller observes `Err(Error::Cancelled)`.

Cancellation races normally with completion. If the handler completes before
the cancellation takes effect, the request receives its normal `Response`.
If an unguarded cancellation wins, it receives `Error { id, Cancelled }`; if a
guard intercepts it, the handler may instead return its chosen `Response`. A
late or unknown `Cancel` is a silent no-op; it does not create a second
terminal message for the original request.

Cancellation is transport-independent. It applies equally to TCP, stdio, and
local attachment-capable sessions.

## Serialization Context

Direct handles require per-frame state while values are serialized and
deserialized. The crate passes this state explicitly through wrapper
`Serializer` and `Deserializer` implementations and serde seeds. The wrappers
use transactional handle contexts supplied by the transport; application
payload types never access the transport directly.

Serialization obtains the transport's associated `Send` value. Serializing an
`OsHandle` calls the platform-appropriate attachment method, which stages or
copies the native handle and returns its wire representation. Unix descriptor
attachment returns a queue index. Windows handle attachment returns the actual
peer-local `HANDLE` value at pointer width. A self-by-value finishing method
takes the complete header-and-payload buffer and sends it with all staged
handles. Dropping a Unix `Send` before finishing clears its staged descriptors.
Windows server-to-client duplication is immediate. The send frame records each
peer-local result and makes a best-effort attempt to close them if the frame is
dropped during serialization; once transmission starts, delivery is ambiguous
and cleanup is deliberately disarmed.

Unix `Send` retains `BorrowedFd`s for its full lifetime rather than duplicating
them. Requests and responses are therefore moved to the writer task and kept
alive until the consuming send operation completes. The contextual serializer
is the only unsafe bridge from serde's erased raw descriptor representation to
the frame lifetime.

The transport accumulates received native handles internally as reads
complete. The receive loop obtains an associated `RecvFrame` value before
reading a frame and retains it through deserialization. The contextual
deserializer takes handles directly from this value. Handles received early for
a later frame remain in the receiver. On Unix, dropping `RecvFrame` removes the
prefix through the largest descriptor index taken while decoding; any
unconsumed descriptors in that prefix are closed. No attachment count is
needed.

This replaces thread-local queues of sent and received descriptors. Explicit
context makes nested serialization, cancellation, tests, and concurrent
sessions tractable, and it makes the association between a payload and its
attachments unambiguous.

## Direct Native Handles

The type for a directly usable native OS resource is `OsHandle<T>`, whose
default parameter is the platform alias `DefaultHandle` (`OwnedFd` on Unix and
`OwnedHandle` on Windows). The name is deliberately descriptive rather than
capability- or rights-oriented: an open OS handle often has security
significance, but the type represents a local resource, not an application
authorization scheme. It lets platform-neutral protocol definitions use plain
`OsHandle` while code which borrows or wraps a resource can specify another `T`.

`OsHandle<T>` is serializable only on a direct-handle transport. Attempting to
enqueue or dequeue a native handle through the generic byte-stream transport is
API misuse and panics. Applications that need transport-independent resource
access use `Opaque<M>` instead.

On Unix, a local Unix-domain socket transfers descriptors with `SCM_RIGHTS`.
The Unix transport owns an `AsyncFd` for the socket, performs `sendmsg` and
`recvmsg`, and maintains the descriptor queues. The kernel's association
between ancillary data and stream fragments is OS-dependent: once a complete
frame has been received, all of its descriptors are assumed to be available,
while descriptors received early for the next frame remain queued.

Unix descriptor indexes must be honored rather than treated as traversal-order
checks. A serialization format or custom serde implementation may deserialize
handle fields in a different order than it serialized them. The receive state
therefore keeps stable indexed slots, allowing `RecvFrame` to take the
descriptor at the requested index without shifting the remaining indexes.
Dropping the frame removes its consumed prefix; descriptors already received
for a later frame remain queued.

macOS requires extra care: received descriptors cannot be atomically marked
close-on-exec by the kernel. The implementation should coordinate descriptor
receipt with process creation through a process-wide lock and `pthread_atfork`
handlers, setting `FD_CLOEXEC` before receipt leaves the critical section.
This requires process spawning/forking to observe the same discipline.

On Windows, transfer is role-directed and happens synchronously during
serialization or deserialization:

- Client to server: the client writes its local `HANDLE` value; the server
  duplicates it from the client process into the server process while decoding.
- Server to client: the server duplicates its local handle into the client
  process while encoding, then writes the resulting client-local `HANDLE`
  value; the client adopts that value while decoding.

This model supports privilege-separated VFS deployments where the client
cannot open the more privileged server with `PROCESS_DUP_HANDLE`. The server
holds the process-handle rights needed to duplicate in both directions. The
named-pipe endpoint role is independent of the RPC role and is used only to
discover the peer process ID.

Windows named-pipe client construction (`Builder::client_named_pipe_server`/
`client_named_pipe_client`, both `unsafe fn`) takes ownership of a trusted
peer process handle. It verifies that the process ID represented by the
handle matches the process ID reported by the named pipe, then retains the
handle for the lifetime of the session. The handle must grant query and
synchronization access so a future shared-memory handle-transfer
implementation can fence cleanup on peer process exit.

The Windows client retains each outbound request value until its correlated
response or error arrives, ensuring any process-local handle values remain
valid while the server decodes them. After a connection failure, those values
remain retained until the client session itself is dropped so a racing server
decode cannot observe a prematurely closed handle.

V1 does not acknowledge server-to-client handle adoption. The send frame
records handles duplicated during serialization and makes a best-effort
attempt to close them in the client process if serialization fails. Once
transmission begins, the client-local handle may remain open until the client
process exits. This bounded leak is safer than remotely closing an ambiguously
delivered handle whose numeric slot may have been reused.

Winsock socket duplication has different mechanics from `DuplicateHandle` and
is out of scope for the first version. The attachment codec should nevertheless
be extensible by attachment kind so it can be added without redefining ordinary
native-handle semantics.

### Future Windows Handle Reclamation

Acknowledging a duplicated Windows handle over the same fallible connection
does not safely establish when the sender may close it. A lost acknowledgement
leaks, while treating a lost acknowledgement as failure can close a numeric
handle slot after the peer adopted it and reused that slot for an unrelated
resource. V1 therefore accepts bounded server-to-client leaks after ambiguous
transmission failures.

A future local transport can close this race with a shared-memory ring of
atomic handle slots. Two sentinel values distinguish empty and reserved slots.
The server reserves a slot, passes the address of that shared slot directly as
`DuplicateHandle`'s output location, and publishes the completed fragment. The
client clears a slot only after successfully adopting its handle. Handle wire
representations may use the slot index, or use the handle value with a slot
lookup.

On connection failure, the client first waits for the server process handle to
be signaled, fencing any in-progress `DuplicateHandle` and shared-memory
writes. It can then close every slot which is neither empty nor reserved and
clear the ring. This is why named-pipe client construction retains an owned
peer process handle with synchronization access. The reserved state is never
treated as an owned handle during cleanup.

No shared ring is needed for client-to-server transfer in the intended
privilege-separated deployment. The privileged server receives the client's
numeric handle value and duplicates it into its own process while decoding;
it does not create an untracked handle in the less-privileged process. Only
the more privileged endpoint is assumed to possess the rights needed to open
the other process for duplication.

### Future Socket Transfer

Sockets should use a distinct `OsSocket` protocol type rather than pretending
all sockets are ordinary `OsHandle`s. On Unix, `OsSocket` can use the existing
`SCM_RIGHTS` descriptor mechanism. Winsock sockets require their bespoke
duplication protocol, such as `WSADuplicateSocket` protocol information and
reconstruction with `WSASocket`; transferring the raw `SOCKET` value or using
`DuplicateHandle` does not preserve the necessary provider state. This remains
deferred until networking itself is virtualized. Unix socket pass-through for
chained local VFS connections continues to use ordinary descriptor transfer.

## Opaque Objects

`Opaque<M>` is a session-scoped reference to an object owned by one endpoint.
It is always serializable: its wire representation is only the owning role and
a never-reused `u64` ID. It can therefore cross local, stdio, and TCP sessions.
On the non-owning endpoint it is a proxy identity used in RPC requests; it does
not expose the underlying object.

`M` is a public marker type shared by the protocol. The concrete server-side
object can remain private, or even belong to a crate which cannot implement a
trait for the public marker because of Rust's orphan rules.

```rust
trait OpaqueResource: Send + Sync + 'static {
    type Marker: ?Sized + 'static;
}

// Available on `CallContext<P>`:
fn register<T: OpaqueResource>(&self, value: T) -> Opaque<T::Marker>;

fn acquire<T: OpaqueResource>(&self, value: Opaque<T::Marker>)
    -> Result<OpaqueGuard<T>, InvalidOpaque>;

fn unregister<T: OpaqueResource>(&self, value: Opaque<T::Marker>)
    -> Result<Option<T>, InvalidOpaque>;
```

`Opaque<T::Marker>` supplies the static protocol-level type. `acquire::<T>`
also checks the registered concrete `TypeId`: several concrete types may
accidentally share a marker, so the associated-type equality alone cannot prove
the downcast is valid.

The owner stores each value as an erased, reference-counted object together
with its concrete `TypeId`. Acquiring an object retains the entry before
returning a typed `OpaqueGuard<T>`. Unregistering removes the table's public
reference and returns the resource when no acquired guard still shares its
ownership, or `None` when one does. Thus a concurrent acquire either fails
with `InvalidOpaque`, or succeeds and its guard keeps the concrete object
alive until the guard drops.

Opaque lifetime is an application convention: the owner explicitly registers
and unregisters objects. Receiving, copying, or dropping an `Opaque<M>` does
not change its lifetime and does not generate a protocol message. A malformed,
stale, or already-unregistered ID is safely rejected by the table lookup and
concrete `TypeId` check with `InvalidOpaque`.

Opaque references are the basis for fully remote file and socket-like APIs:
the owner registers an object, the peer receives `Opaque<FileMarker>`, and
subsequent read/write/close operations are ordinary RPC calls. A protocol may
choose direct `OsHandle<T>` transfer for a local capable session and fall back
to `Opaque<M>` for a remote one.

## Transport Abstraction

A transport connection is split into `transport::Sender` and
`transport::Receiver` halves, both crate-internal (`pub(crate)`) — no part of
the transport layer is public API; applications only ever see `Client<P>`/
`Server<P>`/`UnboundClient`/`UnboundServer`. The session writer owns the
sender and the receive loop owns the receiver, eliminating session-level
transport mutexes and `Arc` wrappers. A backend may still share one
internally synchronized full-duplex descriptor through an `Arc`, as the Unix
`AsyncFd` and Windows named-pipe implementations do, while a stdio
implementation may use unrelated output and input streams. The sender does
not synchronize multiple frame writes; its session task remains the sole
frame writer.

`Sender` exposes an associated transactional `Send` type whose consuming
`finish` method writes the completed frame. `Receiver`
exposes an associated `RecvFrame<'_>` type which performs both byte reads and
native-handle dequeues while preserving one frame's descriptor-index origin.
Closed internal enums (`AnySender`/`AnyReceiver`/`AnySend`/`AnyRecv`) select
and delegate to byte-stream (stdio, TCP, or any other `AsyncRead`/
`AsyncWrite`), Unix socket, and Windows named-pipe implementations without
making the generic buffer methods object-safe. All frame kinds, including
`Negotiate`, `Discard`, and fragmented/trailer-bearing messages, flow through
this same abstraction regardless of backend.

Transport support is summarized below:

| Transport                                         | Framing/RPC | `Opaque` | `OsHandle`            |
| ------------------------------------------------- | ----------- | -------- | --------------------- |
| Separate stdio pipes                              | yes         | yes      | no                    |
| TCP stream                                        | yes         | yes      | no                    |
| Unix-domain socket                                | yes         | yes      | yes, via `SCM_RIGHTS` |
| Windows local transport with peer process handles | yes         | yes      | yes, via duplication  |

Capability negotiation is useful when a transport can vary at runtime. It is
not a substitute for platform checks: the Windows implementation must also
have the peer process handle and the required duplication rights.

## Non-Goals For The Initial Version

- Generated IDL, request enums, and per-request response typing.
- Server callbacks or a bidirectional application RPC model.
- Winsock socket transfer.
- Borrowed (non-owned, non-`'static`) payload trailers.
- Shared-memory reclamation of ambiguously transferred Windows handles.
- Exactly-once delivery or distributed ownership certainty after connection
  failure.
- Making direct handles work over remote transports.

The current implementation includes the session core, handshake with
application-protocol negotiation, staged `Builder`/`UnboundClient`/
`UnboundServer` construction, explicit serde context, request/response
multiplexing, message fragmentation, streaming payload trailers,
opaque-object table, Unix descriptor transfer, and role-specific Windows
named-pipe handle transfer.
