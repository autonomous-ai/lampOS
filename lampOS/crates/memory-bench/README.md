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

This accounting layer has fake-provider tests. Live orchestration remains required before any scored execution.

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


## Shared speech cache

`speech_cache::create` accepts the sole budgeted model worker and prepared audio;
it reads no credential and constructs no separate provider or spending account.
Each distinct WAV is transcribed once, and its result is mapped back to all matching
records. WAV and PCM hashes, sample count and script mapping are verified before
dispatch. A failed attempt stays in its own directory and is never retried silently.

Successful results are re-derived from the saved request and final speech frames.
The cache copies the actual WAV, request, response and provenance files, pins their
hashes, and requires one returned speech-model revision for the whole corpus.
Missing confidence stays unknown. Unknown duration keeps its original spending
reservation; an overrun cannot qualify a cached result. Aggregate file and row
limits apply before publication and during reading. The complete manifest is
written last, so interrupted caches cannot qualify a run.

`verify-speech-cache SOURCE_EVENTS CACHE_DIRECTORY MANIFEST_SHA256 live|fixture`
rechecks every source, audio and provider file against the pinned manifest and
reconstructs observed events. A fixture/loopback cache cannot qualify a live run.
No clean script replaces a failed transcript. These cache checks need no provider
call, and the command prints verification counts only. Live cache creation remains
an internal API pending the approved campaign configuration and orchestration.

## Registered evaluation policy

The common answerer and blind judge prompts, output allowances and probe rules
are frozen in `fixtures/memory-bench/live-policy/registration.json`. Their hashes
are checked against the compiled prompts. This registration precedes every scored
run; it does not constitute model-access or spending approval.

The month enters memory through one shared real-STT cache. All 224 development
probe turns remain fixed registered text, as ruled before scoring on 31 October.
This isolates memory performance from question recognition. The limitation is
explicit: results can be kinder than noisy spoken questions in real use; those
belong in a separate voice test. No probe speech allowance is added.

The judge receives only the fixed criteria, eligible scripted source evidence,
and every candidate turn with its question and local timestamp. It receives no
setup, run, provider, spending or tier diagnostics. Appointment timing and repeat
avoidance are judged across the complete three-turn probe. Earlier completed
exchanges inside that isolated probe are valid citations only for later turns;
current, future and other-probe sources fail the deterministic evidence gate.
Those exchanges never become sources in the restored main month. The judge gets
explicit source IDs for completed probe exchanges and the union of gold and
eligible candidate-cited scripted records. Generated answers can witness timing
or dialogue; they never establish independent owner facts. Fixed probe questions
remain scripted input, separate from the generated answer.

Exact, date, abstention, forbidden-claim and citation checks take precedence over
the judge. Malformed answers or judgments remain incomplete, without silent repair
or a numeric score. Rubric results remain provisional pending human audit. The
existing scorer rules, original questions and answer keys are unchanged. Unit
fixtures test wrong and contradictory answers; they are not benchmark results.

After an actual forget attempt, every setup drops its current round transcript
and all cached memory, obtains a fresh snapshot, and restarts the answer from the
current question plus a content-free forget-status note. This uniform rule was
approved before scoring. It is stronger than the current product, which clears
future recent history but retains the current request prefix and earlier tool
results. Results must carry this privacy-relevant gap: the bench may improve the
current-product setup's forgetting relative to the real lamp.

The common answer loop permits eight model rounds and at most 48 calls in a
batch, through the single run worker. It validates the complete offered batch
against the currently advertised tools before dispatching any call. Opaque tool
extensions and reasoning are preserved for the current batch. Every tool refreshes
memory before the next answer request. Earlier tool-result bodies are replaced
with a fixed content-free invalidation notice, and a new batch discards the prior
batch's transcript. Forgetting discards the whole batch and skips its remaining
calls; attempts, charges and call identities are never rolled back.

Fresh tool-result text shares the memory cap with the next snapshot, recall and
tool schemas. The runner requests only the remaining space and refuses overflow
without truncating results. Within a structured result, arm-supplied keys and
scalar text count as UTF-8; JSON punctuation, escaping and the runner's provenance
envelope are separate overhead. The immediate conversation exposes source IDs
using the existing same-person, same-presence and ten-minute/twelve-exchange
selection. No answer-key evidence or setup label enters the answer request.

The chronological executor preserves the registered nightly and weekly schedule,
then runs isolated probes with the common answer loop and blind evaluator. It
checks the public owner-model and retained-source view after restoration; model
accounting never restores. Suppressed forget confirmations remain in evaluation
evidence but the entire post-answer exchange is omitted from memory replay, as
in the product journal. Such exchanges cannot supply a later memory citation.
This clarification is registered in live policy v2; original scripted ingestion
still reaches every setup. Recorded execution can exercise the judge transport,
but emits no question score and keeps every per-kind mean null.
