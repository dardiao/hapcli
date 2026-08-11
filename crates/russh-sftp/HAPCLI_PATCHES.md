# hapcli patches for russh-sftp 2.3.0

This directory is a vendored fork, not a plain crates.io copy. It is based on
`russh-sftp` 2.3.0, upstream commit
`dcc0c06a2aa14da96fb453f05b530c8fffba1b1a`. That commit is recorded in the
published 2.3.0 crate's `.cargo_vcs_info.json`.

Before upgrading, compare this tree against that exact commit or the exact
crates.io 2.3.0 archive. Do not use a nearby tag or current upstream `master` as
the baseline. Preserve every transfer, lifecycle, and capacity contract below
unless the proposed upstream version provides equivalent behavior and tests.

## Why russh-sftp Is Vendored

hapcli's SFTP path needs behavior beyond upstream's stream-oriented file
API:

- bounded multi-request downloads and uploads for high-latency links;
- raw READ/WRITE encoders and `Bytes` payloads to reduce hot-path copies;
- adaptive per-file windows constrained by server-advertised limits;
- a byte-bounded, session-owned outbound transport with explicit request
  lifecycle semantics;
- owned writer integration with the vendored russh channel transport;
- capacity diagnostics and scheduling hints without identity-bearing data;
- linear directory accumulation and reliable remote-handle cleanup.

## Raw queued sequential downloads

hapcli downloads large files through SFTP. The upstream `File` type keeps the
standard `AsyncRead` implementation strictly sequential: one read request is
issued and awaited before the next request starts. That behavior is compatible
with stream semantics, but it makes high-latency downloads throughput-bound by
round-trip time.

This fork adds `RawSftpSession::read_nowait_raw`, mirroring the existing
`write_nowait_raw`, and a dedicated `File::into_pipelined_downloader_for_range`
path for bulk sequential downloads. It intentionally does not change the normal
`AsyncRead` implementation. The downloader owns the remote file handle, keeps a
bounded number of raw read requests in flight, buffers out-of-order responses,
and emits chunks only at the next contiguous file offset.

hapcli uses this path for normal and resumed downloads in
`crates/hapcli-sftp`. The effective request length still comes from the
server `limits@openssh.com` read limit or the configured packet cap, whichever
is smaller. hapcli currently passes a bulk download cap of 64 requests and
16 MiB of in-flight data.

Correctness notes:

- SFTP servers may return fewer bytes than requested. When that happens, the
  downloader discards the speculative window and restarts from the actual next
  offset so callers never skip a gap.
- `SSH_FXP_DATA` payloads are decoded into `bytes::Bytes` instead of `Vec<u8>`.
  This lets bulk downloads forward the packet body without copying it into a
  separate vector first.
- EOF marks the downloader as finished so repeated `next_chunk` calls do not
  issue extra read requests.
- Dropping or shutting down the downloader discards pending speculative reads
  and closes the remote handle.
- Scheduling failures discard already queued reads before returning the error,
  so callers do not consume stale responses after the session has failed.

## Raw queued sequential uploads

hapcli uploads large files through a dedicated
`File::into_pipelined_uploader` path. Upstream `AsyncWrite` already queues
`write_nowait` requests, but the high-level stream interface only exposes
stream-style writes and session-wide packet/concurrency knobs. Changing those
knobs to optimize downloads also changes upload packet sizing, which can
regress throughput through SSH channel-window or server-side write backpressure.

This fork keeps ordinary `AsyncWrite` semantics unchanged and adds an
upload-owned writer that:

- writes explicit SFTP offsets from a caller-provided start offset, including
  resumed uploads;
- limits both request count and in-flight bytes;
- accepts write acknowledgements in any response order;
- drains all acknowledgements, fsyncs when supported, and closes the handle on
  `shutdown`;
- closes the handle on drop without pretending already queued writes completed.

hapcli currently uses this path for normal and resumed uploads with a
64-request / 16 MiB in-flight cap. The effective write size still honors the
server `limits@openssh.com` write limit or the configured packet cap.

## Raw WRITE packet encoding

The upstream `write_nowait` path accepts owned `Vec<u8>` data and routes it
through the generic `Packet::Write` serializer. For bulk upload chunks this
costs extra copies: the caller first copies the buffer slice into a `Vec`, the
serializer copies that data into a payload buffer, and the packet wrapper then
copies the payload into the final outgoing frame.

This fork adds `RawSftpSession::write_nowait_raw(handle, offset, data)`, which
builds the SFTP v3 `SSH_FXP_WRITE` frame directly into the final `BytesMut`.
The raw path still uses the same request IDs, server packet/write limits,
request map, acknowledgement handling, and outgoing channel as the generic
path. A unit test verifies that raw WRITE packet bytes match the generic packet
encoding exactly.

