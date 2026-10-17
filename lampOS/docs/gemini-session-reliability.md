# Gemini session reliability

Scope: `crates/gemini` and `crates/voice/src/provider_worker.rs`.

The shared branch integrates Agent 1 through `d6b813695` with additional review
corrections. See [integration qualification](provider-recovery-integration.md)
for the combined source, repeat-disconnect fix, startup behavior and new gates.
The verification tables below retain Agent 1's earlier source and evidence;
they are not results for the later shared tree.

Work since then, on top of shared checkpoint `2535fb565`, is in
[the third round](#third-round): a recovery defect found and fixed, the
stopped-playback limit measured, and what remains.

Evidence in this document comes from four separate places. They are never
combined:

| Class | What ran | What it can show |
|---|---|---|
| Host | macOS x86_64, scripted in-memory service | Transport and worker logic, bounds, ordering |
| Linux/ARM64 | Cross-compiled in a container, tests under user-mode emulation | The target builds, type-checks and runs the same logic |
| Cloud | Nothing by Agent 1: no provider credential exists on this host. The integrator ran two probes from a board | That handles are issued unasked, and nothing else yet; see [service evidence](#service-evidence-reported-by-the-integrator) |
| Device | **Nothing.** Lamp access is not restored | Nothing |

Passing these tests does not establish natural voice interaction. It
establishes that the listed failures no longer occur under the listed service
behavior. What the live service actually does in the ambiguous cases is
[unverified](#unqualified-behavior), and behavior that rests on an assumption
is reported as such at run time.

## Failures reproduced before fixing

"Ends the session" meant the provider worker exited and the whole run failed.

First round, reproduced on commit `64529dee4` with a scripted service:

| Scenario | Before | Now |
|---|---|---|
| Delayed `interrupted` arrives while the person speaks the next question | The new question was cancelled, then the session ended with `BarrierTimeout` | Dropped as `LateInterruption`; the question is unaffected |
| Duplicate completion arrives while the next question is being spoken | Session ended with `UnexpectedResponse` | Dropped as `LateTerminal` |
| Stray `interrupted` + completion arrive after the next question ended | The new question was completed with no answer | Both dropped; the real answer is delivered |
| Output transcript trails its answer's completion | Session ended with `UnexpectedResponse` | Attributed to the request that spoke it |
| 400 chunks arrive at once after a network stall | Session ended with `Backpressure`; 0 audio events delivered | All 400 delivered in order |
| Interruption whose confirmation takes longer than 0.8 s | Session ended after 803 ms and 70 blocks | Audio is never held |
| Answer still arriving after the response deadline | Cut with `ResponseTimeout` after 2 of 9 chunks | Deadline applies to the first output only |
| Service closes right after a complete answer | The answer's audio was dropped | Audio, generation and completion are delivered, then the end |
| Second interruption before the first is confirmed | Session ended with `OverlappingInput` | The waiting request is replaced |
| `goAway` during an answer | Session ended at once | The answer completes, then the connection is replaced |
| Any connection loss, including the idle close | Worker exited; run failed | Recovered when nothing accepted can be lost or repeated |

Second round, on commit `fbf9ecc3c`. The first two rows were reproduced in
real time with a consumer draining exactly 24,000 samples per second and the
largest (2 s) messages; the third is that commit's own test of its default.

| Scenario | Before | Now |
|---|---|---|
| 60 s answer, no pause | All 60 s delivered, but the session ended with `Backpressure`, a false delivery timeout | Completes; the connection stays ready |
| 48 s answer, playback paused 3 s after 6 s | Session ended with `Backpressure`; 38.0 of 48.0 s delivered | Completes; pauses up to the delivery bound are tolerated |
| Person resumes before any answer and the service stays silent | Session ended with `BarrierTimeout` after 2 s | The request proceeds; the assumption is reported |

Two further weaknesses of that commit were not reproduced; they follow from
its design and are covered by tests of the new one:

- Output was limited to 16 events plus one message, so an answer played at
  speaking speed stayed on the socket and a connection lost during playback
  would have lost the rest. Now the whole answer plays out and the link is
  replaced meanwhile.
- The worker's queue of up to 384 packets (15 s) was not purged on
  interruption, so once output is paced by playback the next answer would
  have waited behind it. Unsent output of a retired request is now pruned
  (the shared output-capacity change) and cancelled output is purged from the
  transport's buffer. The first packet of the new answer follows admission by
  5 ms (host, scripted service); at most the two packets already
  sent under outstanding capacity arrive late and are discarded.

V1 measured the service closing a session nobody talks to after 86-198 s with
WebSocket code 1008. That alone ended a V2 run after any long pause.

## Request ownership

Provider activity detection is disabled, so only this client's explicit
`activityStart` can interrupt a response, and the service cannot answer an
activity before its `activityEnd`. The transport uses exactly those two facts.

It keeps one **current request**, the newest, which is the only one that
accepts input, and at most one **earlier response** that must settle before the
current request may be committed with `activityEnd`.

- **Opening words are never held.** A new request sends `activityStart` and
  its audio at once, in order, whatever the earlier response is doing. Only its
  `activityEnd` waits. Until that End is sent, everything the service sends
  belongs to the earlier response by construction and is dropped.
- **Interruption is requested, never observed.** `interrupted` is honored only
  for a response this client has retired. Otherwise it is the delayed result of
  an earlier `activityStart`: dropped as `LateInterruption`. It cannot cancel a
  request and is not evidence that anyone spoke.
- **A stray pair stays a pair.** After a dropped `interrupted`, a completion
  that follows within the barrier bound and before any output of the current
  request is dropped as its companion.
- **Stray output re-arms the barrier.** Output with no request on record, or
  after a response's idle completion, is dropped and holds the next commit
  until it settles, exactly like a response this client retired.
- **Superseded input is not committed.** A request replaced before its End was
  sent continues in the same activity; it never gets a response of its own. A
  request cancelled before its End is dropped without `activityEnd`.
- **Output transcripts trail.** One that arrives before the current request
  has produced output is attributed to the previous responder. Input
  transcripts stay session-scoped and are never attached to a request.

### When the service does not confirm

The earlier response settles on its idle completion. If the service sends
neither that nor any further output for `Timeouts::barrier` (2 s):

| `BarrierPolicy` | Result |
|---|---|
| `Require` (library and production worker default) | `BarrierTimeout`; the connection fails |
| `AssumeAfterQuiet` (explicit transport/test policy) | The response is treated as over; `Event::BarrierAssumed` names both requests; the next request is committed |

The bound restarts on every event of the earlier response, so a slow but
active response is waited for. If it produces output again after an assumption
and before the next request is committed, the barrier is re-armed. A response
that is still producing 30 s (`Timeouts::response`) after it was asked to stop
fails the connection under either policy, so the next request cannot wait
forever.

Integration deliberately retains `Require`. The live endpoint has not been
probed for cancellation ordering, and the coordinator does not yet consume
`ownership_assumed`. Silence cannot qualify a late answer's owner. The explicit
assumption policy remains covered in tests and in the service probe tool, but
is not selected by the production worker or exposed in its configuration.

## Unqualified behavior

The transport cannot resolve these. Each is reported, never hidden, and none
is evidence of correct ownership.

1. **Answer after an assumed barrier.** The service stays silent for the whole
   bound, the next request is committed, and only then does an answer arrive.
   Nothing on the wire says which request it answers. It is delivered under the
   committed request, after `BarrierAssumed` for exactly that request.
   Preventing it would require the service to confirm interruptions, which is
   [probe 1](#service-probes).
2. **Lone completion before any output.** A completion that arrives after the
   current request was committed and before its first output cannot be told
   from an empty answer. It completes the request with no audio, and a real
   answer arriving afterwards is dropped as unowned. The coordinator records
   `no_audio_answer`.
3. **What the model heard.** A superseded request's audio stays in the activity
   its successor continues, and a cut-off question stays in the conversation.
   Whether the model answers the whole thought or only the last part is model
   behavior, not transport behavior.
4. **Conversation after a reconnect.** Context is whatever the service restores
   from its handle. This transport reports which kind of handle it used; it
   cannot inspect what the service remembers.

## Bounds

| Bound | Value | On expiry or overflow |
|---|---|---|
| Output held for a consumer slower than the network | 300 s of audio (14.4 MB), configurable 1-600 s | Socket goes unread; transport flow control holds the service |
| Event queue to the worker | 16 events | Events wait in the output buffer |
| Consumer accepts nothing while output waits (`deliver`) | 30 s (was 2 s) | `Backpressure`: the connection is replaced and the request reported lost; on an already failed connection, the rest of its answer is dropped and the request reported lost |
| Worker IPC backlog | high-water 384 packets, hard bound 512 | Worker stops taking provider events |
| Worker packets per 2 ms tick | 8, and never beyond granted output capacity | Remainder waits |
| Silence of an earlier response after an interruption request (`barrier`) | 2 s | Policy above |
| Earlier response still producing after an interruption request | 30 s (`response`) | `BarrierTimeout` |
| Ended input to first output (`response`) | 30 s | `ResponseTimeout` |
| Silence between outputs before generation completes (`stall`) | 10 s | `StalledResponse` |
| Idle completion after generation (`completion`) | received audio duration + 15 s | `CompletionTimeout` |
| Ended input to idle completion (`turn`) | 300 s | `ResponseTimeout` |
| Reconnect attempts per outage | 4; waits of 0.5, 1 and 2 s | `AttemptsExhausted` |
| Connection loss to completed setup | 15 s | `BudgetExhausted` |
| Recoveries per 60 s | 3 | `Unstable` |
| An input or output transcript in one message | 8,192 bytes (`MAX_TEXT_BYTES`) | Cut there at a character's edge and passed on; never refused. Until 2026-10-15 it was `MessageTooLarge`, which ended the service: on lamp-4ace at 14:27 an input transcript of a busy room's long talk stopped the voice |

The last three rows are the bounded policy, `RecoveryPolicy::default`. A
bounded run (a probe, a directed session) still ends on them. A service
that runs all day does not: see the next section.

### A link that is kept (`RecoveryPolicy::endless`)

On lamp-4ace on 2026-10-13 a test cut the link every 24 seconds. Each loss
was cured in under a second, the fourth inside a minute was `Unstable`, and
the voice service ended, three times. A person in the room does not care how
often a link dropped; they care whether the lamp is still there.

With `endless` set, a loss that a new connection can cure never ends the
supervisor:

| Case | Bounded policy | Kept link |
|---|---|---|
| More failed attempts than `max_attempts` | `AttemptsExhausted` | Goes on asking, `max_backoff` (4 s) apart |
| Outage longer than `outage_budget` | `BudgetExhausted` | Goes on; each attempt is ended by its own connect and setup timeouts (10 s each) |
| More recoveries than `max_recoveries` in `window` | `Unstable` | Replaced after `max_backoff` instead of at once |
| The first connection fails | `InitialConnect` | Reported once as unavailable on session 1, then asked for like any other. It comes up as `Context::Fresh` on a later session: there is no earlier conversation for it to have kept |
| Quota, rate or usage refusal | `NotRecoverable` | Asked again after `refusal_pause` (120 s) |
| A fault a new connection would repeat (authentication, configuration, protocol) | `NotRecoverable` | The same: it still ends |
| No handle, with `require_context` | `NoResumableContext` | The same: it still ends. The service sets `require_context` off for its kept link, so the session comes back fresh and says so |
| Local stop | `Shutdown` | The same |

The attempt count a report carries stops at 16 (`MAX_COUNTED_ATTEMPTS`), the
most the worker's link report accepts; a kept link reports 16 for that many
or more. Waits are not jittered and do not grow past `max_backoff`: one lamp
asks its relay fifteen times a minute while it is down. That is chosen for
how soon a lamp hears again after its network returns, and is not right for
a fleet behind one relay.

The voice service asks for it with the provider worker's `--keep-link`
flag, which the coordinator passes for `serve` and for no bounded run
(`ProviderConfig::serving_recovery_policy`).

Tested on the host only, with the scripted service: eight tests in
`recovery/tests/endless.rs` (six of them fail when `endless` is switched
off in their policy), two in the worker through its link conversion and
checks, one in the coordinator's availability register. Not yet shown on a
lamp: a real network taken away and given back, a real quota refusal, and
what a person hears while the link is down.

`deliver` must exceed the playing time of the longest message, 2 s, plus the
longest pause in playback that should be survived. It measures a consumer that
takes nothing at all; one that is slow but still taking events never trips it.

By design, and not exercised by a test: an answer longer than about 285 s
reaches the `turn` bound before the service's idle completion. Everything
received is still delivered, and the request is then settled or reported lost
according to whether generation had completed.

## Function calls

A session can be given functions the model may ask this side to run
(`SessionConfig::tools`, at most 32, declared at setup beside the service's
own search when that is on). Until 2026-10-13 any function call ended the
connection as `UnsupportedMessage`. What the session does with one now:

| Case | What happens |
|---|---|
| A call for the committed request | `Event::ToolCall` with that request's lineage. It counts as the request's first output, and the `stall` bound runs from it |
| The service completes the turn it made the calls in | Not the end of the request, and not reported: the service ends that turn as soon as it has asked (4 ms after the call on lamp-4ace on 2026-10-13) and speaks in another once the calls are answered. The request ends at the completion after that. The same holds when the answer was sent before that first completion arrived |
| A function declared silent (`Tool::silent`) | Declared with `behavior: NON_BLOCKING` and answered with `scheduling: SILENT`: the service says nothing on account of it. The turn it is asked in is then the request's last, so that completion ends the request, and the answer is sent even when it comes after that |
| The answer (`InputSender::try_tool_response`, by identifier and name) | Written at once; the `stall` bound runs again from the answer. Not an object: sent as `{"result": ...}`. Over 4,096 bytes of JSON: refused where it is offered, nothing sent |
| A call with no committed request to own it (none in progress, or one whose input is still open) | Answered on the wire as not done, under `not_done`, saying why (`NOT_DONE_STILL_SPEAKING`: nothing went wrong, the person was still speaking, say nothing about the call, call it again if what they say now still asks for it) so the service is not left waiting; `Discard::UnownedToolCall`; no `ToolCall` event. A bare `cancelled` read to the model as a change of mind: on lamp-4ace on 2026-10-15 it did not ask again in three of five two-breath yeses. Sent as an `error`, it was spoken as a failure: at 20:07 that evening a request to hand work over, cut at its comma, was asked for again and prepared, and the lamp said the work could not be prepared |
| The service withdraws calls (`toolCallCancellation`) | `Event::ToolCallsCancelled`; an answer offered for one afterwards is dropped, `Discard::StaleToolResponse`, nothing sent |
| An answer for a call already answered, or whose request has ended | The same: dropped and reported, nothing sent |
| A call that is never answered | The connection ends as `StalledResponse` at the `stall` bound (10 s), like any other silence; whoever takes `ToolCall` must answer every call, also one it will not run |
| More than 8 calls in a message or waiting on one request, an identifier over 128 bytes, a name over 64, arguments over 4,096 bytes of JSON | `MessageTooLarge` (the first three and the last); a ninth call waiting on one request is answered as cancelled |
| A call message with no calls, or beside content or a resumption update | `MalformedMessage` |

The codec does not judge whether a name or its arguments mean anything:
that is for whoever runs the function. Through the supervisor an answer is
taken only by the connection that made the call
(`Supervisor::try_tool_response`); a call dies with its connection and is
not carried to the next.

Tested with the scripted service (`session/tests/tools.rs`, ten tests).
Against the real service, one silent written request each on lamp-4ace on
2026-10-13: the relay accepted two declarations; "Lamp, make your light
blue." got a call of the sample function with `{"colour":"blue"}` 0.93 s
after the request; "Lamp, what is two plus two?" got no call and a spoken
answer. Not yet shown: calls made from speech, what the model says after
an answer, and what a declared function costs an ordinary answer's first
audio.

`LAMP_GEMINI_SHAPES` set to anything makes each session say on stderr what
every server message looked like (`wire::shape`: field names and a few
schema enumerations, never audio, text or a handle; a run of one shape is
said once). It is how the completion after a call was found.

## A stop is a fence on the wire

A stop is `Connection::shutdown` or `InputSender::shutdown`, or a failure
the input side records itself, such as `Backpressure`. From the moment it
is recorded the session does three things and no others.

| What was pending | After the stop |
|---|---|
| Commands still in the queue: audio, an End, a start | Discarded unread |
| An End held behind an earlier response | Never written, whatever arrives |
| A message read from the service after the stop | Not acted on: no event, no release of a held End, no answer to a ping |
| A write that has not begun: any of the above, a keepalive | Does not begin |
| A write waiting on the transport | Abandoned. The transport is not offered another byte |

Every write the session makes after setup goes through one place,
`Actor::write`, which looks at the stop before the transport each time it
is polled. An image is written by `send_image`, which already ended on a
stop in the same way and is unchanged.

**The exact limits.** None of this recalls anything.

- Bytes the transport had accepted before the stop stay accepted, and
  that can be a whole frame: a write may be waiting on the transport's
  flush with every byte of its message already taken. A write still
  waiting is abandoned, what the transport had not accepted is dropped,
  and the connection is not used again.
- A stop recorded by another thread while the session's task is in the
  middle of a poll is seen at its next poll. That one poll may hand the
  transport the rest of the one message it was writing: one audio block,
  one `activityStart` or `activityEnd`, one ping or pong. Nothing follows
  it. On a single-threaded runtime the task is not mid-poll when another
  task stops it, and this case does not arise.
- "Accepted by the transport" means accepted by whatever this session
  writes into. Below that, a TLS layer and the kernel hold bytes of their
  own. Whether those reach the service after a stop is not controlled
  here and has not been measured. Nothing here is a claim about when the
  service stops hearing.

A session that fails by itself while a write is held, for example because
the queue behind the held write fills, ends at once with that failure and
not with the write's own deadline. With no stop the fence is not in the
way: a held write that is released completes, and what was queued behind
it follows in order. The write deadline, 200 ms, is unchanged.

| Test | What it holds |
|---|---|
| `a_stop_with_input_queued_behind_it_writes_none_of_that_input`, `a_stop_with_input_queued_behind_a_barrier_writes_none_of_that_input` | Twenty blocks and an End queued, then a stop before the session runs again: none written. Forty trials each, to exercise different choices among the session's ready work; not a proof over every order |
| `stop_fence::a_write_held_at_the_transport_is_abandoned_by_a_stop_and_nothing_follows` | The transport has accepted nothing of an audio frame and holds. The stop ends the session within half the write deadline; released afterwards, the transport is offered nothing, not that frame and not the End behind it |
| `stop_fence::bytes_the_transport_had_accepted_stay_and_not_one_more_follows_a_stop` | The same with twenty bytes accepted first: those stay and none is added. Twenty bytes are less than that frame, so in this test no message arrives |
| `stop_fence::a_held_end_is_not_released_by_a_terminal_read_after_the_stop` | The stop is recorded as the earlier response's idle completion is read: the held End is not written and no event is made of the completion |
| `stop_fence::a_failure_recorded_while_a_write_is_held_ends_it_as_a_stop_does` | `Backpressure` behind a held write: ends at once as `Backpressure`, nothing more written |
| `stop_fence::a_held_write_released_with_no_stop_completes_in_order` | The control: no stop, the frame completes and the End follows |

The `stop_fence` tests run on an in-memory transport with gates that hold
a write before or after it has accepted bytes, and that record a stop at
the instant a read returns. The gates are the integrator's, from the
review that found the held End and the waiting write still uncovered
after the first fix. Before the fence the first four `stop_fence` tests
fail and the control passes.

### Integration verification, 2026-10-11

Main composed Agent 1's reviewed `53f303c7e` Stop change with integration
`ff683e665` (failure-notice runtime `e63f3152`), using a source-only export.
The two reproduced transport gaps and the queue guard are covered; the
before-output continuation proposal is not included. Runtime defaults and
response deadlines are unchanged.

| Combined-source check | Result |
|---|---|
| `cargo fmt --all -- --check` | passed |
| `cargo clippy --locked --offline -p lamp-gemini -p lamp-voice --all-targets -- -D warnings` | passed on macOS and emulated Linux/ARM64 |
| `cargo test --locked --offline -p lamp-gemini --lib -- --test-threads=1` | 144 passed, 0 failed, 2 existing ignored |
| `cargo test --locked --offline -p lamp-voice --lib failure_notice -- --test-threads=1` | 13 passed, 0 failed |
| `cargo test --locked --offline -p lamp-voice --lib provider_worker -- --test-threads=1` | 62 passed, 0 failed, 2 existing ignored |
| ARM64 release `lamp-gemini --lib` filtered to `stop_fence` and `a_stop_with` | 7 passed, 0 failed, 0 ignored |
| ARM64 release build of `lamp-voice` | linked as little-endian AArch64 ELF |

This is 219 passing host tests and seven focused emulated ARM tests, not a new
full-workspace pass or physical Stop measurement. Agent 1's separate
full-workspace gate was on `2fd77a417`; its final checkpoint changes only
comments and documentation after that gate.

The private source manifest is
`8565c7d5b6b7eadb0e7cbf4e24c77b00bc698850f9f81c54ce3d14459a851c8b`
(409 files, before this verification prose). The exact ARM receipt is
`87fbabb32ce10100d42c278eae0a337f71ccaf9e48ebabbe8cfc6b02b1ce98e4`; the 8,681,808-byte executable is
`b1c61a224ea9958eeaa5f06b87fdc7591128da3daf01c21e2f3c851a7643edbd`. The copied artifact matches the
build digest. Source, logs and artifact are retained locally at
`/private/tmp/lamp-stop-composed-9_evluok`; no executable is committed.

The installed Lamp remains on the separately qualified `55d7af98e` runtime.
These checks do not prove acoustic interruption latency, physical mute/reopen,
or when Gemini stops receiving already-accepted bytes.

## Speaking speed

The service generates faster than a speaker plays. The transport therefore
receives an answer at network speed and holds it, so playback speed does not
decide how long the connection must survive, and a connection that dies during
playback does not take the rest of the answer with it. Only beyond the output
bound does the socket go unread.

Commands, local retirement and shutdown stay live throughout. One server
message is handled per scheduling turn. Cancelled output is purged from the
transport's buffer and from the worker's queue the moment its request is
retired, so the next answer is not delayed by audio nobody will hear.

Timing relies on the service answering WebSocket pings during the long quiet
period between generation and its idle completion.

### Output capacity

The coordinator and the worker share the cumulative packet limit defined in
[provider output capacity](provider-flow-control.md), which belongs to the
coordinator's owner: `Control::ProviderOutputCapacity { through }`, granted
from the free space of the coordinator's 30 s reply buffer, with at most two
packets outstanding. The provisional `AudioCredit` seam this worker carried
before that contract existed is gone.

What the worker does with it:

- Every packet consumes capacity: audio, lifecycle events, transcripts,
  `Ready` and the proposed statuses alike. Nothing is sent without it.
- Link status (`unavailable`, `recovered`) is queued ahead of audio but still
  waits for capacity. At playback speed that is at most one packet, 40 ms. With
  a full reply buffer and stopped playback it waits as long as playback does.
- Unsent output of a retired request is pruned on every tick, before anything
  is sent. Failure and link statuses are never pruned.

A long answer therefore sits in four places, each bounded, from the speaker
back to the service:

| Holder | Bound |
|---|---|
| Coordinator reply buffer | 30 s (720,000 samples) |
| Worker queue | 512 packets (20.5 s); stops taking events above 384 (15.4 s) |
| Event queue to the worker | 16 events |
| Transport output buffer | 300 s; beyond it the socket goes unread |

### A stopped speaker

At speaking speed the worker's queue falls below its high-water mark, and so
takes one more event from the transport, once per message's worth of playback:
every 2 s for the largest message. Stopping playback for a time *H* stretches
one of those gaps to *H* plus one message. The transport gives up the audio it
still holds once nothing has been taken for `deliver` (30 s), so with 2 s
messages the limit is 28 s of stopped playback. It is now measured through the
worker in real time, with a 90 s answer, a 5 s reply buffer and playback
stopped once after 20 s, when every holder is full:

| Playback stopped for | Result |
|---|---|
| 25, 27, 28 s | Complete: all 90 s delivered in order |
| 29, 30, 31, 33 s | `Backpressure`. 58.64 s of the 90 s reached the speaker |

Past the limit:

- **Nobody can tell until playback resumes.** The worker reads the supervisor
  only when its own queue has room, so it learns of the dead connection once
  it has played some of its backlog. In the measured runs the worker ended
  33.7 s after playback resumed, having delivered everything the reply buffer,
  its own queue and the event queue held. The answer is cut there.
- **The answer is lost; the session need not be.** When the table above was
  measured, `Backpressure` was a local fault that was never reconnected, so
  the supervisor ended. Since `f085868f3` (request `MAIN-007`) it is recovered
  like a lost connection: the stuck connection is abandoned with what it still
  held, the request is reported lost exactly once at `Responding`, and a
  replacement is dialed with the resumption handle inside the usual attempt,
  budget and recovery-rate bounds. Nothing is replayed. A session with no
  resumable context still ends, as do authentication and quota refusals.
  Under `Legacy` reporting the worker ends on the lost request either way, so
  the worker-level table is unchanged until the coordinator consumes the
  failure.
- **Which bound fires depends on the answer.** The delivery bound is only
  reached by an answer long enough that the service's own deadlines are later:
  generation complete and more than about 15 s of audio still unplayed. A
  short answer whose speaker stops is ended earlier by the stall or completion
  bound, which were already recoverable.
- **Input that overruns its queue** is the same failure seen from the other
  side, and is recovered the same way: the request that was speaking is
  reported lost at `InputOpen` and its remaining input is refused, not carried
  to the replacement.

The same bound applies to an answer still being drained from a connection that
has already failed, and is tested in the transport on a paused clock. Stopped
for 29 s, a 40-message answer is delivered whole and reported `Settled`.
Stopped for 31 s, only what had reached the 16-event queue is delivered, as an
unbroken beginning, and the request is reported `TurnLost { Responding }`. A
cut answer is never reported complete, and the replacement connection is
untouched.

Holds of 4, 10 and 20 s inside long answers are tested separately (see
[verification](#verification)).

## Connection loss

`lamp_gemini::Supervisor` owns one live connection at a time.

1. A failure is noticed at once, by the worker's 2 ms tick through
   `Supervisor::maintain`, not when the next event is asked for. Input is
   refused from that moment.
2. The replacement is dialed immediately on the runtime, with the retained
   resumption handle, while output the failed connection had already received
   keeps being delivered in order at playback speed.
3. When that output is exhausted, the request that was in flight is reported:

| Request state when the output ran out | Result |
|---|---|
| None, or cancelled locally | Nothing to report |
| Generation complete | `Settled`: the closed connection is the idle barrier; the worker emits `TurnComplete { idle: true }` |
| Input still open | `TurnLost { InputOpen }` |
| Input ended, no output | `TurnLost { AwaitingResponse }` |
| Output began, generation incomplete | `TurnLost { Responding }` |

**Nothing is replayed.** Input is never re-sent and no answer is requested
twice. A lost request's identifier is retired across the reconnect. A
follow-up during the drain goes to the replacement connection and cancels what
remained of the old answer. A local shutdown or privacy stop drops queued
output.

Quota, rate-limit and usage-limit refusals, and authentication, configuration
and protocol faults, do not reconnect. Server text is never retained: a close
reason or error body only selects a fixed category. A `Transport` failure
keeps no cause, so the socket's own kind is said on stderr as it happens
(`lamp-gemini: the link failed: io ConnectionReset: …`, `tls: …`,
`protocol: …`, `http 404`, `url`): the local error's words, never the URL,
which carries the key (#705, 17 October: lamp-4ace's 166 transport drops
could not be told apart).

Context survives only through the service's resumption handle
(`Resumption::Retain`: initial setup unchanged, handle kept in memory, never
logged or persisted). With `require_context` on and no handle, the worker does
not continue with a session that has forgotten the conversation.

### Recovery does not wait for its reader

The supervisor can be read in two ways. `next()` returns link changes and
answer events through one call. `poll_link()` and `next_output()` return them
separately, so that a consumer with no room for audio can still learn that the
link changed; see [provider availability](provider-availability.md). Either
way one link update is held at a time, and nothing that would need a second
one is applied while the first is unread.

Retry work announces nothing, so it is not held back. That was a defect in the
shared tree: with an outage update unread, a refused replacement attempt was
never retried. The outage budget ran out on the clock and the link ended as
`BudgetExhausted` when the update was finally read. The provider worker reads
nothing from the supervisor while its queue is above its high-water mark, so a
stopped speaker put it in exactly that position. `Supervisor::maintain` now
harvests a failed attempt, backs off and redials inside the same attempt and
budget bounds, whether or not anything is being read.

Two limits remain for a consumer that reads no link update for longer than
the outage budget (15 s):

- A replacement that completes setup and then dies is noticed only once the
  earlier update has been read. If that is more than the budget after it died,
  the link ends as `BudgetExhausted`.
- A replacement that completes setup stays unannounced, and input stays
  refused, until the earlier update has been read.

Reading the link on the control tick, independently of output, removes both.
The worker does not do that yet; it still uses `next()`.

### Reporting to the coordinator

`Reporting::Legacy` is what runs today, because the coordinator implements only
this:

- A recovery is a repeated `provider_ready`; its `setup_us` spans loss to setup.
- `TurnLost`, and input admitted during an outage, end the worker with the
  request and stage in the error.
- One standard-error line per outage, recovery, settled answer, assumed
  barrier, announced close and first dropped event of each kind. A clean run
  writes nothing.

`Reporting::Typed` is implemented and tested but not enabled. It keeps the
worker running and sends the statuses below. See the next section.

### When a write cannot finish

A write finishes once the transport's buffer takes it, so it only waits
when that buffer is full: the link is behind. A write that doesn't finish
within its bound ends the connection as `write_timeout`. Since #1393 the
error also says which write it was, so a drop can be traced to it
(lampOS#705). For example:
`{"category":"write_timeout","write":"picture","kib":245,"deadline_ms":200}`.
- `write` is one of:
  - `setup`;
  - `audio`, a block of the person's audio;
  - `image`, a camera frame with speech;
  - `picture`, a written turn with the lamp's look;
  - `written`;
  - `activity`, a start or an end;
  - `note`;
  - `context`;
  - `tool`, a tool's response;
  - `keepalive`, a ping or a pong.
- `kib` is the message's size, rounded up.
- `deadline_ms` is the bound it had, rounded down. That is
  `Timeouts::write` (200 ms unless set), 400 ms for a camera frame, and
  less for a block of audio near its age bound: 2.5 s, or 2.5 s plus the
  patience for kept audio (`drain_held`). A block with under 1 ms left
  reads 0.

The running records carry it today:
- `provider_unavailable`'s `link.state.reason`;
- `provider_ready`'s `link.state.after`;
- `provider_terminal`'s `link.state.fault`;
- `provider_turn_failed`'s `reason`;
- the journal's `connection lost (…)`.

Records from before carry the category alone.

**How long a write may wait** is the session's own choice
(`SessionConfig::writes`), because two bounds meet there:
- **The write bound:** `Timeouts::write` for every write, plus
  `Timeouts::write_per_kib` for each KiB of the message, so a big message
  has time to go out. The allowance is zero by default. A block of audio
  never gets it, since its bound is capped by its age.
- **The input queue:** while a write waits, the session takes no input.
  The queue holds `INPUT_QUEUE_CAPACITY` (32) commands by default, and a
  block of audio is one command of 20 ms, so 640 ms of the person's speech
  fits. A wait past that while someone speaks fills it, and the next block
  ends the connection as `Backpressure`. A longer write bound alone only
  turns a `write_timeout` into that. The queue may hold up to
  `MAX_INPUT_QUEUE` (128 blocks, 2.56 s). A block's age is checked only
  when the session takes it, so speech behind a write that waits longer
  than that still ends the connection.
- **The quiet and response bounds:** the barrier's quiet counts from the
  last event read, and nothing is read while a write waits. With
  `SessionConfig::writes` (patient writes), a write that
  held the session 100 ms or more owes a read, so what the service sent
  meanwhile is read before either bound is judged (`read_waiting`, Nine on
  lampOS#1397). A confirmation that came in during a stall is not silence
  (`session::tests::writes::a_confirmation_that_comes_while_a_write_waits_is_read_before_the_quiet_is_judged`;
  judged first, it ends as `BarrierTimeout`).
  Without it the read loop is as before, and a confirmation that comes in
  during a write of 140 ms still ends as `BarrierTimeout`
  (`without_patient_writes_the_quiet_is_judged_as_before`, the reviewer on
  lampOS#1397).

With `LAMP_PROVIDER_WRITES=patient`, the voice's provider worker sets
1 s, 8 ms a KiB (a 1 Mbit/s floor) and 128 blocks
(`voice::config::patient_writes`). The look's picture of up to 245 KiB
then has about 3 s, and a camera frame about 1.1 s. A line spoken over a
stall of 0.8 s keeps every block and its connection, where today's bounds
end the connection on that line's audio
(`session::tests::writes::a_line_over_a_stall_is_kept_only_with_patient_writes_and_their_queue`).
The cost: while a write waits nothing is read, so an answer's audio behind
it comes late.

**Its limits, unmeasured on a lamp:**
- A stall longer than 1 s still ends the connection as `write_timeout`.
  The 1 s is to be sized from the writes lampOS#1393 names on lamp-4ace.
- Speech behind the look's picture written for longer than 2.56 s ends it
  as `Backpressure`.
- With a second route, a command waits for room in the queue up to
  1.05 s instead of 250 ms, past the write bound either way
  (`provider_worker::ROOM_WAIT`).

## Proposed coordinator contract

Not enabled. `ProviderOutput` is matched exhaustively in `coordinator.rs`, so
each addition needs a change by its owner. The statuses are `ProviderStatus` in
`provider_worker.rs`; they use the same envelope and `kind` tag and are meant
to become `ProviderOutput` variants unchanged.

```json
{"kind":"unavailable","reason":{"category":"peer_closed","code":1008}}
{"kind":"recovered","outage_us":1500000,"attempts":2,"context":"resumed_before_latest","reason":{"category":"server_go_away"}}
{"kind":"turn_failed","request":7,"stage":"awaiting_response","reason":{"category":"stalled_response"}}
{"kind":"ownership_assumed","request":8,"superseded":7}
```

| Status | Meaning | Coordinator obligation |
|---|---|---|
| `unavailable` | No connection. Sent ahead of queued audio | Stop admitting, or expect `turn_failed` for what is admitted |
| `recovered` | A connection is ready. `context` is `resumed`, `resumed_before_latest` or `fresh` | Resume admission; record the context |
| `turn_failed` | No further output for this request. `stage` is `not_delivered`, `input_open`, `awaiting_response` or `responding`. Sent after all output that did arrive | Give the turn an honest failed outcome and cancel it; this is the hook for a spoken failure |
| `ownership_assumed` | See [Unqualified behavior](#unqualified-behavior) | Mark the answer's outcome as unqualified |

`reason.category` is one of `connect_timeout`, `setup_timeout`, `read_timeout`,
`write_timeout`, `barrier_timeout`, `response_timeout`, `stalled_response`,
`completion_timeout`, `authentication`, `rate_limited`, `quota_exceeded`,
`server_unavailable`, `server_rejected`, `server_go_away`, `transport`,
`peer_closed` (with `code`), `protocol`, `local`.

Each `turn_failed` is sent once. Remaining input for a failed request is
dropped by the worker until the coordinator retires it, and never reaches a
later session.

Also needed from the coordinator's side:

- When the statuses are adopted they are packets like any other: the
  coordinator must count each one with `OutputReceiver::received` or the
  capacity counters diverge.
- A decision on whether `unavailable` should be able to pass a full reply
  buffer. Today it cannot, by the rule that every packet reserves space.
- A `ProviderConfig` option for `Resumption::Request` and the barrier policy.
- The startup poll now accepts a repeated `Ready` while later children start,
  preserving the original output-capacity counters. Other unsolicited output
  before input is still rejected. This does not implement outage admission.
- `provider_interrupted` can no longer arrive for the current reply.

## Service probes

No cloud probe has been run: the provider configuration and credential exist
only on the device. `crates/gemini/examples/service_probe.rs` is ready for
whoever has access. It needs no microphone, speaker or device. It speaks one
cached question and prints one JSON line of event kinds and timings: no
transcript, audio, handle or credential.

```sh
cargo run --locked -p lamp-gemini --example service_probe -- \
    /absolute/private/provider.json QUESTION.wav PROBE
```

| Probe | Question it answers | Decides |
|---|---|---|
| `interrupt-before-output` | After `activityEnd` and before any output, does `activityStart` produce `interrupted`, a completion, or nothing? | Whether pause-and-resume ever takes the assumed path |
| `handles` | Are resumption handles issued without being requested? | Whether recovery is available on this endpoint as configured |
| `handles-requested` | Is `sessionResumption: {}` accepted in setup? | Whether to switch to `Resumption::Request` |
| `resume` | Is a handle accepted on a new connection, and does that session answer? | Whether context survives a reconnect at all |
| `idle-close` | How long does an unused session live and how does it end? | The idle-close code and timing assumed from V1 |
| `fixture` | Offline: what would be spoken from the WAV file | Nothing about the service |

`fixture` was run on the cached greeting: a 20 s file, 2.31 s spoken, 231
blocks. The other five have never run.

## Verification

### Explicit capture-clock fixtures

The default-off `lamp-gemini` feature `fixture-clock` adds
`testing::CaptureClock`, `pair_with_capture_clock(config, session, capacity, clock)`
and `connector_with_capture_clock(config, capacity, clock)`. Unit-test builds expose
the same APIs without that feature, so the normal native test command runs these
regressions. Ordinary library builds expose them only with `fixture-clock`.
The explicit factories run the same setup handshake and Actor as the existing
in-memory service. One
shared clock supplies capture age to the actual InputSender and Actor, including
pending images and kept audio. Existing factories always select system time,
including in a build where another package enables the feature.

The clock returns a pure, current `std::time::Instant` in the acquisition domain.
Reads do not advance time, pump work or change Tokio time. The fixture controller
owns monotonic updates, epoch validation and cancellation; invalidation must keep
the last admitted reading until cancellation, never jump to host time. Audio
still has the full strict 2,500 ms bound, images 2 s, and kept audio
`MAX_INPUT_AGE + patience`. The two Actor audio checks remain separate fresh reads.
Protocol deadlines, image write timing and the original 10 ms Actor intervals
still use Tokio.

The additional `session::capture_clock_tests` exercise exact age boundaries,
queue-to-Actor expiry, real image and audio writes, kept-audio release, default
factory isolation, and reconnect setup/capacity. Their ordering evidence comes
from actual wire messages, events and Actor completion. A clock sample, successful
enqueue or empty socket is not a completed poll. Existing test bodies and
assertions are unchanged. This is clock preparation: it does not migrate a Voice
scenario, provide a provider progress barrier, or establish device latency.

### Strict continuation probe (example-only addition)

The explicit `continue-before-output` mode sends two **different**, already
cached mono 16 kHz PCM16 WAVs. It uses the production barrier policy
`Require` and its default **2 s** bound. It collects events concurrently with
both paced 10 ms sends. No Supervisor, reconnection, retry, physical audio,
camera, or coordinator participates. The old `interrupt-before-output` mode
is retained as the historical repeated-question, `AssumeAfterQuiet` / 5 s
probe; do not use that legacy mode as strict production-policy evidence.

```sh
cargo run --locked --offline -p lamp-gemini --example service_probe -- \
    /absolute/private/provider.json FIRST.wav continue-before-output SECOND.wav 300 \
    > /absolute/private/continuation-result.json
```

The separately named control is
`PRIVATE_PROVIDER.json QUESTION.wav repeat-before-output [GAP_MS]`. It is a
repeated question, not a test of preserving a split thought. `GAP_MS` defaults
to 300 and is limited to 0–5000. It starts after the first End is **queued
locally**, not after a wire acknowledgement. No new credential or model
configuration is introduced. The configured endpoint/model/voice/thinking
level/language are used, but the example's generic one-sentence instruction
is not production `CHAT_INSTRUCTION`.

Input is restricted to one standard PCM16 format/data chunk, exact RIFF and
chunk lengths, 10 ms–10 s per file, and at most 385,536 file bytes. It is not
trimmed, resampled, normalized or padded. Identical decoded samples are
refused in continuation mode before opening any provider configuration.
Files must be regular; the existing bounded reader checks descriptor identity
and caps reads. Local filesystem I/O precedes the service observation window.
There is no hashing dependency: the operator must pin the WAV SHA externally;
the report explicitly says the probe did not verify a SHA. Counts mean
samples accepted by the local input queue, not confirmed service receipt.

The connected trial has one nonrenewable **40 s** deadline, including input,
the gap and up to **8 s** after request 2's idle completion. Library connect
and setup each retain their 10 s bound before that trial. Every early close,
partial send, refusal, missing terminal, or deadline is retained in the JSON.
Aftermath duration is actually observed and `window_complete` is false if
interrupted; an early close never becomes eight seconds of silence. Event
storage stops at 2048 rows or 65,536 transcript/text bytes and reports
incomplete evidence. Strict modes print bounded output/model transcript text
for private review, never credentials, handles or PCM. Input transcripts are
explicitly session-scoped and have no invented request identifier.

`strict_barrier_confirmed_and_answered` means the public API reported request
2 waiting at a barrier, no request 1 audio had been observed before the second
Start was queued, request 1's original idle completion was observed after
that queue, request 2 produced audio and an idle completion, and the bounded
aftermath finished. Only this outcome returns exit 0; other outcomes still
print the JSON and return a nonzero exit. `Interrupted`, generation complete,
and a non-idle completion do not substitute for request 1's idle terminal.
`strict_barrier_timeout` is a recorded failure, not a successful continuation.
An answer already begun or completed before the second input is a different
case and is reported as such.

This is an **observed transport result**, not a semantic or acoustic pass.
The public API has no End-write acknowledgement, server receive clock, or
drop ledger. `waiting_for_barrier` alone does not name the prior gate owner.
Already retired output can be filtered before the caller observes it, so the
probe cannot prove absolute service-side before-output ordering or the
absence of extra dropped output. A `Discarded` event is not a count of extra
answers. Event times are caller observation times; output tokens remain the
transport's original session/request, with no fabricated attribution.
The report keeps these missing capabilities explicit. Assess whether the
answer addresses the **whole** two-part input separately from its actual
output transcript; semantic assessment remains `unscored` automatically.

Focused offline verification uses the actual transport with an in-memory
scripted service and paused clock:

```sh
cargo test --locked --offline -p lamp-gemini --example service_probe -- --test-threads=1
cargo clippy --locked --offline -p lamp-gemini --example service_probe -- -D warnings
```

These tests are not real-service evidence. They cover simultaneous send and
receive, strict timeout during a longer continuation, original identities,
early close, non-idle terminal, missing answer/terminal, nonrenewing deadline,
bounded evidence, and unchanged valid fixture samples.

### Historical workspace commands

From `lampOS/`, with a fresh target directory:

```sh
cargo fmt --all -- --check
cargo clippy --locked --offline --workspace --all-targets -- -D warnings
cargo test --locked --offline --workspace --no-fail-fast -- --test-threads=1
cargo build --locked --offline --release -p lamp-voice -p lamp-voice-eval
cargo test --locked --offline -p lamp-gemini --lib soak -- --ignored --nocapture
cargo test --locked --offline -p lamp-voice --lib follows_output_capacity -- --ignored --nocapture --test-threads=1
```

The last two are the prolonged conversations (about 150 s of CPU) and the
real-time 60 s and 180 s answers (270 s). `lamp_gemini::testing` is the
scripted service: in-memory, no TLS, socket, credential or cloud request.
Transport scenarios run on a paused runtime clock, so their timings are exact
rather than load-dependent. Worker scenarios run in real time, because the IPC
and authority clocks cannot be paused.

In the speaking-speed tests the harness stands in for the coordinator. It
counts every packet with the shared `OutputReceiver`, grants capacity with the
shared formula from the free space of a reply buffer, and drains that buffer
at 24,000 samples per second of measured time. Time spent on hold or with
nothing to play is never made up. The scripted service writes 2 s messages at
ten times speaking speed, stalls for 3 s a third of the way in, then sends
10 s with no gap, and withholds its idle completion for the answer's own
length. The test fails if a block is lost, repeated or reordered, if a packet
exceeds granted capacity, if the buffer overruns or never fills, if playback
runs dry for 100 ms, or if playback ends at any time other than the answer's
length plus the hold. **The real coordinator loop and speaker are not in these
tests.**

### Host results

macOS x86_64, Rust 1.96.0, source-only export of merge commit `a09b305bc`
outside the repository, fresh target directory:

| Gate | Result |
|---|---|
| Formatting | pass |
| Strict Clippy, workspace, all targets | pass |
| Workspace tests, serial | 755 passed, 0 failed, 3 ignored |
| Release build of `lamp-voice` and `lamp-voice-eval` | pass |
| Prolonged conversations (ignored by default) | pass: 4 seeds x 400 turns, tallies unchanged |
| 60 s and 180 s real-time answers (ignored by default) | pass, run in the working tree on the same source |

`lamp-gemini` contributes 99 of those tests (81 unit, 6 setup, 12 codec) and
the provider worker 36. The count is higher than the 636 recorded for
`7a556b3ff` because the merge brought in the voice evaluator's suites.
`crates/gemini/**` is byte-identical to `7a556b3ff`.

### Linux/ARM64 results

Cross-compiled for `aarch64-unknown-linux-gnu` in a Debian bookworm container
on the x86_64 host (Rust 1.96.0, `aarch64-linux-gnu-gcc` 12.2.0), from a
read-only source-only export. Test binaries are real ARM64 Linux executables
run under `qemu-aarch64` 7.2 user-mode emulation. **This is not the device**: it
checks target compilation and the exercised code paths under emulation. It
does not establish device timing, ALSA hardware behavior or room audio.

All on merge commit `a09b305bc`:

| Gate | Result |
|---|---|
| Strict Clippy, workspace, all targets, ARM64 target | pass |
| Release build of `lamp-voice` and `lamp-voice-eval` | pass: `lamp-voice` is ELF 64-bit ARM aarch64, dynamically linked |
| `lamp-gemini` tests, debug and release builds | 99 passed, 0 failed, each |
| Provider worker tests, release build | 36 passed, 0 failed |
| 60 s real-time answer, release build | pass: playback ended at 70.02 s after a 10.00 s hold; backlog peaked at 30.00 s |
| Prolonged conversations, 4 seeds x 400 turns, release build | pass in 46 s; every tally identical to the host run |
| Provider worker tests, debug build | 33 of 36 passed |
| Workspace tests, release build, serial | 793 passed, 1 failed, 3 ignored |

The workspace count is higher than on the host because Linux-only suites run
there. Its one failure is outside this work:
`a_transport_that_closes_mid_trace_invalidates_the_attempt` in the voice
evaluator's `physical_loopback` suite expected an abort naming a partial
trace and got `session start failed: Broken pipe`. It passes on the host. It
has not been investigated here and belongs to that crate's owner.

The three debug-build failures were observed when the test harness missed
real-time deadlines. The harness plays the
coordinator: it must renew a 100 ms input lease and a 250 ms heartbeat while
the same single-threaded test process decodes megabytes of scripted audio in
unoptimized emulated code. When the heartbeat lapses the worker exits
(observed: broken pipe, connection refused); when output is not read in time
the harness gives up (observed: no provider output within 5 s). They are the
three tests that carry the most audio: the 80 s burst, the 20 s answer and
the interruption of a 40 s answer. Earlier commits failed two or three of the
same kind, varying between runs. The same binaries built with optimization
pass all 36 under the same emulation, and the debug build passes on the host.
Whether a debug build keeps these deadlines on the device itself is not known.

Measured under emulation, release build, same boundaries as the host table
below: interruption to first packet of the new answer 9 ms with 1 stale
packet; 80 s burst in 4.0 s; 20 s answer with a 4 s hold played out in
24.01 s.

### Measured on the host

Inside the test process, scripted service, real time unless marked virtual.
None of these is a network, answer-latency or audible measurement.

| Measurement | Boundary | Result |
|---|---|---|
| 20 s answer, 5 s reply buffer, 4 s playback hold | First tick to the last sample leaving a modeled speaker | 24.03 s; completion received at 20.00 s; service wrote it in 4.3 s; 500 packets; backlog peaked at 5.00 s; 3 ms run dry |
| 60 s answer, 30 s reply buffer, 10 s hold | Same | 70.03 s; completion at 60.01 s; written in 8.5 s; 1,500 packets; peak 30.00 s; 1 ms run dry |
| 180 s answer, 30 s reply buffer, 20 s hold | Same | 200.05 s; completion at 180.01 s; written in 21.2 s; 4,500 packets; peak 30.00 s; 1 ms run dry |
| Interruption of a 40 s answer under capacity | Admission to first packet of the new answer | 5 ms in each of 2 runs; 1 stale packet each, of at most 2 possible |
| 80 s of audio, capacity returned on receipt | Service write to local IPC reader | 4.0-4.3 s for 2,000 packets over 3 runs |
| Stop during a hung reconnect | Control datagram to worker task end | 2.6-3.8 ms over 3 runs; 2.4-24.2 ms over 10 runs before the merge |
| 60 turns with injected failures (virtual) | Seed 1 | 5.3 min; 27 completed, 14 cancelled, 5 settled, 13 lost, 27 recoveries |
| 4 x 400 turns with injected failures (virtual) | Seeds 2, 3, 5, 8 | 52-70 min each; 843 completed, 417 cancelled, 107 settled, 232 lost, 477 recoveries, 145 assumed barriers in total |

The 80 s burst took 1.5 s before the merge. The difference is the capacity
contract doing its job: with two packets outstanding and a reader on a 2 ms
cadence, delivery to the coordinator is limited to about twenty times speaking
speed in this harness.

In every scripted conversation each request ended in exactly one outcome, no
audio was delivered under another request's name, and the service received
each request's input exactly once across all sessions.

## Third round

Base: shared checkpoint `2535fb565` of `lamp-v2-chat`, fast-forwarded.
Assignment: items 1 and 3 of Agent 1's list in the
[completion plan](rust-completion-plan.md).

### What changed

| Commit | Branch | Files | Content |
|---|---|---|---|
| `67154e0b6` | `gemini-voice` | `crates/gemini/src/recovery.rs`, `recovery/tests.rs`, `recovery/tests/availability.rs`, new `recovery/tests/pressure.rs` | Retry work is no longer held back by an unread link update; five regressions |
| `155bab4e2` | `gemini-voice` | `crates/gemini/src/recovery/tests/soak.rs` | Adversarial conversations through both supervisor interfaces |
| `548fda500` | `gemini-voice-worker-proposal` | `crates/voice/src/provider_worker.rs` (tests only) | Stopped-playback measurement. A proposal: that file is under the main agent's lock |

Behavior change, one: [recovery does not wait for its
reader](#recovery-does-not-wait-for-its-reader). Nothing else in the runtime
is altered. `Dialing` became a struct with a failure flag; a test helper reads
`attempt.task` where it read `attempt.0`.

The defect was found by the adversarial conversation, not by reading. The
first runs of that test failed in a scenario that must recover: a speaker held
for longer than the outage budget while the first replacement attempt was
refused. Two focused tests reproduce it on the shared tree (one attempt made
where two, and three, were due) and pass with the fix.

### Review of `poll_link` / `next_output`

Read in full, with its tests. No defect was found by inspection other than
the one above. Points an integrator should know:

- `next_output` returns nothing while a link update is unread. A split
  consumer that stops polling the link therefore stops its own audio. That is
  the documented contract; it is an ordering guarantee and a liveness
  coupling at once.
- A `Ready` that became false before it was read is dropped, so a consumer
  can see two `Unavailable` in a row. Revisions skip and never go back.
- A request can be accepted on a connection whose `Ready` nobody has read
  yet. If that connection then dies, the request is reported lost exactly
  once, with that connection's session, and the next recovery is reported as
  `ResumedBeforeLatest`. Tested.
- An unread `Settled` or `TurnLost` can in principle outlive a local
  cancellation of the same request, which would report a cancelled request as
  lost. On the single-threaded runtime the worker uses, nothing can run
  between the two steps involved, so this was not reproduced and is not
  changed. It would matter on a multi-threaded runtime.

### Regressions added

| Test | What it pins |
|---|---|
| `refused_attempt_is_retried_while_the_outage_notice_is_still_unread` | The fix: two attempts, recovery stamped inside the budget, answer intact |
| `unread_outage_notice_does_not_lift_the_attempt_or_budget_bounds` | The fix does not loosen policy: three attempts of three, then `AttemptsExhausted`, stamped when it happened |
| `answer_of_a_dead_connection_survives_a_stopped_speaker_only_up_to_the_delivery_bound` | 29 s: whole answer, `Settled`. 31 s: an unbroken beginning, `TurnLost`, never `Settled` |
| `request_accepted_before_its_connection_was_announced_is_lost_exactly_once` | Third point above |
| `replacements_that_die_on_arrival_end_as_unstable_with_the_first_answer_intact` | Three dials for three allowed recoveries, then `Unstable`; the first answer whole, settled once |
| `adversarial_recoveries_account_for_every_request_through_both_interfaces` | 60 turns under the production `Require` policy, see below |
| `prolonged_adversarial_recoveries_account_for_every_request` (ignored) | Four seeds of 400 turns |

The adversarial conversation uses the same ledger as the earlier one: every
request's audio carries that request's own sample value, every accepted
request must end in exactly one outcome with every sample that arrived, and
its input must reach the service exactly once. It adds, chosen at random per
turn: a replacement that is refused once, is acknowledged up to 3 s late,
issues no new handle, or dies before anything is said on it; a speaker that
stops taking output for up to 20 s; a person who speaks into the outage, with
and without cancelling what was owed; an interruption of an answer whose
connection is already gone; and a handle the service withdraws. The service
answers connection attempts when they are made, as a real one does. Each
`Ready` must carry the context a model of the handle's provenance predicts.
The same seed is run through `next()` and through `poll_link()` with
`next_output()`, and both must produce the same row for every request.

The consumer that uses `next()` is held for at most 12 s, not 20: it cannot
read the link while its speaker is stopped, and past the outage budget the
second [remaining limit](#recovery-does-not-wait-for-its-reader) ends the
link. That is the limit, stated, not a tolerance in the test.

### Host results

macOS x86_64, Rust 1.96.0, source-only export of `548fda500` outside the
repository, fresh target directory. That commit is `gemini-voice` plus the
worker measurement, so one run covers both branches; the measurement adds
only ignored tests.

| Gate | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --locked --offline --workspace --all-targets -- -D warnings` | pass |
| `cargo test --locked --offline --workspace --no-fail-fast -- --test-threads=1` | 849 passed, 0 failed, 11 ignored |
| `cargo build --locked --offline --release -p lamp-voice -p lamp-voice-eval` | pass |
| `cargo test --locked --offline -p lamp-gemini --lib soak -- --ignored --nocapture --test-threads=1` | 2 passed in 553 s: the earlier prolonged conversation with unchanged tallies, and the adversarial one |
| `cargo test --locked --offline -p lamp-voice --lib follows_output_capacity -- --ignored --nocapture --test-threads=1` | 2 passed in 270 s: the 60 s answer finished playing at 70.03 s, the 180 s answer at 200.05 s |
| `cargo test --locked --offline -p lamp-voice --lib stopped -- --ignored --nocapture --test-threads=7` | 7 passed in 118 s; see [a stopped speaker](#a-stopped-speaker) |

`lamp-gemini` contributes 136 of the 849 (118 unit, 6 setup, 12 codec). The
eleven ignored tests are the two prolonged conversations, the 60 s and 180 s
answers and the seven stopped-playback trials.

| Measurement (virtual clock unless marked) | Result |
|---|---|
| 60 adversarial turns, seed 1, each interface | 13.3 and 13.8 min; 23 completed, 18 cancelled, 5 settled, 14 lost, 13 refused, 59 recoveries, 12 refused attempts, 11 dead on arrival, 10 speaker holds |
| 4 x 400 adversarial turns, seeds 2, 3, 5, 8, each interface | 343 and 352 min in total; 679 completed, 386 cancelled, 225 settled, 309 lost, 249 refused, 1,453 recoveries, 234 refused attempts, 231 dead on arrival, 341 speaker holds; identical rows through both interfaces |
| Playback stopped with every holder full (real time) | See [a stopped speaker](#a-stopped-speaker) |

### Linux/ARM64 results

On `0f688994d`, which is this round's code plus the later audio work,
cross-compiled and run under `qemu-aarch64` user-mode emulation from a
read-only source-only export. **Not the device.**

| Gate | Result |
|---|---|
| Strict Clippy, workspace, all targets, ARM64 target | pass |
| Release build of `lamp-voice`, `lamp-voice-eval` and the `service_probe` example | pass; ARM aarch64 ELF |
| `lamp-gemini` tests, release build | 136 passed, 0 failed, 2 ignored |
| Provider worker tests, release build | 44 passed, 0 failed, 2 ignored |
| Prolonged adversarial conversations, release build | pass; every tally identical to the host run through both interfaces |

Not run on ARM64 for this round: the whole-workspace suite, the debug
profile, and the real-time 60 s, 180 s and stopped-playback tests.

### Not run for this round

- **Cloud.** Nothing. The five probes are unchanged and still have not run.
- **Device.** Nothing.
- The real coordinator. Every test here drives the supervisor or the worker
  directly.

### Open after this round

- The two [remaining limits](#recovery-does-not-wait-for-its-reader) of a
  consumer that does not read the link. They disappear when the worker polls
  the link on its control tick.
- A stopped speaker past 28 s [ends the session](#a-stopped-speaker), and the
  failure surfaces only after playback resumes.
- Production keeps `BarrierPolicy::Require`. A person who continues before
  any answer has started still ends the session after 2 s if the service
  never confirms the cut-off request. Only the `interrupt-before-output`
  probe can say whether it does.

## After the third round: two requests from the integrator

Both read from `coordination/main.md` at `51bd815e9`.

| Commit | Files | Content |
|---|---|---|
| `e7b8d24b2`, renamed in `ee5387da7` | `crates/gemini/src/recovery.rs`, `lib.rs`, `recovery/tests.rs`, new `recovery/tests/checked.rs` | `MAIN-008`: `Supervisor::try_start_for_session(expected, request)` and `StartRefusal` |
| `f085868f3` | `crates/gemini/src/lib.rs`, `recovery/tests/pressure.rs` | `MAIN-007`: `Error::Backpressure` is reconnected; three regressions |

**Checked start.** A decision to start a request is taken while one
connection is ready and may reach the supervisor after a recovery. The
unchecked `try_start` would then send it to the replacement, which the caller
never chose. `try_start_for_session` names the session, compares it with the
ready connection in the same synchronous call, and refuses before anything is
queued or any earlier request retired:

| `StartRefusal` | Meaning |
|---|---|
| `NotConnected` | No connection is ready |
| `SessionMismatch { live }` | A connection is ready and it is not the one named; `live` is the one that is |
| `Refused(error)` | The right connection refused, as `try_start` would |

A refusal sends nothing, supersedes no earlier answer and spends no request
number. `try_start` is unchanged for existing callers. Audio and `End` were
already bound to the exact session and request that started.

| Test | What it pins |
|---|---|
| `start_chosen_for_a_replaced_connection_is_refused_before_anything_is_sent` | A start chosen for connection 1 arriving after connection 2 took over: `SessionMismatch`, the replacement hears nothing, the unread answer of connection 1 is delivered whole and settled, and the same request then starts on connection 2 |
| `a_refused_start_leaves_the_request_on_the_live_connection_untouched` | With request 2 already speaking on the replacement, a late start for request 3 chosen for the dead connection is refused; request 2 keeps sending audio and ends normally; the service hears nothing of request 3; its number is not spent |
| `a_request_lost_before_its_start_was_written_is_reported_without_input_started` | A connection that cannot take another byte: `try_start`, and `try_start` + audio + `try_end`, are accepted locally, the `activityStart` write times out, and the request is reported lost at `InputOpen` and `AwaitingResponse` with no `InputStarted` before it |
| `checked_start_never_reaches_a_connection_other_than_the_one_named` | A session that does not exist yet; the ordinary refusals on the right session; `NotConnected` throughout an outage; no request number spent |
| `a_stopped_speaker_past_the_delivery_bound_costs_the_answer_not_the_session` | One replacement with the handle, `Resumed`; an unbroken beginning of at most 16 events; one `TurnLost { Responding, Backpressure }`; nothing sent again; the next request served |
| `a_stopped_speaker_still_ends_a_session_that_cannot_resume_its_context` | No handle and context required: no attempt, the request still reported once, then `NoResumableContext` |
| `input_that_overruns_its_queue_loses_that_request_and_recovers_the_link` | The 33rd unserviced block: `Backpressure`, one `TurnLost { InputOpen }`, recovery, the rest of that request's input refused |

**A lost request may have no `InputStarted`.** The stage is recorded when the
caller's `try_start` (and `try_end`) is accepted, which is synchronous.
`InputStarted` is emitted only once `activityStart` has been written. A
connection that fails between the two reports `TurnLost { InputOpen }`, or
`AwaitingResponse` if the `End` was accepted as well, with no `InputStarted`
before it. That is the producer's contract, confirmed and now tested; a
consumer must accept that exact loss without having seen the start.

Host results are in
[Agent 1's status](../coordination/agent-1.md).

## Service evidence reported by the integrator

Not run by Agent 1. The integrator ran the probe tool built from `64806f862`
on board `lamp-a0ae` against the deployment endpoint (`campaign-api`, model
`gemini-3.8-live-extended-thinking`, voice `Kore`, thinking `LOW`), opening no
hardware, and reported in `MAIN-011`:

| Probe | Reported | What it settles |
|---|---|---|
| `handles` | No handle after setup; a handle after one answer, marked current | Resumption handles are issued without being asked for. `Resumption::Retain`, as the worker is configured, has something to retain |
| `interrupt-before-output` | Only request 2 produced events: first audio 3,387 ms after the second start, generation complete and idle completion at 5,538 and 5,539 ms. No `interrupted`, no terminal for request 1, no assumed barrier | **Nothing.** See below |

The second result is not evidence about interruption. The probe queued the
first `End` and the second `Start` back to back. `InputSender::try_start`
retires earlier requests synchronously, so the second start overtook the
first `End` inside the client: request 1 was treated as cancelled before it
was committed, no `activityEnd` was sent for it, and both recordings went to
the service as one question. The service was never asked to interrupt
anything. The integrator suspected exactly this.

Since `d938a3101` the probe waits 300 ms after the first `End`, records what
the service sends meanwhile, and reports
`first_request_committed_before_second_start` (taken from the transport's own
`waiting_for_barrier` on the second start) and a `trial` label:
`interrupt_before_output`, `interrupt_during_output` or `not_an_interruption`.
Only the first label is a trial of the case in question. The `resume` probe
now records `resumed_setup_ms` when setup completes, and times the resumed
answer from its own question.

**The barrier policy stays `Require`.** No service evidence yet says what the
service does when interrupted before it has answered.

## Integration notes

Branch `gemini-voice`. It contains shared checkpoint `4b8436d23` of
`lamp-v2-chat` by merge:

| Commit | Content |
|---|---|
| `fbf9ecc3c` | First round: ownership rules, delivery, bounded recovery |
| `9f22e541d` | Output buffer for speaking speed, drain after failure, barrier model and policy |
| `55193bb3c` | Supervisor recovers while output drains; worker purge, typed statuses |
| `a7b575918` | Service probe tool |
| `0489ef5d7` | Scripted audio built outside the real-time window in two worker tests |
| `7a556b3ff` | Bound on an interrupted response that keeps producing |
| `93a30b3cd` | This document, first version |
| the merge after it | `lamp-v2-chat` `4b8436d23`; the worker moves from its provisional seam to the shared output-capacity contract |
| the commit after that | This document, for the merged tree |

Changed files: `crates/gemini/**`, `crates/voice/src/provider_worker.rs`, this
document, and the barrier sentence in `docs/live-runtime.md`. No shared
manifest, lock file, coordinator, admission, audio or choreography file is
touched, and `ProviderInput` / `ProviderOutput` are unchanged on the wire.

The merge conflicted only in `provider_worker.rs`. Everything the shared
change added there is kept as it was: the capacity sender, the packet-size
constant, the pruning rule and its tests. Two things differ from the shared
version, both because this worker's queue also carries statuses: the pruning
rule runs over that queue and never removes a status, and its unit test
checks that. The worker's own speaking-speed tests now use the shared
`OutputReceiver` exactly as the coordinator does.

The initial `fbf9ecc3c` transport had a two-second no-consumption watchdog.
The bound is now 30 s and the 60 s and 180 s delivery tests pass on the host.
The shared [flow-control notes](provider-flow-control.md) describe the updated
bound. [Root integration qualification](provider-recovery-integration.md)
records the combined source and distinguishes modeled playback from real
coordinator, speaker and room behavior.

What an integrator will notice:

- `lamp_gemini::Event` gained `BarrierAssumed`; `Discard::UnansweredBarrier`
  is gone. `Notice` gained `Unavailable`. Only the provider worker matches them.
- `Timeouts::deliver` is 30 s. `SessionConfig` gained `barrier_policy` and
  `output_buffer_seconds`.
- Agent 1's branch selected `BarrierPolicy::AssumeAfterQuiet`. Shared integration
  retains `Require` pending the service probes and explicit ownership handling.
- Provider memory can reach the output bound (14.4 MB) during a long answer.
- A recovery writes to the worker's inherited standard error.
- Do not build two checkouts into one cargo target directory: workspace crates
  share build identity by relative path and a stale test binary can run
  without warning.

## Required before relying on this

- The five service probes, on an endpoint with real credentials.
- A native build and test run on the device. Emulated execution covers the
  tested Linux/ARM64 paths; it does not establish general architecture
  independence, device timing or hardware behavior.
- A physical cohort that includes: an idle pause longer than 200 s followed by
  a follow-up that depends on earlier context; a question continued after the
  endpointer cut it; two interruptions in a row; a topic change before the
  answer starts; an answer of at least 60 s; a network link dropped while idle
  and mid-answer. Record every `provider_ready`, the worker's standard error
  and the room audio.
- Echo-driven false admissions are outside this work and unchanged by it. Each
  one still cancels the answer in progress.

## Where a barrier's wait starts, and what a barrier reports (opt-in)

Two settings on `SessionConfig`, both off by default. Neither changes the
policy, which stays `Require`, and neither assumes a settlement.

**`barrier_wait(BarrierWait::AtCommit)`.** By default the quiet bound of
[When the service does not confirm](#when-the-service-does-not-confirm)
counts from the interruption, so an earlier response that stays silent for
two seconds ends the connection while the next person is still speaking,
with their question half uploaded. With `AtCommit` the quiet bound counts
only while a request's `activityEnd` is being withheld, and from no earlier
than the moment it was first withheld. While nobody is waiting to be
committed, only the absolute bound applies.

| | `FromInterruption` (default) | `AtCommit` |
|---|---|---|
| Quiet bound starts | At the interruption, then at each event of the earlier response | At the later of those and the moment the next `activityEnd` was withheld |
| The next person is still speaking | Can time out under them | Cannot, short of the absolute bound |
| A pause inside their utterance | Counts | Does not count |
| A second continuation behind the same barrier | Counts on | Restarts at that request's own End |
| The waiting request is cancelled | Counts on | Nothing waits; only the absolute bound |
| Absolute bound, `Timeouts::response` (30 s) from the interruption | Kept | Kept |
| What settles a barrier | The earlier response's idle completion | The same |

What is committed, when, and to whom output is attributed are unchanged.
Output belongs to the current request only after its `activityEnd` has been
written, and that End is written only once no barrier stands. Until then
everything received is the earlier response's: dropped, and counted.

**`barrier_reports(true)`.** One `Event::Barrier` when a barrier ends by
settling (`Settled`) or by its timeout (`TimedOut`, emitted before the
`BarrierTimeout` that follows). It carries which request was superseded
and which waited, how long the barrier stood, how long it had been quiet,
whether the waiting request was still speaking, how long its End had been
withheld, and counts of what arrived for the earlier response and was
dropped: audio frames and samples, text frames, generation completions,
`interrupted`, and completions that were not idle. Counts only: no audio
and no text. It is what tells a terminal that never came from one that
came and was dropped. A stop by the owner and a close by the service end
the connection as themselves and report no barrier outcome.

### What this does and does not make work

Two real-service runs by the integrator, one each, on synthetic cached
speech, no microphone or speaker:

| Run | Next request started | What the service sent for the earlier response |
|---|---|---|
| `continue-before-output`, 2 s bound | 303 ms after the first End, before any output | Nothing exposed in the 2.0 s the session lasted. `BarrierTimeout`, 27,680 of 58,671 samples uploaded |
| `continue-during-output`, 2 s bound | 0.8 ms after the first audio was received | `interrupted` 358 ms and the idle completion 363 ms after the start. Settled; the second request was committed and answered |

So, on one attempt each: a response interrupted **while producing** is
confirmed promptly, and a response interrupted **before it produced
anything** was not confirmed inside two seconds. The first trace shows no
terminal exposed in that window. It does not show that none was sent or
that none would have arrived later, and the service's own voice activity
notices in it are not acknowledgements of this client's End or Start.

`AtCommit` therefore changes this much and no more:

- A barrier that would settle while the next person is still speaking, or
  within the bound after they stop, now settles, where before it could end
  the connection in the middle of their utterance.
- A barrier that never settles still ends in `BarrierTimeout`, later: the
  bound after their End, with the whole utterance uploaded and its End
  never written. The question is lost exactly as before. Nothing here
  makes an interruption before output succeed.

Whether a terminal for a response interrupted before output arrives late
or never is not known. The probe's `barrier-ms=5000` watches three seconds
longer than production and no more.

### On the probe

On a build with this library the strict probe also takes a trailing
`barrier-wait=at-commit` (after `barrier-ms=N` when both are given), turns
barrier reports on, and records each as an event row of kind `barrier`
with the counts above. `barrier_wait` and `barrier_wait_is_default` in the
result say which was used. A run with `barrier-wait=at-commit` under the
two second bound can pass and exit 0: that is a pass of this opt-in
setting, which it would need before being turned on, and not a pass of
production's default.

## Keeping the next request's start until the earlier answer begins (opt-in)

`SessionConfig::successor_start(SuccessorStart::AfterFirstOutput { patience })`.
Off by default. The policy stays `Require`; nothing is assumed, nothing
committed is replayed.

### What the service's documentation gives, and what it does not

Read on 2026-10-11 from the Live API reference
(`https://ai.google.dev/api/live`) and guide
(`https://ai.google.dev/gemini-api/docs/live-guide`). These are the
documentation's statements, not observations of the service. The quoted
words came through a page-reading tool that shortens quotations: check
them against the pages before resting anything on one word.

| Documented | What it says |
|---|---|
| `interrupted` | "a client message has interrupted current model generation". An interrupted turn has no `generationComplete`; `interrupted` is followed by `turnComplete` |
| Start of activity | With the default `activityHandling`, "start of activity will interrupt the model's response", cut off "in the moment of the interruption" |
| `interactionStatus` | "Always sent alongside turnComplete"; `IDLE` or `IN_PROGRESS` |
| `waitingForInput` | "the model is not generating content because it is waiting for more input" |
| `sessionResumptionUpdate.resumable` | Resumption is not possible "when the model is executing function calls or generating" |
| `voiceActivity` | "Voice activity detected in audio stream", with an audio offset. Not described as an acknowledgement |
| `activityHandling: NO_INTERRUPTION` | "The model's response will not be interrupted" |

Not in either document:

- Any acknowledgement of a client's realtime input, or of which client
  message the service has consumed.
- Any message that confirms the cancellation of a response that has not
  begun to produce output. The guide says an interrupted generation "is
  canceled and discarded" and that the server reports the interruption; it
  does not say whether it does so when nothing had been produced.

So the documentation defines no stronger settlement signal than the one
this transport already waits for, and none at all for the case that
failed. `waitingForInput` and the resumption flag are the nearest: each
says something about whether the model is generating, neither is
documented as following an interruption, and neither is used to decide
anything here. They are counted while a barrier stands and reported, so
that a probe run can show whether the service sends them in that case.

What this strategy relies on, kept apart:

| | Source |
|---|---|
| A start of activity interrupts a response that is producing, and `interrupted` then `turnComplete` follow | Documented |
| That confirmation arrives promptly | One probe run: `interrupted` 358 ms and idle 363 ms after the start. One attempt, not a rate |
| It still arrives when the start is followed at once by a burst of kept audio | One run on a Lamp, below: 10,400 kept samples behind the start, `interrupted` and idle 465 ms after it. One attempt |
| A response that has produced nothing is not confirmed when interrupted | Two runs: nothing exposed in 2.0 s, and nothing at all in 6.2 s. Not documented either way |

### Three real-service runs, 2026-10-11

Run by the integrator on a Lamp with the probe built from `901622831`
(SHA-256 `08a80e49…5551`), the credential used in place, cached synthetic
speech, no microphone or speaker. One attempt each. Their receipt is
`f2109d30a499adc58cdbabc9ed9020d607467f8df0a05a224d3d517ce51494f1`.
Times are since the first start was queued.

| | At-commit wait only | Kept start, before output | Kept start, during output |
|---|---|---|---|
| Second start | queued 4.099 s, 301 ms after the first End | handed in 4.089 s, 301 ms after the first End | queued 5.148 s, 1.3 ms after the first audio |
| Start kept | not asked for | **717 ms, 10,400 samples**, released by the first output | nothing kept: the answer had begun |
| For the first response, while the barrier stood | **nothing: no audio, no `interrupted`, no completion, in 6.18 s** | 6 audio frames dropped, `interrupted` and idle at 5.272 s, 465 ms after the start went out | 8 audio frames dropped, `interrupted` 361 ms and idle 402 ms after the start |
| `waitingForInput`, resumption updates | 0, 0 | 0, 0 | 0, 0 |
| Second End | withheld 2.003 s, never written | written 8.261 s | written 9.313 s |
| Outcome | `strict_barrier_timeout` | `strict_barrier_confirmed_and_answered`; first audio of the answer 955 ms after the End | `strict_barrier_confirmed_after_audio_and_answered`; 960 ms after the End |

What this is: the case that failed, failing again under the at-commit
wait alone, and succeeding once with the start kept, burst included. What
it is not: a rate, a latency of anything a person hears, or a statement
about a microphone, an endpointer or a speaker. Neither documented signal
appeared in any of the three, so on this evidence neither is a
settlement signal for the case that fails. The probe that ran is the one
before the retirement fix below; none of these cases retires a kept
request.

### The rule

When a committed request is superseded and its response has produced
nothing, the next request's `activityStart` is not written. Its audio is
kept here, in order, and the first of these ends that:

| Ended by | What is written | Reported as |
|---|---|---|
| First output of the earlier response | `activityStart`, the kept audio in order, then audio as it is spoken. The earlier response is now interrupted while producing, and the barrier's quiet bound counts from here | `FirstOutput` |
| Idle completion of the earlier response with no output | The barrier is down. `activityStart`, the kept audio, and the End as soon as it is asked for and every kept block is out | `Completion` |
| `patience` | `activityStart` and the kept audio. From here it is the default path exactly: the strict wait, and `BarrierTimeout` if nothing confirms | `Patience` |
| More than `MAX_RETAINED_SAMPLES` kept | The same as for patience | `Overflow` |
| A stop, a close, any failure | Nothing. What was kept is dropped unsent | The stop or the failure itself |

A request retired while it is kept, or while what was kept is being
written, is never committed. Retirement is read again immediately before
an End is handed to the socket, because the writes that lead there are
awaited and a retirement can be recorded during any of them. The limit
is the hand-over itself: once the End frame is inside the WebSocket layer
it goes out with the next flush, and a retirement recorded after that
finds a request that has been committed. It is then what a committed
request that is superseded always was: interrupted, and settled before
the next commit. A healthy connection is not abandoned to take a frame
back.

A response that has already produced output when it is superseded is
interrupted at once, as before. With `barrier_reports` on, the barrier's
report says how long the start was kept, what ended it and how many
samples were kept.

**For a request that is continued it changes when bytes are written and
never which.** The same `activityStart`, the same audio in the same order
and the same single `activityEnd` go out as on the default path. A
request superseded before its End keeps its audio in the activity its
successor continues. A request that is abandoned is different, and is
told apart from one that is continued: what is still kept of it is
dropped and never written. See
[Continued or abandoned](#continued-or-abandoned-what-becomes-of-kept-input).

### Bounds and outcomes

| Bound | Value | Past it |
|---|---|---|
| `patience` | Set by the caller, 1 ms to 4 s (`MAX_SUCCESSOR_PATIENCE`) | The start is written; default path from there |
| Audio kept | 96,000 samples, 6 s (`MAX_RETAINED_SAMPLES`) | While keeping: the start is written. While writing out and still over it: `Backpressure` |
| Age of kept audio when written | `MAX_INPUT_AGE` + `patience`, by its own capture time | `StaleInput`; the connection fails |
| Age of audio when handed in | `MAX_INPUT_AGE`, 1 s, unchanged | `StaleInput`, as before |
| Kept audio written per turn of the session loop | 32 blocks | The rest follows on the next turn; commands, incoming messages and a stop stay live |
| From supersession to settlement | `Timeouts::response`, 30 s, unchanged | `BarrierTimeout` |

The third row is a deliberate widening and the only one: audio this
library itself held is written up to `patience` later than audio is
otherwise allowed to be. It is not relabelled, a stop drops it, and with
the setting off nothing changes.

**Truthful failure.** If neither output nor completion comes within the
patience and the service then confirms nothing, the outcome is the same
`BarrierTimeout` as today, with a report saying the start was kept, for
how long, and that patience ended it. The question is then lost, and the
caller is told so, as it is today: nothing is committed on a guess. What
the person then hears, if anything, is the coordinator's own handling of
a lost turn and is not changed here.

### Whose the output is

The existing argument, which keeping the start does not touch:

1. Output is attributed to a request only if it is current and its
   `activityEnd` has been written. Everything else is the earlier
   response's: dropped, and counted under its own name.
2. A request's `activityEnd` is written only when no barrier stands and
   nothing kept remains unwritten.
3. Under `Require` a barrier is taken down only by the earlier response's
   idle completion.
4. While the start is kept, the next request is current and not committed,
   and the barrier stands from the supersession. So nothing received in
   that time can be attributed to it, before or after the kept start goes
   out.
5. Therefore the first output attributed to the next request arrives
   after its End, which was written after the earlier response's idle
   completion.

The assumptions are the ones the transport already made: that an idle
completion is the last thing a response sends, and that the service
produces nothing for an activity before its End.

### Tests and what they do not show

`session::tests::retain`, against the in-memory service in real time:

| Case | Test |
|---|---|
| Opening words kept, then in order behind the interruption | `the_opening_words_wait_for_the_earlier_answer_to_begin_and_then_follow_in_order` |
| Long next utterance, paced as spoken, nothing lost or repeated | `a_long_next_utterance_is_kept_only_until_the_answer_begins_and_loses_nothing` |
| Two continuations, one start, one End | `two_continuations_are_kept_in_order_behind_one_start_and_one_end` |
| Completion instead of audio | `an_earlier_answer_that_completes_with_nothing_lets_the_next_request_start_normally` |
| Stale and unasked events do not release the start; the two documented signals are counted | `events_of_the_earlier_answer_that_are_not_output_do_not_release_the_start` |
| Quota close while keeping | `the_service_closing_for_quota_while_the_opening_is_kept_ends_as_that_and_writes_nothing` |
| Stop while keeping, and while writing out | `a_stop_while_the_opening_is_kept_writes_none_of_it`, `a_stop_while_kept_audio_is_being_written_ends_the_writing` |
| Slow consumer | `a_consumer_that_takes_no_events_does_not_keep_the_opening_back` |
| Patience, then the strict timeout | `patience_runs_out_and_the_start_is_written_with_the_strict_wait_from_there` |
| Overflow | `more_than_may_be_kept_is_written_at_once_and_in_order` |
| Cancelled while kept | `a_kept_request_that_is_cancelled_is_never_committed_and_what_was_kept_stays_in_order` |
| Already producing: nothing kept | `an_answer_that_has_already_begun_is_interrupted_at_once_as_before` |
| Off by default, bounds | `without_the_setting_the_start_is_written_at_once_and_reports_say_nothing_was_kept`, `the_patience_is_bounded_and_off_is_the_default` |

Also `retain::a_request_cancelled_while_its_kept_start_is_being_written_is_not_committed`
and its superseded twin, the integrator's
`stop_fence::review_retirement_during_pending_audio_does_not_commit`, and
`retain::a_retirement_recorded_while_the_end_itself_waits_on_the_transport_comes_too_late`
for the limit.

Not shown by any of this: more than one attempt at anything on the real
service; the age bound being reached (no test holds audio that long);
a kept start through a microphone, an endpointer and a speaker.

### In the provider worker: `--provider-continuation kept-start`

One explicit option, off by default. `off` adds nothing to the provider
worker's command line and the worker runs exactly as before. `kept-start`
selects three things together, for the worker's lifetime:

| Setting | Value |
|---|---|
| `SuccessorStart::AfterFirstOutput` | patience 2,500 ms (`KEPT_START_PATIENCE` in `provider_worker.rs`): a proposed value |
| `BarrierWait::AtCommit` | the quiet bound counts only while an End is withheld |
| `barrier_reports` | how each barrier ended, on the worker's standard error, counts and times only |

`BarrierPolicy::Require`, every timeout, the input and output queue
bounds, capture times and the 1 s age bound for audio as it is handed in
are unchanged. Nothing committed is replayed.

Integration, in three places and no others:

1. `options.rs`: `DirectedOptions::provider_continuation`, parsed from
   `--provider-continuation off|kept-start`.
2. `coordinator.rs`: the worker's arguments gain
   `--provider-continuation kept-start` before the camera context, and
   `run_start` records `provider_continuation`.
3. `main.rs`: the provider worker takes that flag off the front of its
   flags and calls `provider_worker::run_with_continuation`.

To try it: `lamp-voice serve CONFIG OUTPUT --provider-continuation
kept-start`, with whatever else the session already uses. A reply
fixture (`directed-fixture`) refuses `kept-start`: it has no Gemini
session to apply it to, and its worker takes exactly its WAV and digest.

`provider_worker::tests::kept_start`, through the worker's own intake,
hand-off and session against the scripted service, input at 10 ms a block
with its own capture times:

| Case | Test |
|---|---|
| A resumed question is kept, and exactly it is delivered: one start, its blocks in order, one End; the first answer's audio is never the coordinator's | `a_resumed_question_is_kept_until_the_first_answer_begins_and_exactly_it_is_delivered`, and `a_resumed_question_is_kept_and_delivered_through_the_coordinated_contract`, which is the contract the command line starts the worker with |
| Two resumed utterances: one question, one End | `two_resumed_utterances_are_delivered_in_order_as_one_question_with_one_end` |
| The first answer completes with nothing | `a_first_answer_that_completes_with_nothing_lets_the_resumed_question_start` |
| Stop, and mute, while kept, under both the legacy and the coordinated contract | `a_stop_or_a_mute_while_the_question_is_kept_writes_none_of_it` |
| Stop while being written out | `a_stop_while_the_kept_question_is_being_written_ends_the_writing` |
| The service closes for quota while kept: nothing written, nothing replayed to a new connection | `the_service_closing_while_the_question_is_kept_writes_none_of_it_and_replays_none` |
| No first answer: patience, then the strict timeout, no End | `with_no_first_answer_the_patience_ends_in_the_strict_timeout_and_no_end` |
| An abandoned kept question, and the boundary once its words are out: see "Continued or abandoned" below | `an_abandoned_kept_question_is_dropped_unsent_and_the_next_one_is_heard_alone`, `an_abandonment_after_the_words_were_written_is_reported_as_sent_not_discarded` |
| Without the option, as before | `without_the_option_a_resumed_question_is_written_at_once_as_before` |

The other cases run under the legacy contract only. Overflow is not among them and cannot be reached through the worker at
the rate speech arrives: the buffer holds 6 s and the patience ends at
2.5 s. It is tested in the session.

### Continued or abandoned: what becomes of kept input

Only with `--provider-continuation kept-start`. Without it nothing is
ever kept and none of this changes anything.

A request that is retired before its End was either continued or given
up, and the two must not be treated alike: a person who goes on speaking
must keep their words, and a request the coordinator has abandoned must
not have its kept audio sent after the fact.

**Where the difference comes from.** The coordinator ends every turn in
one function, `cancel_reply`, with a reason, and the authority it then
publishes already says which case it is:

| The coordinator | Authority the worker then receives | Meaning |
|---|---|---|
| `commit_admitted_input`: cancels as `user_interrupted` and admits the new input at once, publishing nothing in between | a newer owner | **continued**: the person went on, or corrected themselves |
| `cancel_reply` for `deadline_cancelled`, `provider_interrupted`, `playback_discarded`, `service_stopped`, a runtime failure, or `user_interrupted` when the new input could not be given to the cloud | no owner | **abandoned** |
| `complete_turn` | no owner | finished; nothing of it is kept by then |

No new message from the coordinator is needed. The worker already
derives retirement from that authority; it now also notes whether a
retirement came with a newer owner or with none, and tells the transport
`retire` for the first and `discard` for the second.

**What the transport does.**

| | Continued (`retire`) | Abandoned (`discard`) |
|---|---|---|
| Audio still kept, not yet handed to the socket | Stays, in order, as the opening of what continues it | **Dropped. Never written** |
| Audio already handed to the socket, in an activity with no End | Stays | **Stays. Cannot be recalled, and is not called discarded** |
| Its End | Never written for it | Never written for it |
| The start that interrupts the earlier answer | Written when that answer begins | The same: the earlier answer still has to be stopped |
| Reported | nothing | `InputDiscarded` with `dropped_samples` and `sent_samples`, to the coordinator's trace as `provider_input_discarded` |

Abandoning a request also abandons the kept words it had continued: they
were one uncommitted thought, and neither part can be committed under the
other's End any more. `discard` is synchronous like `retire` and does not
wait for queue space. Order, capture times, both queue bounds, `Require`,
every timeout and the Stop fence are unchanged, and nothing is replayed.

**Cancellation ordering and memory bound.** The integrated correction from
`4d40fff3d` publishes retirement before abandonment, so new audio cannot enter
kept storage behind its purge. The provider keeps an independent abandonment
watermark even if multiple later owner changes arrive in one control drain.
Per-request sent-audio accounting exists only with kept-start and holds at most
64 entries in one open activity. Request 65 is refused before its audio reaches
the socket, with `OpenActivityOverrun` classified Fatal: one named failure, no
reconnect or replay. Explicit discard removes the entries it reports. The
immediate-start default has no ledger and no added request-count limit.

The complete 14-file code/test/document subset was composed with canonical
motor-binding source `274d5ad3c`. Host format and strict Gemini/live all-target
Clippy passed; 21 independently run production-path controls cover the bound,
publication ordering, late discard and real coordinator/worker IPC. Their 174-file
dependency closure is unchanged in the composed source. Agent 1's exact
`4d40fff3d` ARM emulation passed strict Gemini/live Clippy, 236 Gemini all-target
tests (two ignored), 41 focused live tests and a release build. The ARM artifact
was not deployed by these gates; the existing native conversation evidence
still names the earlier `2021c1bb9` runtime. No full combined workspace or real
abandonment test is claimed here.

**The transport boundary, exactly.** A block is droppable until it is
handed to the socket and not after. An abandonment recorded while a block
is waiting inside the socket layer does not take that block back: it is
written, counted in `sent_samples`, and the blocks behind it are dropped.
When `sent_samples` is not zero, those samples are in an open activity,
and the next request that continues that activity is heard after them,
under its own End. That is reported as it is. The connection is healthy
and is not abandoned to hide it, and no retry rule is changed.

> **Proposed separately, not built.** If audio of an abandoned request
> must never be heard at all, the only way once any of it has been sent
> is to replace the connection before the next request. The typed outcome
> above is what would trigger that: `sent_samples > 0`. It costs a
> reconnect and whatever context the handle does not restore, and it is a
> decision for the coordinator, not something the transport should do
> quietly.

Tests, at three levels:

| Level | Tests |
|---|---|
| Session, in-memory service, gated transport for the boundary | `retain::an_abandoned_kept_request_is_dropped_unsent_and_the_next_one_is_heard_alone`, `abandoning_a_continuation_takes_the_kept_words_it_continued_with_it`, `what_was_handed_to_the_socket_is_reported_as_sent_and_only_the_rest_is_dropped`, `an_abandonment_after_the_kept_words_were_written_drops_nothing_and_says_so`, `an_abandonment_does_not_wait_for_a_full_queue_and_fails_nothing`, `without_the_kept_start_an_abandonment_is_exactly_a_retirement` |
| Provider worker's own loop | `kept_start::an_abandoned_kept_question_is_dropped_unsent_and_the_next_one_is_heard_alone`, `an_abandonment_after_the_words_were_written_is_reported_as_sent_not_discarded` |
| **The coordinator's own `cancel_reply`, authority and `flush_provider_input`, with the Coordinated worker and Supervisor over IPC** (`provider_pair::continuation_or_discard`) | `continued_twice_the_words_are_one_question_in_order_and_nothing_is_discarded`, `abandoned_while_kept_none_of_it_is_sent_and_the_next_question_is_heard_alone`, `continued_then_abandoned_then_a_new_question_only_the_new_one_is_sent`, `abandoned_after_the_words_were_written_the_outcome_says_sent_and_not_discarded`, `abandoned_with_no_first_answer_only_the_start_is_written_and_late_output_is_nobodys`, `abandoned_with_no_output_credit_it_is_dropped_at_once_and_reported_when_credit_returns`, `a_stop_after_an_abandonment_writes_nothing_and_ends_cleanly` |

**Compatibility and migration.**

- Default off. With `--provider-continuation off` the worker's calls
  reach a session that keeps nothing: `discard` is `retire`, no
  `InputDiscarded` is ever produced, and the wire is byte for byte what it
  was.
- One new provider output, `input_discarded`, appears only with
  `kept-start`. A coordinator built without the arm for it would fail to
  decode it: the arm and the option must be integrated together. They are
  one shared commit, `f932d1155`, plus the tests in `ffd5a1e64`.
- The earlier candidate wrote a cancelled kept request's audio when the
  first answer began. This one does not. Nothing else about `kept-start`
  changes.

**What remains, after this.**

- Audio of an abandoned request that was already sent is still heard
  with the next request. Reported, not prevented.
- The coordinator's own queue of input not yet given to the worker is
  cleared on every cancellation, continued or not, as before: words still
  in that queue when a person goes on are the coordinator's to keep or
  drop, and this does not change what it does.
- Whether an owner follows a cancellation is read from authority in the
  order it is published. A coordinator that published an ownerless
  authority between cancelling and admitting a continuation would have
  that continuation treated as an abandonment. `commit_admitted_input`
  does not; the property is the coordinator's to keep.
- Mute while kept is covered through the worker under both contracts and
  through the real coordinator pair; no physical-switch test is claimed.
- No real-service run abandons a kept request.

Known limits of this candidate: the barrier report is a line on standard
error and not a typed event the coordinator records; the patience is a
constant; a kept start has had one real-service attempt; nothing here
has been through a microphone or a speaker.

### On the probe, and on a Lamp

A trailing `keep-start-ms=N`, 1 to 4000, last of the trailing settings,
turns the setting on for one run. The barrier row gains `start_held_ms`,
`start_released_by`, `retained_samples`, `waiting_for_input`,
`resumable_updates` and `unresumable_updates`; the result says
`keep_start_ms` and `successor_start_is_default`. A pass under it is a
pass of this setting and not of the default.

Build the Rust probe and run each case explicitly on a machine that already
holds the provider file and its credential. The private seven-case wrapper is
not part of standalone LampOS. For example, these are separate connections;
retain each result and exit status without automatically replaying a prompt:

```sh
cargo build --locked --release -p lamp-gemini --example service_probe
./target/release/examples/service_probe /path/to/provider.json FIRST.wav continue-before-output SECOND.wav 300 barrier-wait=at-commit keep-start-ms=2500
./target/release/examples/service_probe /path/to/provider.json FIRST.wav continue-during-output SECOND.wav barrier-wait=at-commit keep-start-ms=2500
```

The remaining cases are the default before-output control, its observation-only
five-second wait, retained before-output at 0 and 800 ms, and retained
`repeat-before-output 300`. The examples use only cached synthetic speech.

The credential is read in place and never copied or printed. The audio is
cached synthetic speech; no microphone or speaker is opened. The control
case is expected to end in the strict timeout and the five-second
observation never passes. What would qualify the setting is the three
`kept-before-output` cases and `kept-repeat` ending
`strict_barrier_confirmed_and_answered` with `start_released_by:
FirstOutput`, and `kept-during-output` unchanged, on more than one
attempt.

## Two additions to the strict probe (example only)

Neither changes the library. Both exist to answer what the failed run
below could not.

**`continue-during-output SECOND.wav [GAP_MS]`.** The same strict trial,
with the successor started only once the first request's own audio has
actually been received: one event at a time, a condition and not a time
guess, `GAP_MS` after that audio and at once by default. It is an
interruption of a response that is producing. The result's `case` is
`barrier_wait_observed_after_audio`; `interrupted_observed_us` and
`idle_observed_us` say when the first request's `interrupted` and idle
completion were exposed, if they were; the outcome is
`strict_barrier_confirmed_after_audio_and_answered` only when the idle
completion came after the second start, nothing was assumed and the second
request was answered.

**`barrier-ms=N`, trailing, 2000 to 5000.** Lengthens the barrier's quiet
bound for one run, to watch whether a terminal arrives late. Five seconds
is the longest the library accepts (`Timeouts::validate`), and the probe
does not widen that. It is observation only. The policy is still `Require`: nothing is assumed, the
successor's End stays withheld for as long as nothing settles, and no
answer can be attributed on the strength of the longer wait. The result
records `barrier_ms`, `barrier_bound_is_production: false` and
`observation_only: true`.

```sh
service_probe PROVIDER.json FIRST.wav continue-during-output SECOND.wav
service_probe PROVIDER.json FIRST.wav continue-before-output SECOND.wav 300 barrier-ms=5000
```

The second line is the failed run again with five seconds, not two, in
which a terminal for the first request may show itself: three seconds
more than production waits. It cannot say what happens later than that.

The first version of this option accepted up to 20000 and was first run
with `barrier-ms=10000`. The parser took it, the library refused the
configuration (`InvalidConfiguration`), and the run ended in 0.014 s with
nothing printed and nothing sent. That was a fault of the probe, whose
parser was tested and whose configuration was not, and says nothing about
the service. The accepted range is now the library's, and a test builds a
configuration from every bound the parser accepts.

**Exit status.** The probe exits 0 only for
`strict_barrier_confirmed_and_answered` or
`strict_barrier_confirmed_after_audio_and_answered` under production's two
second bound. A run with `barrier-ms` other than 2000 always exits non-zero
and says it is observation only, whatever it saw, so that a gate reading
exit status cannot be satisfied by a longer wait than production allows.
Its JSON is still printed in full.

## Actual strict continuation probe, 2026-10-11

The first real service run of `continue-before-output` with a 300 ms continuation
gap failed with `BarrierTimeout` during the continuation upload. Only 27,680 of
58,671 successor samples were accepted, and its End was not queued. No reply audio
was received. Keep speculative early endpointing disabled: the required settlement
behavior is not qualified. The [sanitized result and limits](verification/strict-continuation-20261011/README.md)
distinguish observed public events from unavailable wire acknowledgements and
explain why this functional probe is not a latency benchmark.

## Main integration verification, 2026-10-11

The opt-in worker/CLI path and both review corrections are composed with Main's
speaker-acknowledgment safeguard. Main's standalone host gates pass 697 combined
Gemini/live library tests (six existing ignored); emulated ARM gates pass 42
selected tests and the release build. Defaults remain off and the installed
pilot is unchanged. See [exact integration evidence](verification/provider-continuation-integration-20261011/README.md)
and its explicit transport and cancelled-audio limits.
