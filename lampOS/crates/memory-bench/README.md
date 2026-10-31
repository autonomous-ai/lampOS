# Memory benchmark implementation

This crate is isolated from the product. See the [registered plan](../../docs/memory-bench.md)
and [fixtures](../../fixtures/memory-bench). Recorded plumbing is never an
accuracy score, and the partial current-product arm is never the full baseline.

## Campaign spending boundary

`campaign::Campaign` holds one exclusive OS file lock for a campaign and
persists a hash-chained cost journal. Its immutable plan pins the explicitly
approved cap and a hash of public model/pricing/scope settings. It refuses a
requested plan above the remaining balance. The API is not approval to spend;
the caller must receive the owner's approved settings first.

Attach `Campaign::audit` at the central `model::Client` audit boundary, before
the run-ledger write. A reservation is synced before provider dispatch. If
either write fails, the client refuses dispatch. A known receipt reconciles
once; missing usage retains the full reservation. Only an explicit expiry
before dispatch can release a reservation without usage. A provider charge
above its reservation, or any token accounting limit violation, is recorded
and stops further work even after restart; already incurred
charges cannot be undone.

Creation syncs both files, the campaign directory and its parent before returning.
A new process replays the journal under the lock. Unsettled reservations stay
charged, duplicate call identities cannot spend twice, and a partial/corrupt
journal is refused without automatic repair. The cap and public policy cannot
change on reopen. Every setup, probe, judge, speech call and retry must use the
same campaign account; restoring memory does not restore money.

This accounting module has offline fake-provider tests. HTTP transport, environment-only credential loading and wire provenance are
implemented below; duration-priced speech, blind live judging and the complete
live CLI remain subsequent implementation steps. No live command is enabled by this module. Keys and spending approval
remain required before any paid execution.

## HTTP transport and evidence

`live_http::Http` implements the product route's DeepSeek chat wire: explicit
thinking disabled, streaming with usage, unchanged messages/tools, and a bounded
output allowance. Public configuration pins the complete credential-free endpoint,
requested model, expected reported model and context size. Credentials are loaded
only from a named environment variable. A separate literal-loopback constructor
uses a fixed dummy credential for offline tests. There is no network-enabled CLI
in this checkpoint, and neither constructor constitutes spending approval.

Every dispatched request saves private request/response body artifacts and their
hashes, returned model/request identity, finish reason and terminal usage. The
receipt carries provenance separately from the unchanged arm reply format.
Authorization headers are never written, transport error bodies are never printed,
and a response echoing the credential is refused without saving that body. Missing
or incomplete usage keeps the full reservation. Even a refused, truncated answer
reconciles known terminal usage; an output-token overrun stops the client and the
campaign. A failed evidence write leaves its reservation charged.

The client disables redirects, automatic retries and environment proxies. One
absolute deadline bounds connect/upload/read; the worker also bounds caller wait
while evidence is synced. Aggregate wire bytes, SSE lines/events, tool calls,
arguments and reasoning details are bounded. A missing terminal marker, changed
model/request identity, inconsistent usage, duplicate tool identity, or abnormal
finish fails the turn. Tool argument fragments and opaque extra/reasoning fields
are preserved. A conservative byte-based context check refuses an oversized full
history before dispatch; it never trims history or silently switches models.

The loopback tests exercise the actual HTTP path, including fragmented bytes,
usage on a final content chunk or separate usage chunk, missing usage, premature
EOF, length truncation, overruns, deadline expiry, service refusals, invalid streams,
size limits and credential echoes. They establish local protocol/accounting
behavior only, not cloud compatibility or service latency.

Wire references checked on 31 October 2026:
[DeepSeek chat completions](https://api-docs.deepseek.com/api/create-chat-completion/)
and the in-tree product client's `second_provider/net/chat.rs`. The benchmark
adds output/context bounds and evidence; it does not enable thinking or select a
different model. Provisional usage before the finishing chunk is not a final bill.
Any evidence failure stops subsequent spending, even when known usage costs less
than the reservation. Rejected SSE receives the same credential scan as answers.

## Verified audio and duration accounting

The internal transcription request owns immutable PCM decoded from a bounded
16 kHz mono PCM16 WAV. Its identity includes computed WAV/PCM hashes and sample
count; serialized requests contain no audio bytes or filesystem path. External
arms cannot deserialize or invoke transcription requests. The audio bound is
120 seconds, covering the unchanged aging fixture's 66.804-second utterance.

Speech uses the same `Client`, `Worker`, receipts and durable campaign as chat.
Dated per-minute prices charge integer microseconds, without minute-sized billing
blocks. Before dispatch, the sample count reserves the actual duration rounded
up to one millisecond. Only complete provider duration evidence can reconcile
that reserve. Missing/incomplete duration retains the reserve. A duration overrun,
mismatched PCM evidence or failed evidence write stops future spending and keeps
at least the reservation or any higher known charge. Restoring a probe does not
restore this account. Transcription confidence remains optional; no confidence
is inferred from a missing service field.

This accounting layer has fake-provider tests. The verified shared transcript
cache and live orchestration remain required before any scored execution.

Each durable reservation also binds its billing kind. Token evidence cannot
refund an audio reservation, and audio evidence cannot refund a token reservation;
a mismatch is persisted as a stop. Existing token-only journals retain their
original serialized hashes. A successful transcription must match its recorded
transcript hash, and any supplied confidence must be finite and within zero to one.
These checks precede refunds and also have restart regression coverage.


## Speech transport and evidence

The WebSocket transport uses the product route's Nova-3 general English settings,
16 kHz mono PCM16 and 20 ms audio frames paced in real time. It has a ten-second
handshake bound, a thirty-second response stall bound, a bounded Finalize wait,
a fifteen-second closing bound and the caller's absolute deadline across all of
these. It makes no retry. Chat and speech share one provider facade, client and
campaign account; an absent service fails before network dispatch.

Credentials come from the environment. Only the explicit offline fixture entry
point permits literal loopback plain WebSockets, using a fixed dummy credential.
Each call saves a private request description, received text frames and provenance
with hashes. Credential echoes cannot enter those artifacts. Frames, accumulated
wire bytes and transcript length are bounded. The WAV and PCM identity, samples
sent, provider request/model revision, terminal duration and optional confidence
remain linked to the receipt, including failed requests. Known duration survives
an identity or close failure; missing duration retains the original reservation.

Only final results enter the transcript. Repeated identical finals are deduplicated;
conflicting, overlapping or unresolved later results fail. Final coverage must
reach the input's end within 50 ms; processed duration alone cannot prove that no
words were lost. Missing confidence remains unknown. Terminal model maps are
cross-checked against result identity. There is no clean-script or interim fallback.

Offline WebSocket tests exercise upload pacing, finalization, metadata, truncation,
identity drift, deadline cancellation, secret echoes and failed evidence writes.
These tests are plumbing checks; they do not establish recognition quality.
The wire matches the in-tree product speech client and the
[Deepgram streaming reference](https://developers.deepgram.com/reference/speech-to-text/listen-streaming).