Both `File::into_pipelined_uploader` and ordinary `AsyncWrite` now use the raw
encoder for write requests. This changes allocation behavior only; write order,
offset handling, acknowledgement draining, fsync, and close semantics are
unchanged.

## Raw READ packet encoding

The pipelined downloader schedules many small `SSH_FXP_READ` requests. Routing
each request through the generic `Packet::Read` serializer clones the remote
handle and allocates an intermediate packet payload for every queued read.

This fork adds `RawSftpSession::read_nowait_raw(handle, offset, len)`, which
builds the SFTP v3 `SSH_FXP_READ` frame directly into the final `BytesMut` from
a borrowed handle. The raw path keeps the same request IDs, server read/packet
limits, request map, response handling, and timeout behavior as the generic
path. A unit test verifies that raw READ packet bytes match the generic packet
encoding exactly.

## Bytes-backed DATA packets

The upstream DATA packet model stores `Data.data` as `Vec<u8>`, so decoding a
download response copies the packet payload even though the underlying incoming
frame is already a `bytes::Bytes` buffer. This fork stores DATA payloads as
`Bytes` and decodes `SSH_FXP_DATA` manually with `copy_to_bytes`.

The generic serializer/deserializer path is still used for small control
packets. DATA encode/decode has focused unit coverage to verify that packet
length, request id, and payload bytes round-trip correctly.

## Session-owned outbound transport

Upstream 2.3.0 runs outgoing packets through an unbounded sender and registers
only a response waiter. hapcli's `client::transport` module instead makes the
queue, byte budget, request map, cancellation token, reader task, and writer
task one ownership unit tied to a single live SFTP session.

The transport contract is:

- `Config::max_outbound_inflight_bytes` limits the combined bytes of locally
  queued and sent-but-unacknowledged frames. The default is 16 MiB.
- Each request holds its byte permit until acknowledgement, cancellation before
  send, or session termination. Queue depth alone is not treated as a memory
  bound.
- Raw WRITE reserves byte capacity before allocating and copying its final
  frame, so producers cannot accumulate encoded frames outside the budget.
- Poll-based writers register a capacity waker and retry only after permits are
  released or the session closes.
- `OwnedSftpWriter` accepts a `Bytes` frame directly. The vendored russh
  `ChannelStreamWriter::write_bytes(...)` can preserve that allocation while
  fragmenting it across SSH channel windows.
- `RawSftpSession::new_owned_with_config(...)` keeps the owned reader/writer
  path available without falling back to `tokio::io::split` and borrowed
  `AsyncWrite` buffers.

Every request has an explicit phase:

- queued;
- sent;
- acknowledged;
- cancelled before send;
- abandoned after send;
- disconnected before send;
- disconnected after send.

Dropping a queued request prevents its frame from entering SSH. Dropping a
request after sending cannot retract the remote operation, so the request entry
and byte permit remain until a late response or disconnect resolves the live
session. A request timeout closes that SFTP session because the remote outcome
is unknown and retaining an unreachable sent request would permanently consume
the byte budget.

Best-effort CLOSE packets are detached only after they enter the live session's
queue. If the byte budget is full, the session owns an async retry instead of
silently dropping the remote-handle close.

## Adaptive bulk transfer windows

The caller-provided 64-request / byte-window values are now hard caps rather
than the active window at every moment. `PipelinedFileDownloader` and
`PipelinedFileUploader` own a small `SftpWindowTuner` that records request
send time, DATA/ACK completion latency, completed bytes, and congestion events.

The tuner uses conservative additive-increase / multiplicative-decrease rules:

- successful DATA/ACK intervals grow target request count, in-flight bytes, and
  chunk length up to the server-limited caps;
- short reads, protocol errors, status errors, and closed response channels
  shrink the target request count, in-flight bytes, and chunk length;
- server `limits@openssh.com` and the caller-provided caps remain hard upper
  bounds.

The startup window intentionally begins below the cap, then clean early
intervals ramp faster than steady-state growth. This avoids overloading unknown
servers immediately while still filling high-RTT links quickly when there are no
short reads or status/protocol errors.

This keeps UI-level SFTP settings stable while letting each single-file
transfer adapt to RTT, ACK pace, server limits, and short-read/error feedback.

## Local-only window diagnostics

The pipelined downloader and uploader expose in-process diagnostic snapshots for
hapcli's local SFTP performance debugging. These snapshots contain only
numeric counters and small enums such as target request count, in-flight bytes,
chunk length, RTT summaries, shrink reason, short-read counts, queue state, and
ACK/capacity-wait counters.

