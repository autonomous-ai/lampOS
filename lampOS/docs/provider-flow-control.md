# Provider output capacity

The coordinator keeps at most 720,000 queued PCM samples (30 seconds at 24 kHz).
That is a backlog limit, not a maximum answer duration. Before this change a
fast answer could overflow the limit and end the directed session. Pausing IPC
reads was not a valid fix: a datagram already sent expires after 100 ms.

The new coordinator/worker contract reserves space **before sending**. It keeps
the existing two-packet/2 ms receive slice, continues receiving output on every
tick, and retains the ordinary 100 ms transport freshness rule. Slow playback
holds unsent output in the provider's bounded queue. It does not block capture,
authority publication, interruption, or the direct render-reference channel.

## Contract

`Control::ProviderOutputCapacity { through }` carries a cumulative output packet
limit for this worker incarnation. It is private control metadata, not an
actuator permit, admission decision, or heartbeat. Other workers reject it.
Build the coordinator and all workers together; do not mix protocol versions.

- The sender begins with capacity for two packets, allowing startup `Ready`.
  Every successfully sent output consumes one packet, including setup,
  transcripts, lifecycle events, and audio. `WouldBlock` consumes nothing.
- A new grant must be nondecreasing and no greater than `sent_packets + 2`.
  An identical grant is idempotent. Counter exhaustion fails rather than wraps.
- The receiver counts every decoded output, including old-owner PCM it discards.
  It preserves these counters from startup through new turns and reconnects.
- After processing output and dispatching speaker PCM, the coordinator grants:

  ```text
  received_packets + min(2, floor((720000 - reply_pcm_samples) / 960))
  ```

  Each output packet reserves the maximum 960 audio samples, even when it
  contains fewer samples or only metadata. There are at most two unreceived
  reservations. Failed nonblocking grant delivery is retried with the newest
  cumulative value, without appending an application control queue.
- The provider processes urgent control and microphone input before sending.
  It discards unsent output for retired requests without changing surviving
  ordering or assigning an old callback a new owner. Two already-sent packets
  may still arrive; the coordinator counts and discards them by original owner.
- The offline fixture provider enforces the same credits in its process service
  loop. Its synchronous fixture-model `send_one` method remains a unit-test seam,
  not a second production transport.

Each received packet increases the receipt count by one and adds at most 960
samples. The calculated grant therefore cannot decrease. Playback and
cancellation only free space. The last 240-sample holdback cannot deadlock a
completion packet because it leaves ample capacity for metadata. The old
overflow check remains an invariant guard against protocol violations.

The logical coordinator PCM bound is 1,440,000 bytes. Allocator rounding and the
separate worker, transport, speaker and Gemini buffers are additional; this is
not a total-process memory measurement. Credit replenishment uses constant-size
state and nonblocking I/O. Its service target remains the existing 2 ms loop;
no acoustic latency improvement follows from host tests alone.

## Reuse and integration

V1 main `d5efe9d7b73cc529b34cd4abe97624682a82ca94` already has bounded TTS frame
queues and cancellation-aware producers (`hal/drivers/voice/tts/service.py`,
`_PendingItem`, `_pre_synthesize`, and the head/tail producer loops). Preserve
that useful producer/consumer principle. Rust uses explicit process credits
rather than blocking the conversation thread or relaxing message freshness.
No Python runtime or source dependency is introduced.

This work integrates Agent 1's Gemini reliability commit `fbf9ecc3c` without
changing `crates/gemini/**`. The shared changes in `provider_worker.rs` are the
capacity sender, shared packet-size constant, retired-output pruning, and
integration tests. Existing scripted worker tests now return credits on receipt.
Agent 1 should preserve these when updating the provider.

## Verification and remaining qualification

The focused tests include a 90-second PCM stream plus a partial final packet
over real private IPC with a **virtual** 10 ms playback cadence. They check exact
sample ordering, bounded backlog and final padding. Separate real-time tests
withhold credits for longer than the IPC age limit and check that unsent output
can subsequently be sent fresh; a packet already sitting unread still expires.
The scripted provider test checks that new input and Stop remain serviceable
with exhausted output credits. None opens a physical audio device.

Earlier checkpoint source manifest: `670a905dfb3b01bd5724b98e8a8f38fc4787f46dc81f895ad6d6358ff6b411af`
(160 Rust, Cargo and toolchain files; documentation excluded from this identity).
On the macOS x86-64 host, the source-only export passed **696 tests, zero failed
or ignored**, formatting, strict workspace/all-target Clippy, and both release
builds:

```sh
cargo fmt --all -- --check
cargo clippy --locked --offline --workspace --all-targets -- -D warnings
cargo test --locked --offline --workspace --no-fail-fast -- --test-threads=1
cargo build --locked --offline --release -p lamp-voice -p lamp-voice-eval
```

