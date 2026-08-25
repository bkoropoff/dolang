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
[Payload Trailers](#payload-trailers-and-fragmentation). It also carries the
response's [payload quota](#payload-quota), released when it is decomposed or
dropped, or handed to a caller that wants to release later through
`take_payload_credit(&mut self) -> PayloadCredit`.

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
(`max_fragment_size`, `max_payload_size`, `max_outstanding_payload`,
`trailer_session_window`, `trailer_credit_interval`,
`trailer_recv_copy_threshold`, `trailer_recv_demand_copy_threshold`,
`trailer_send_copy_threshold`, `max_concurrent_calls`) override individual size
and concurrency limits;
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
    fn trailer(&mut self) -> Option<&mut TrailerRecv>;
    fn trailer_manual_credit(&mut self) -> Option<&mut TrailerRecv>;
    fn respond(self, response: P::Response);
    fn respond_with_trailer(self, response: P::Response) -> TrailerSend<()>;
    fn release_payload(&mut self);
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
Cancel    { id }
Discard   { id }
Negotiate { id, payload }
Ack       { id }
Release   { id, count }
Credit    { id, count }
PayloadCredit    { count }
```

`Request` and `Response` provide ordinary RPC correlation. `Error` is a
terminal session-level failure for the correlated request, initially including
cancellation. `Cancel` controls a request already in flight. `Discard` is an
advisory, non-fatal signal that the receiver no longer wants a request's or
response's trailer; see
[Payload Trailers](#payload-trailers-and-fragmentation). `Negotiate` is the
handshake message described below, and must run to completion before any
other kind is valid on a connection. `Ack` confirms the boundary marked by
`WANT_ACK`. `Release` drops references to a session
[opaque](#opaque-objects); `Credit` returns
[trailer flow-control credit](#trailer-flow-control). Both carry a 4-byte
count and tolerate an unknown `id`, because in both cases the sender may
legitimately have retired the thing named while the message was in flight.
`PayloadCredit` returns [payload quota](#payload-quota) and is the one kind
with no `id` at all: quota is charged per message but released per call, and
dropping the attribution is what lets any number of retirements coalesce into
one count. Its `id` field is reserved and must be zero.

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
  blob per offered RPC version — for version 1, `HandshakeV1`, carrying the
  limits this endpoint enforces and its optional authentication digest — and
  `app_protocol` is this
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

### Pre-Shared Key Authentication

`Builder::key` makes negotiation prove, in both directions, that each peer
knows the same secret. This exists for transports whose access control cannot
identify the peer: a Unix-domain socket that must be world-connectable because
the uid that will connect is not knowable in advance (a VFS agent inside a
container, where the runtime's id mapping is not under our control).

Each side derives two digests from the key with BLAKE3's key-derivation mode
under role-specific contexts, advertises the one for its own role in
`HandshakeV1.key`, and requires the other role's. Verification happens as soon
as the peer's blob is decoded, before anything else it said is acted on, and
failure takes the same best-effort `ABORT` path as a version mismatch.

Two properties follow from the digests being one-way and role-separated, and
both are the point of the design:

- A peer that connects first and harvests the server's advertisement cannot
  derive the client's, so reaching the socket ahead of the intended client
  gains nothing.
- A peer that binds the socket first cannot produce the server's, so
  impersonating the agent fails too. Client-only authentication would leave
  this open, and it is not hypothetical: anyone whose uid maps onto the socket
  directory's owner can replace the socket.

Both digests ride the existing symmetric exchange, so authentication costs no
extra round trips — which is why it is a bearer proof rather than a
challenge-response. The consequences are worth stating plainly: there is no
nonce, so the exchange is not replay-resistant, and the session that follows
has neither integrity nor privacy protection. It is meant for keys minted per
launch and carried over a channel the peer cannot observe. The key is padded
to a fixed width rather than salted, so it must carry sufficient entropy on
its own; `AuthKey::new` enforces a 16-byte floor as a backstop, not as a
substitute for generating one properly.

Both ends must agree: keyed-to-unkeyed is refused from both sides
independently, so a configuration mistake fails closed rather than silently
dropping authentication. Neither `AuthKey` nor `HandshakeV1` derives `Debug` —
a derived digest authenticates its side as effectively as the key it came
from, so printing one is equivalent to printing the secret. Error messages
carry no key material for the same reason.

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
    fn into_response_trailer_manual_credit(self) -> (R, Option<TrailerRecv>);
}
impl<P: Protocol> CallContext<P> {
    fn trailer(&mut self) -> Option<&mut TrailerRecv>;
    fn trailer_manual_credit(&mut self) -> Option<&mut TrailerRecv>;
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
thresholds, plus `trailer_session_window`, `trailer_credit_interval`, and
`max_concurrent_calls`, are `Limits` fields configured through
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

`max_concurrent_calls` bounds calls in flight, counted from a request's first
fragment to its response. The client writer leaves excess requests unencoded
in its local queue and continues sending control fragments. A slot is taken
where the request reaches the scheduler and released on `Response`/`Error`, or
on an ordered abort for a request that never reached dispatch — never before
the request goes out, so a request that fails to encode cannot strand one.

On the server the same span has two custodians. The reassembler holds a call
while its payload is still arriving (`Reassembler::payload_incomplete`); the
server holds it from dispatch to the response head (`outstanding`). The two
are disjoint — a message leaves payload phase in the same `accept` call that
dispatches it — so the limit is on their **sum**, and neither count can
enforce it alone: separate budgets would admit twice the limit, and twice the
worst-case reassembly memory that sizes the default.

`check_call_admission` therefore takes both numbers, and runs at the two
points where the sum can grow: `Event::PayloadIncomplete`, which the
reassembler emits when a message opens a payload that will span more
fragments, and dispatch. A call moving from one custodian to the other leaves
the sum unchanged, so the transition never trips it. Single-fragment requests
never enter payload phase at all and are admitted at dispatch alone.

The client needs none of this. It answers no calls, and its header gate
(below) refuses any fragment that does not belong to a call it made, so its
reassembler holds at most one entry per live call — already bounded where
calls are issued.

### Header Gates

Each reader validates a fragment header against its own direction before
handing the frame to the reassembler, so an inadmissible fragment costs no
buffer at all. A client refuses any `Request`, and any first `Response`
fragment naming an id with no call outstanding — ids are minted locally, so
one that names no live call is fabricated. A server refuses any `Response`.
Later fragments are not gated: they name a message this end already accepted,
and the reassembler rejects an id it has no entry for.

Messages that have finished their payload and entered their **trailer phase**
are excluded from that count, on both the receiving `Reassembler` and the
sending `Scheduler`. A trailer may outlive its call and stream for as long as an
application keeps writing, so counting it would let a handful of streaming
transfers exhaust the limit — fatally on the receiver, and as indefinite
head-of-line blocking on the sender, which simply declines to promote further
sends. Trailer memory is bounded by
[credit](#trailer-flow-control) instead, which bounds bytes rather than a
count and so does not care how many trailers are open.

The receiver retains incomplete assemblies by message ID, bounded by
`max_concurrent_calls`/`max_fragment_size`/`max_payload_size`/
`max_handles_per_message`. `LAST` dispatches a request or completes a
response; `ABORT` discards an incomplete request or completes an incomplete
response with an error. Unknown, duplicate, and terminally-completed fragment
sequences are protocol errors or defined late-message no-ops as appropriate.

A receiver that no longer wants an in-progress trailer — because the
application dropped or discarded its `TrailerRecv` — sends a `Discard { id }`
notice immediately, exactly once. `Discard` is advisory: it never changes the
outcome of the request or response it names, it only tells the sender to stop
spending bandwidth on a trailer nobody will read.

The notice must be eager, because a sender parked on exhausted credit emits no
further fragments: waiting to notice an unwanted `TRAILER` fragment arriving
would mean never noticing, and an abandoned trailer would leave its sender
parked forever instead of aborting. Under manual release this is also the only
thing distinguishing "the consumer is holding bytes it has not released yet"
from "the consumer is gone" — from the sender's side the two look identical.

Unix `SCM_RIGHTS` descriptors are attached to postcard fragments in serialized
index order. Each fragment carries at most the negotiated
`max_handles_per_fragment`, itself capped to the operating system's ancillary
data limit, and the receiver sizes its control-message buffer accordingly.
The reassembler accumulates descriptors per message across interleaved
fragments. If all postcard bytes have been sent before all descriptors, the
scheduler emits zero-payload postcard fragments until the attachment phase is
complete; only then may it complete the message or enter its trailer phase.
After postcard decoding, every accumulated descriptor must have been consumed
by an `OsHandle` field. Leftover attachments are a protocol error and are
closed with the rejected message.

`WANT_ACK` marks the final non-trailer fragment of a request or response. The
receiver queues an empty, single-fragment `Ack` for the message ID immediately
after reassembly accepts that boundary and before it starts consuming any
trailer. A message may request at most one acknowledgement; `Ack` itself cannot
request one. An unsolicited, duplicate, or late acknowledgement is a protocol
error.

On macOS, messages carrying file descriptors set `WANT_ACK` to work around XNU
collecting reachable sockets while processing `SCM_RIGHTS`. The scheduler moves
every successfully transmitted `OwnedFd` into message-ID escrow instead of
dropping it, without testing its descriptor type. Receipt of `Ack` releases the
escrow. Other platforms honor `WANT_ACK`; Windows does not yet use it for its
own outgoing handle escrow.

### Trailer Flow Control

Trailers share one session-wide credit pool. Across every trailer on the
connection, the peer may have at most `trailer_session_window` bytes
outstanding that this end has not **retired**, and a `Credit { id, count }`
fragment returns retired bytes to it. A sender that exhausts the pool parks
and resumes when credit arrives; there is no cap on a trailer's total size, so
a trailer may stream indefinitely.

Retiring is deliberately later than reading. Bytes that have crossed into the
consumer's buffer still occupy memory until the consumer has flushed them
wherever they are going, so a consumer that reads a chunk and then blocks
writing it to a slow disk has moved the bytes without freeing them. Crediting
on retirement makes the pool bound end-to-end receiver memory — the receive
buffer plus whatever the application still holds — and makes the sender's rate
track the destination's real drain rate rather than the receiver's willingness
to buffer. The transfer becomes self-pacing.

By default every byte is retired as it is read, which is what an ordinary
consumer (`read_to_end`, `io::copy`) wants and requires no participation.
Obtaining the trailer through `CallContext::trailer_manual_credit` or
`CallResult::into_response_trailer_manual_credit` opts into explicit
`release(n)` instead, for consumers that care about that pacing. The mode is
fixed where the trailer is obtained rather than switchable afterwards, so no
trailer is half auto-credited. Manual release moves one hazard into calling
code, and it is loud in the API docs: never wait to accumulate more credit
than the pool holds before releasing, or the consumer deadlocks against
itself.

A request trailer is *taken* from its `CallContext`, not borrowed, so a
handler may go on reading after it has responded: the reassembler entry, the
credit route and the peer's send all outlive the handler already. `respond`
discards only a trailer the context still holds.

Taken together with `respond_with_trailer`, that makes one call a duplex byte
pipe. Each direction is an independent stream, and the call is complete once
the response head is out — the server drops its `outstanding` entry in
`respond_with_trailer`, and the client drops its call slot on the `Terminal`
that follows the response. So a pipe holds no call slot in either direction,
and its cost is trailer credit rather than `max_concurrent_calls`. Nothing
couples the two halves: closing one leaves the other open, there is no
call-level cancellation left to send, and a peer that disappears is noticed
through the transport or through a credit stall. That is the same contract a
socket offers, and code layered on top should own the pairing itself.

#### One Pool, No Per-Trailer Window

The pool is the only credit limit trailers have — postcard payloads are metered
separately, see [Payload Quota](#payload-quota). A sender that lets one trailer
consume the whole pool starves only its own other trailers. Since `Credit`
indicates which message ID is responsible for returned quota, the sender can
always attribute quota consumption to its individual streams and apply
per-stream limiting as local policy.

That stays safe only because the receiver never assumes the pool is the sole
reason a sender might be parked. A private per-stream budget is invisible on
the wire — nothing announces it, and `trailer_credit_interval` is not
advertised either, so the sender could not size one against the peer's
coalescing granularity even if it wanted to. So the receiver treats its own
consumer waiting on bytes that have not arrived as sufficient reason to flush,
whatever the threshold says: see the third clause below.

#### Coalescing

Credit is emitted once half a `trailer_credit_interval` has accumulated, *or*
when the trailer ends, *or* whenever the peer might be parked waiting for it:
the pool is exhausted, or this trailer's own consumer is blocked with nothing
staged. Only the first is an optimization. Without the end-of-trailer flush a
transfer that finishes below the threshold would strand its pool debt for the
life of the connection. Without the exhaustion clause a consumer that retires
less than the interval and then waits for more data would deadlock against a
sender parked on an empty pool. Without the stalled-consumer clause the same
deadlock returns for a sender parked on a per-stream budget of its own, which
the receiver has no way to observe directly — the stall is the only symptom it
shares with every other reason a sender stops sending.

The stall clause fires at both edges: when a consumer goes from reading to
waiting (flushing whatever had accumulated), and when credit is retired while
it is already waiting (which is where a manual consumer's `release` lands).

`trailer_credit_interval` is purely local coalescing granularity — it is not
negotiated and bounds nothing, so the two ends need not agree on it and no
value can stall a sender.

#### Backstops And Ledger

The pool is enforced connection-fatally on the receiver as a backstop against
a peer that ignores what it agreed to; a well-behaved sender parks rather than
overrunning. Negotiation floors it at 1. Only zero deadlocks — the sender
would park before its first byte, and no credit could ever arrive for bytes
that were never sent. A pool below `max_fragment_size` is legal and merely
produces short fragments.

On the send side, credit is *reserved* under the pool's own lock rather than
read and then spent, because the pool is shared across trailers holding
different `SendShared` mutexes and a separate read-then-debit would let two
writers spend the same bytes. A reservation taken before requesting a
transport grant is carried across the wait, so a granted trailer always has
credit in hand and never has to park while holding a lease — parking with a
live lease would wedge the connection's single writer.

Outstanding pool bytes are tracked per message id in a small ledger rather
than on the `SendShared`/`RecvShared` that spends them, because settlement
outlives those objects. A trailer's last `Credit` fragments routinely arrive
after its send has finished and left the scheduler; charging the debt to the
send would drop those refunds and shrink the pool a little on every completed
transfer, until eventually nothing could be sent at all. Keying by id also
makes settlement idempotent, so an abort that returns a whole debt at once
cannot be double-counted by a `Credit` that crossed it on the wire.

That ledger is what lets `Discard` refund implicitly. When a consumer
abandons a trailer it sends `Discard { id }` and stops charging for that id,
including for fragments already in flight behind the notice; the sender
receives it, returns the id's whole remaining debt, and drops the entry.
Both ends reach zero without either counting the bytes on the wire, because
`Credit` and `Discard` travel the same direction on one ordered stream, so a
`Discard` can never overtake a credit the sender has not yet applied. The
alternative — crediting the racing fragments explicitly — would require the
sender to keep a dead trailer's ledger entry alive until every byte it ever
sent had been credited, with no crisp point at which it could be dropped.

## Payload Quota

`max_payload_size` bounds one message and cannot bound the sum. Multiplied by
`max_concurrent_calls` it is the whole reassembly footprint a peer can demand,
and a peer reaches it cheaply by opening that many messages and sending one
fragment of each. `max_outstanding_payload` bounds the sum directly: total
charged postcard bytes across every call that has not yet released.

Trailers keep no size cap because they are streamable and can be paced
incrementally. Payloads cannot — a payload has to be reassembled whole before
it can be deserialized — so a cap is the only bound available, and the two
rules invert.

The quota measures wire bytes. The deserialized form is `O(serialized size)`,
so that is an adequate proxy, but postcard is compact and a struct-heavy
payload can land at four to eight times its wire size once padding and
per-node overhead are counted. The default is chosen knowing that.

### The Whole Call, Not Just Reassembly

Counting buffered bytes across incomplete messages and releasing at dispatch
would bound only the reassembly buffers. A payload's memory does not end at
dispatch; it ends when the application is done with it. Dispatched payloads
would still be bounded by `max_concurrent_calls` × `max_payload_size` — the
exact product this limit exists to escape. It would close the cheap attack
without lowering the peak.

So the quota is charged for the whole call lifecycle and released when the
application is done with the payload. That makes it a byte-denominated
concurrency bound, which is the real prize: `max_concurrent_calls` can be
generous — the default is 1024 — without also admitting 1024 large calls.

### Charge At Admission

The sender subtracts a message's **full** payload size when it moves from
`waiting` into `active`, before its first fragment goes out, and does not
restore it until the peer's release arrives. A message starts only if its
whole payload fits in the remaining credit; otherwise it waits, unstarted,
holding nothing, and cancellable with no wire trace.

Charging at admission rather than incrementally is what removes the deadlock
class entirely. Incremental charging lets several messages reach a
partially-sent state that no remaining credit can drive to completion,
recoverable only by cancelling and reissuing them — and nothing releases
credit until something completes, so nothing completes. Charge-at-admission
makes that unreachable by construction: anything started can always be
finished.

It costs almost nothing in utilization, because under end-of-use release a
fully-sent message holds its entire payload against the pool anyway. The only
window where "reserved" differs from "buffered at the peer" is the
transmission itself. Nor does it reduce interleaving: a 16 MiB pool admits
eight 2 MiB messages at once, which round-robin among themselves exactly as
they would unconstrained.

The sender's reservation therefore precedes the receiver's accounting and is
released after it, so the safety argument does not depend on transport
ordering the way a dispatch-time scheme would.

Cancellation settles in two parts, because the two halves come back from
different places. What was earmarked but never transmitted is buffered nowhere
and is reclaimed locally, immediately. What did reach the wire is in the
peer's reassembler, and only the peer can say when it is gone — it credits
that part back when it retires the aborted message. Together they are exactly
what was charged.

### Release

Every release path is **drop-driven, not call-driven**. A dropped
`CallResult`, a `CallContext` whose handler panicked, a handler that returns
without responding — each must release, and does, because each drops the
charge that travelled with the message. A missed release is not a delayed
release; it is a permanent subtraction from the pool, and enough of them stall
the connection with no diagnostic. There is no reconciliation protocol worth
building to recover from that — a session-level "ids I am still charging you
for" exchange is far more machinery than the feature — so the drop paths are
the entire defense.

- **Server**: `CallContext::release_payload` releases explicitly; otherwise the
  context's drop does it, whichever way the call ends.
- **Client**: `CallResult` releases when decomposed or dropped, or
  `take_payload_credit` extracts a `PayloadCredit` token to hold and drop
  later. The token is where this is easiest to get wrong, since it is the one
  release that outlives the obvious scope.
- **Cancelled before the last fragment**: no charge was ever handed out, so the
  reassembler settles the id itself on the `ABORT` path.

Settlement is keyed by message id in the same ledger the trailer pools use, and
`settle` clamps to the recorded debt, so releasing twice cannot over-credit.

### Two Pools, One Mechanism

Payload quota and trailer credit are separate pools sharing `SessionWindow` as
a mechanism. Merging them is a deadlock class, not a tuning mistake.

Take the ordinary streaming-upload shape: a small descriptor payload, a large
trailer, and a handler that reads the trailer to completion before responding.
The payload's charge is held until the handler completes; the handler cannot
complete until it has consumed the trailer; the trailer needs credit. One such
call is fine because its own payload is small. `N` of them whose payloads sum
to a shared pool all wedge: each holds payload quota, each needs trailer
credit, and none can release. Pricing a trailer fragment into the admission
cost would guarantee only that the trailer can *start*, not that it can
continue. Closing it by contract — "a handler consuming a trailer must release
payload quota first" — is a rule whose violation deadlocks rather than
degrades, on the most common trailer pattern there is.

Two further reasons, independent of the deadlock:

- **The dynamics do not mix.** Trailer credit recycles continuously as the
  consumer reads; payload quota is held long and released once. Shared,
  streaming throughput becomes hostage to unrelated calls' response latency.
- **It breaks a documented property.** A sender letting one trailer consume
  the pool is supposed to starve only its own other trailers, which is what
  makes "no per-trailer window" defensible. Shared, a bulk trailer would
  starve call admission too.

The cost is that total peer-attributable receiver memory is the sum of two
numbers rather than one.

### Credit, Enforcement, And Scheduling

The negotiated `max_outstanding_payload` is the initial credit, and the rest is
the same shape as trailer credit. `PayloadCredit` carries only a count — no
`id` — so returns coalesce into a single number however many calls retired;
the scheduler merges any already queued into one fragment, and gives it the
same priority as `Credit`, since the peer may be parked with nothing. There is
no coalescing *threshold*: a release is already coarse, so credit is flushed on
every one, and no threshold means no threshold-induced deadlock.

The receiver enforces the aggregate where it already enforces the per-message
cap, right before it extends the reassembly buffer, and a breach is a protocol
error exactly like the others — fatal to the connection. There is no flow
control to fall back on: this is the backstop against a peer that ignores the
credit it was issued, not a mechanism the healthy path is expected to touch.
Making it a credit loop is also what keeps the check honest, since it fires
when the peer exceeds *issued* credit rather than because the local
application was slow to release.

Negotiation keeps `max_outstanding_payload` at least `max_payload_size` in
both directions — raised before the handshake is built, and `max_payload_size`
lowered to it afterwards — because `clamp_limits` mins each field
independently, so a peer with a small quota can break the relationship even
when both endpoints are individually valid. That is a peer decision, not a
misconfiguration, so it is normalized rather than rejected.

Scheduling is deliberately minimal: FIFO admission out of `waiting`, which is
starvation-free on its own, unchanged round-robin among admitted messages, and
a genuine park when nothing can be admitted. Exhausted quota is a park rather
than a reorder, which splits one question the scheduler used to answer once.
`ready` must be polled while a send waits on quota, since that poll is where
it registers on the pool — gate it on "is there work" and the credit that
would start the send arrives with nobody listening. That splits the scheduler's
old single "is there anything to do" question into two: `has_work`, which
counts only what has already been committed to the wire, and `has_pending`,
which also counts sends parked on quota. Which one is the *drain* condition
depends on whether the receive half is still there to deliver credit, and that
is what [Shutdown And Draining](#shutdown-and-draining) is about.

Control fragments and trailer-phase sends are never gated by the quota: a
handler blocked on an inbound trailer cannot complete, and therefore cannot
release. A trailer producer, symmetrically, must reserve no *trailer* credit
until its own message has been admitted — a trailer fragment can never precede
its payload, so credit taken before then is held for bytes that cannot move,
and several unadmitted messages can between them hold the whole trailer pool
against the started messages whose completion is the only thing that would
free the payload quota they are waiting for. That is the one place the two
pools would otherwise still meet.

### The Contract This Creates

Holding the pool for the whole call means a long-pending call with a large
payload throttles the connection. Indefinitely pending calls are legitimate —
an event poll is the usual shape — and a large payload on one is unusual but
not unthinkable. This is documented as a caveat with an escape hatch rather
than designed around: **release explicitly if you are going to pend for a long
time, or don't mix large payloads with slow responses.** Violating it degrades
throughput. It does not hang anything.

## Shutdown And Draining

Each endpoint runs two long-lived futures, a receive driver and a send driver,
raced against each other. They stay at arm's length on purpose. Neither is
cancel-safe — the receiver holds bytes already consumed from the transport in
a partially read fragment, the sender holds a fragment partially committed to
it — so each has to be polled as a whole rather than stepped, and folding them
into one loop would mean a stalled write blocks reading, which deadlocks
against a peer that is itself blocked writing. Keeping them independently
pollable is what makes read progress and write progress independent.

That independence leaves exactly one thing neither can decide alone: when the
send driver is allowed to stop. Its own state says whether it still holds work;
only the receive half knows whether more work can still arrive, and whether the
peer is still there to return the credit a parked send is waiting on. So one
bit crosses between them — the drain mode, published by the receive driver over
a `watch` and observed by the send driver.

### Why It Cannot Just Drop The Send

Payload quota made the scheduler able to hold a message back indefinitely. A
send that does not fit the session budget waits for credit, and that credit
arrives through the *receive* half. A writer draining after its reader is gone
can therefore be holding a send that will never proceed.

Dropping it is not an option. A graceful shutdown promises that work already
accepted still finishes, and silently discarding a queued response breaks that
— the peer's call fails with a connection error for a response the server
had already produced. So the ordering has to be inverted relative to the
obvious one: the receive driver stays alive until the send driver has drained,
rather than the send driver being torn down once the receive driver ends.

### The Three Modes

`Running` is the live session. The send driver stops only if its channel
closes, which means every handle that could still queue work is gone.

`Graceful` is published by the receive driver once shutdown has been requested
*and* every dispatched call has answered. The send driver then finishes
everything it holds, `has_pending` rather than `has_work`, parked sends
included — the receive half is still running, so the credit that releases them
can still arrive. The condition also requires the outgoing channel to be
drained: a handler queues its response and *then* completes, and completing is
what seals the drain, so at the instant the signal lands the last response may
still be in the channel rather than in the scheduler.

An empty send side does not complete graceful shutdown. The receive driver
keeps reading until the peer closes its transport, which gives every control
message the peer queued before closing a chance to arrive without requiring
the server to enumerate them. The send driver remains alive as well, so it can
emit rejections and other control messages prompted by those final reads. The
client must therefore close its session after receiving the shutdown response.
That close drains its queued writes before releasing the transport.

`Abrupt` is published unconditionally on the receive driver's way out, and it
sticks — it cannot be downgraded. No further credit can arrive, so the send
driver flushes only `has_work`, what it had already committed to the wire, and
abandons the rest. This is what keeps a lost peer from turning into a hang, and
it is why the two predicates exist.

Between the request to shut down and the seal, the receive driver keeps reading
but stops taking on new calls: an arriving request is refused with an error
rather than dispatched, and dropping its charge returns its payload quota to
the peer — this end is still reading, so that credit is still worth sending.

### Termination Is The Caller's To Bound

`Graceful` is unbounded by construction: a peer that never closes its transport
will hold this end open indefinitely. The bound belongs to the caller, who can
wrap the endpoint's driving future in a timeout — dropping it aborts both
halves at once. This crate does not depend on a timer, so it does not impose a
policy of its own.

### The Client

`Client::close` rejects new calls and fails pending calls with
`ConnectionClosed`, then closes the writer's work channel. The writer drains
writes already committed to the wire and exits, closing the client-to-server
transport. The reader remains alive throughout that drain so it can deliver
credit, then continues until the peer closes the server-to-client transport.
Only after that natural EOF does `close` release the reader's shutdown sender.
Windows named pipes cannot half-close: both drivers own one duplex pipe handle.
For that transport, `close` keeps the reader alive through the writer drain,
then stops it so releasing the shared handle delivers EOF to the peer.

Like graceful server termination, client close is unbounded. Callers that need
a deadline apply their own timeout and call `Client::abort` if the peer does
not cooperate. `abort` performs the former abrupt behavior: it closes outgoing
work, fails pending calls, and signals the reader immediately. The writer still
finishes fragments already committed to the transport, but abandons sends
parked on credit that can no longer arrive. Dropping the last client handle is
also abrupt and remains non-blocking.

The reader shutdown sender is owned independently of the task join handles.
Consequently, an `abort` through one clone can interrupt another clone's
`close` while it waits for peer EOF.

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
deserialized. The crate installs the transport context in scoped thread-local
storage around each synchronous postcard call. `Opaque` and `OsHandle` access
that context directly from their serde implementations. A swap guard restores
the previous context after normal return or unwinding, so nested serialization
uses the innermost context. Using either type through serde outside an RPC
session panics.

Serializing an `OsHandle` transfers its owned handle into the transport context
immediately and writes the returned wire representation. Unix descriptor
attachment returns a queue index. Windows handle attachment returns the actual
peer-local `HANDLE` value at pointer width. Serializing the same `OsHandle`
again fails because it has already been emptied. Outgoing messages are owned by
the RPC writer and discarded on serialization failure; the state of such a
failed message is not part of the API contract. Windows server-to-client
duplication is immediate, while Unix retains the stolen descriptors until
attachment.

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

The Windows client retains the owned handles stolen from each outbound request
in a session escrow table until its correlated response or error arrives,
ensuring process-local handle values remain valid while the server decodes
them. Session failure drops the remaining escrow entries.

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

Protocol grant totals saturate rather than wrap. An owner entry that reaches
`u32::MAX` is immortal until the session ends; releases cannot make the
unrepresentable total finite again. The holder normally prevents this by
collapsing a mirrored total at `u32::MAX / 2` to one live reference and sending
one counted `Release` for the excess.

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