The snapshot types intentionally do not derive serde serialization or broad
debug formatting. They do not include hostnames, usernames, node identifiers,
paths, filenames, server banners, raw error messages, or secret-bearing data.
hapcli may format them to the local stderr stream only when an explicit local
diagnostic environment variable is enabled.

## Directory listing and request lifecycle cleanup

The high-level `SftpSession::read_dir` path now appends each `SSH_FXP_NAME`
batch into the accumulated entry vector instead of rebuilding
`new_batch.chain(old_entries).collect()` for every packet. Large directories
therefore grow linearly instead of repeatedly copying previously collected
entries. The order now follows the server-provided `readdir` order.

If `readdir` returns an error after `opendir` succeeds, the session now attempts
to close the remote directory handle before returning the original error. This
keeps error reporting stable while avoiding server-side handle leaks on failed
large listings.

The session-owned transport registers a request and its byte permit atomically
with queue admission. If queueing fails, it removes that exact lifecycle entry;
if either transport half disconnects, it marks outstanding requests by whether
SSH sending had started, clears the registry, releases capacity, and wakes all
waiters. This replaces the older check-then-send cleanup with one live-session
termination contract.

## Server limit visibility

`SftpSession` now exposes the negotiated OpenSSH `limits@openssh.com` values,
the effective packet length cap, and the advertised open-handle cap. These are
read-only capacity hints for upper layers such as `hapcli-sftp`; they do not
change the high-level read/write behavior by themselves.

The main immediate use case is safe directory-transfer scheduling. A caller can
prefer server-provided handle limits over hardcoded concurrency when deciding
how many files or directory handles to keep open at once.

## Files That Carry the Fork

The required behavior is distributed across these files:

- `src/client/transport.rs`: session ownership, byte admission, cancellation,
  request phases, owned writes, and disconnect cleanup.
- `src/client/mod.rs`: outbound budget configuration and `OwnedSftpWriter`.
- `src/client/rawsession.rs`: raw packet encoders, owned-session constructors,
  request timeout policy, capacity reservation, and best-effort CLOSE handling.
- `src/client/fs/file.rs`: pipelined download/upload state machines, adaptive
  windows, ordering, short-read recovery, shutdown, and diagnostics.
- `src/client/session.rs`: high-level limits visibility and linear directory
  listing cleanup.
- `src/protocol/data.rs` and `src/protocol/mod.rs`: `Bytes`-backed DATA packet
  encode/decode.

Runtime abstraction, `StatusReply`, and the server handler changes already
exist in upstream russh-sftp 2.3.0. They are not hapcli vendor patches and
should be rebased normally rather than preserved as local modifications.

## Verification

After changing this fork, run:

```sh
cargo fmt --check
cargo test -p russh-sftp
cargo test -p russh channel_tx_write_bytes_preserves_owned_slices
cargo test -p hapcli-sftp
cargo test -p hapcli-ssh
cargo check -p hapcli-egui-app
git diff --check
```

For transfer-performance changes, also run the optional benchmarks:

```sh
cargo bench -p russh-sftp --bench upload_benchmark
cargo bench -p russh-sftp --bench local_transfer_benchmark
cargo bench -p russh --features _bench --bench sftp_transport
```

Focused regression coverage must include:

- raw READ and WRITE bytes matching generic packet encoding;
- bounded byte capacity held until acknowledgement;
- queued cancellation preventing a transport send;
- sent cancellation retaining late-response ownership;
- disconnect distinguishing queued from sent requests;
- CLOSE waiting for released capacity instead of being dropped;
- short reads restarting from the actual contiguous offset;
- out-of-order DATA/ACK responses preserving file order;
- adaptive growth and shrink behavior staying within caller and server caps;
- directory batches preserving server order and cleaning up failed handles.

## Upgrade Checklist

When updating russh-sftp:

1. Diff against upstream commit `dcc0c06a2aa14da96fb453f05b530c8fffba1b1a`
   or the exact 2.3.0 crate archive.
2. Check whether upstream now provides equivalent pipelined file APIs, owned
   frame writes, byte-bounded session ownership, and cancellation semantics.
   Prefer upstream only when its behavior and tests cover these contracts.
3. Reapply only the still-required files listed above. Do not classify line
   ending changes, formatting, runtime abstraction, or `StatusReply` as local
   product patches.
4. Keep `Cargo.toml.orig` and the normalized `Cargo.toml` synchronized,
   including the path-patched vendored russh used by workspace tests.
5. Recheck OpenSSH `limits@openssh.com` handling and effective packet/read/write
   caps before tuning application-level worker concurrency.
6. Run functional tests before benchmarks. A faster transfer path is invalid if
   cancellation, resume offsets, fsync, CLOSE, or response ordering regresses.
7. Update this document and the upstream baseline whenever the vendored source
   is rebased.