This includes Agent 1 `fbf9ecc3c`, Agent 2 through `447fa188b`, output credits,
the startup fix, and the subsequent [answer accounting](voice-eval-answer-review.md)
and [recording verification](voice-eval-recording-evidence.md) fixes. The
reporting regressions reproduce wrong completion credit on the old evaluator;
synthetic WAV fixtures prove verification behavior, not physical speech quality.

Validation logs, failed earlier runs, exact commands, and the source manifest
are retained under `artifacts/provider-flow-20261011/`. The earlier 623-test run
preceded Agent 2 integration; the subsequent 665-test run preceded the evaluator
fixes. They are retained separately and are not substituted for that checkpoint run.

The first full run exposed an outdated fixture test client that did not return
credits; it now speaks the new protocol. A subsequent run exposed the
[private-socket startup race](worker-startup.md), fixed separately without
weakening endpoint permissions. Both failed runs are retained in the evidence.

The earlier `fbf9ecc3c` actor had a two-second no-consumption watchdog. One
maximum 48,000-sample event becomes 50 packets; at playback speed, draining
the wrapper from 433 packets below its 384-packet high watermark already takes
two seconds. The later [provider integration](provider-recovery-integration.md)
replaces that bound with 30 seconds and incorporates real-time playback-paced
60–180-second regressions with deliberate holds. The separate transport buffer
holds at most 300 seconds of audio before socket reads pause. These tests use
the actual worker and capacity receiver with modeled coordinator playback;
they do not exercise the full coordinator, hardware speaker or room audio.

The original `670a905d` qualification did not include Linux/ARM64 compilation.
The later [combined integration](provider-recovery-integration.md) records its
own ARM build and emulation gates. Cloud behavior, room-audio latency, genuine
overlapping human speech and natural desk interaction remain unverified by
these software gates.
The owner has deferred physical work until Lamp access is restored.

A subsequent integration of Agent 2 through `3eaa0365a` is recorded in
[voice evaluation integration](voice-eval-integration-20261011.md). Its source
identity and gates supersede this 696-test checkpoint without changing the
provider flow-control protocol.

## Admitted-forget fence over priority control (device validation pending)

The downstream forget extension uses control independently of credited output:
`WorkerEvent::ProviderSpeechCut` carries the current request, inclusive old
provider `Audio.sequence` watermark and monotonic admission time. A saturated
PCM queue must not prevent this fence from reaching the coordinator. These are
sequence watermarks, not packet-credit counters or new interaction authority.
The coordinator validates the current request, removes old reply PCM, and
continues counting/discarding late old packets so capacity accounting remains
cumulative across the cut. Cancelling unsent output consumes no packet credit.

`Control::CutSpeech` names the existing turn owner and the inclusive old
speaker-chunk watermark, a different sequence space from provider audio. Fresh
speaker chunks wait for matching `WorkerEvent::SpeechCutApplied`, emitted only
after the speaker's checked physical stop/reset. Only then does
`Control::ProviderSpeechCutApplied` release the provider continuation. The local
acknowledgment budget is 250 ms; failure/timeout cannot authorize continuation
through an unconfirmed cut. No extra cloud round or new playback permission is
introduced. Accepted-write receipts stay true; discard does not report old
speech as drained or retired. Duplicate and stale-owner controls must not stop
fresh audio. Existing authority renewal and capture remain independent.

These private variants require coordinator and workers from the same voice
release. Supported updates install immutable hash-named release directories;
Linux `current_exe()` resolves the parent's running release for its children.
This is not a negotiated cross-version protocol or executable-descriptor pin.
No independently updated worker or manual executable replacement is supported.

The provider-only baseline was 30,040 ms emitted/unplayed under host saturation
and 130 ms in a draining control, with simulated speaker retirement. Mac
formatting and strict `lamp-voice`/`lamp-audio` all-target clippy pass. Focused
Mac tests pass 30 cases: speech_cut 12, forget_tail 6, forget_speech 9 and
music_scope 3. The forget_tail filter covers baseline measurements and
coordinator cut controls. Matched host simulations at `38654f37c` reduce old audio across five
buckets from 30,040 ms saturated and 130 ms draining to zero. Actual Engine
admission to modelled drop/ACK is 1,788/1,946 us saturated and 1,656/1,786 us
draining. Speaker cut/ACK is scripted simulation (`acoustic=false`), not a
hardware measurement or worst-case bound. The workspace build is still running;
no pass is claimed. Linux speaker-helper and actual speaker-loop tests have not run;
acoustic acceptance remains pending. See [second voice](second-voice.md) for
exact reproduction commands and the remaining device plan.
A reset acknowledgment measures a local control boundary, not last audible
speech. The device goal is at most one second from admitted forget to old words
stopping; a same-request scene without actual old pending speech is
NOT EXERCISED. No new performance or acoustic result is claimed here.
