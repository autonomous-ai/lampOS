# Automated voice acceptance runner (`lamp-voice-eval`)

Status 2026-10-11 (voice-evaluation workstream, Agent 2): the runner, evaluator,
reports, evaluator self-test, fake runtime and trace import are implemented and
tested offline. The physical runner is implemented and exercised end to end
against a fake `lamp-voice` over its real relay and cue socket, but **has not run
on a Lamp or played any audio**. There are no physical attempts yet, so there is
**no positive V2 overlap/interruption cohort**; none may be derived from the fake
runtime. Ranked, reproducible defects are in [voice defects](voice-eval-defects.md).

A long-lived [serve-session runner](voice-eval-serve.md) carries one
conversation of up to 20 context-linked turns in a single `lamp-voice serve`
runtime. Each stimulus is armed before readiness and started by one original
expiring grant. It is also tested offline only.

`crates/voice-eval` turns the [comparison protocol](comparison-protocol.md) into
a repeatable runner: pre-registered scenarios, stimuli started from actual
runtime/playback events with bounded deadlines, an append-only attempt ledger,
and reports that keep software, simulated and acoustic evidence apart. It reuses
`lamp-sound-fixtures` (catalog, verified cache, rendering), `lamp-observer` (iMac
playback; room recording through the signed bundle), `lamp-voice` (`TurnDetector`,
VAD, `CueSink`, trace vocabulary) and `lamp-ipc` (clock).

## One command per suite

```sh
cargo run -p lamp-voice-eval -- fake-run --out NEW_DIR          # offline, every scenario
cargo run -p lamp-voice-eval -- physical-run --config FILE --cache .cache/acoustic --manifest M... --out NEW_DIR [--execute]
```

Both first run the evaluator self-test, then every selected attempt, and write
`NEW_DIR/evaluations/<unix-ms>/report.md`, `evaluation.json` and one evidence
page per attempt in `attempts/`. Exit code 2 means the self-test failed and the
report is marked UNTRUSTED; a physical run refuses to start in that case.
Exit code 0 means only that the run and its evaluation were written. It never
certifies an attempt, room audio or acoustic acceptance. The final stdout line
gives the attempt outcomes, `room_recordings` (`verified`, `invalid`, `absent`
from the recorder's terminal report) and `acoustic_measurements`.

`evaluate RUN_DIR` re-scores a retained run with the current evaluator into a
new `evaluations/<unix-ms>/` directory. Earlier evaluations, including failed
originals, are never rewritten. Pass the run's own plan; a serve run needs
`--plan fixtures/voice-eval-serve-v1.json`.

## Evidence strata and stimulus sources

Every attempt has one stratum and one stimulus source; reports group by both and
never pool groups.

| Stratum | What produced it | What it can show |
|---|---|---|
| `fake_timing` | Fake runtime, declared speech intervals | Turn-policy consequences: false/missed interruptions, splits, unwanted responses, provider-fault handling, voice/ring state. Not level- or acoustics-sensitive. |
| `fake_signal` | Fake runtime, lamp-voice VAD on the exact cached digital mixes | The same, plus what the VAD/detector does with the real stimulus waveform. No room, loudspeaker, microphone or AEC; host VAD numerics can differ from native ARM64. |
| `physical_fixture` | `lamp-voice directed-fixture` on the Lamp | Real capture/echo path with the cached Gemini reply. Only the first turn has a reply (with the diagnostic `--fixture-at-ms N`, only its own turn N ms after `listening_ready`; see microphone-experiments.md). |
| `physical_gemini` | `lamp-voice directed` | Conversation with event-triggered follow-ups and interruptions through the optional cue socket. |
| `imported_trace` | An existing lamp-voice `events.jsonl` | Retained trials scored with declared turn attribution. |

| Source | Meaning |
|---|---|
| `declared_timing`, `digital_mix` | Fake runtime; nothing audible. |
| `loudspeaker_synthetic` | Cached synthetic voices on the explicit iMac speakers. |
| `direct_human` | A person speaks lines prompted silently on the operator console at the trigger. Only the prompt time is known in software. |

Jieli's onboard processing may treat loudspeaker replay and a person
differently, so loudspeaker results never stand in for direct-human ones.
Software timestamps (runtime host clock, ALSA acceptance/retirement, runner
start requests) are never acoustic boundaries. Several synthetic voices from one
loudspeaker cannot establish spatial speaker discrimination.

## Scenario plan and stimuli

`fixtures/voice-eval-v2.json` is the current pre-registered plan: 42 scenarios with
expected behavior per step, triggers, deadlines, fake replies/faults and three
fake profiles. It changes ordinary follow-ups to the whole-turn completion
trigger; the original v1 plan and its hashes remain unchanged and readable.
`fixtures/voice-eval-stimuli-v1.json` extends `desk-v1` with
twelve utterances and twenty scenes. `desk-v1.json` is unchanged (SHA-256
`636a5199…`); extension utterances that reuse a desk ID must be identical and
scene IDs may not collide. Eight of the new scenes only remix cached speech (for
example `quiet-question` is `math` at -24 dB, `interrupt-sky` reuses `mid-name`);
twelve use the twelve new utterances, which need speech rendering once, on the Mac
that holds the existing cache (`render-stimuli`, which reuses every verified
object).

| Category | Scenarios |
|---|---|
| Quick questions, rapid follow-ups | `quick-chat`, `quick-fact`, `follow-up-chain` (two follow-ups, each 0.5–0.6 s after the previous answer ends), `rapid-follow-up` (150 ms after the answer ends) |
| Short answers to Lamp's questions | `short-yes`, `short-no` |
| Long answers, thoughtful pauses, unfinished questions | `long-answer`, `hesitant-sharing`, `unfinished-question-pause` (1.4 s pause mid-question), `unfinished-question-resume` (finished 300 ms into Lamp's reply to the fragment) |
| Natural topic changes, repeated interruptions | `topic-change` (+1.2 s), `topic-change-late` (+4 s), `repeated-interruptions` (three in a row), `fixed-reply-topic-change` |
| Correction, quiet and rapid speech | `self-correction`, `quiet-question`, `rapid-question` (240 wpm) |
| Listener acknowledgments | `listener-acknowledgment`, `ack-yeah`, `ack-right`, `fixed-reply-acknowledgment` |
| Background conversation, two speakers, other devices, noise | `background-conversation-question`, `two-colleagues`, `ambiguous-not-invited`, `overlapping-speakers`, `colleague-aside`, `other-device`, `computer-call`, `background-media`, `noise-only`, `noisy-question`, `typing-question` |
| Echo-only playback | `fixed-reply-echo-only` (the h4jds57i condition), `long-answer` |
| Delays, disconnects and recovery (fake only) | `provider-slow-first-audio`, `provider-late-answer`, `provider-supply-gap`, `slow-response-then-follow-up`, `provider-failure-before-audio`, `provider-disconnect-mid-reply`, `disconnect-between-turns`, `disconnect-mid-reply-recovery`, `provider-spurious-interrupt` |

`fixtures/voice-eval-late-cut-in-v1.json` plays the 20 s forgiveness of a
fruitless yield over a long story (`turn_taking.rs`). It is a plan of its own,
so the v2 plan's bytes and hash stay as registered. Both scenarios ask for a story of
about a minute; 3 s into it two asides ("What was that?", then "Oh, really?"
4 s later) each yield and resume, which spends both fruitless yields. In
`late-cut-in-after-fruitless-yields` a request ("Hey Lamp, what's seven times
eight?") comes 25 s after the second aside, past the 20 s, and must cut in and
be answered. In `late-line-too-soon-after-fruitless-yields` it comes 10 s after,
while both still count, and the story must go on. The evaluator reads no
`duplex_yield` or `duplex_resume`, so that second pass is shown only with the
two yields and resumes in the session's own events; an aside admitted as a turn
fails either. Run it with `--plan fixtures/voice-eval-late-cut-in-v1.json`.

Expectations: `answer` (one turn, complete within the 15 s deadline),
`interrupt_and_answer`, `no_interrupt`, `silence`, `observe` (no admission during
Lamp's own playback), `honest_failure` (explicit failure, no fabricated
completion, spoken notice), `recover_and_answer` (the session survives an
injected provider failure and answers) and `fragment` (an unfinished utterance,
judged through the step that continues it). A step in a multi-talker scene can
list its `addressed` utterances; admissions whose captured input never overlaps
them are background responses. Validation rejects unknown scenes or addressed
utterances, unbounded deadlines, overlap expectations without the speaking
precondition, plan triggers that disagree with a scene's declared trigger, and
fault injection claimed as physical.

## Event triggering and retention

The first step follows `listening_ready`; later steps follow
`speaker_first_write`, `turn_completed` or `turn_cancelled` (historical plans may
still use occurrence-level `speech_retired`) of a turn newer than
every turn known when the previous step started, then wait the declared delay.
Waiting is bounded by the step deadline. A missing event gives `trigger_missed`;
an ended session `session_ended`; a planned start that passed more than 100 ms
before the trigger could act `trigger_stale`; an overlap step whose reply stopped
before injection `precondition_lost`. Nothing is played in those cases and later
steps are withheld. A player reporting failed delivery marks the step
`delivery_failed`. There is no fixed-delay fallback. Stimulus timing (which
reads and verifies cached audio) is computed before the session starts.

The ledger (`RUN/ledger.jsonl`, mode 0600, append-only, sequence-checked) holds
`run_start` (plan/catalog hashes, backend, seed, attempt order, evaluator
self-test), `attempt_planned` (synced before any session or stimulus),
`attempt_finished` (step receipts, live events, authoritative events, clock map,
evidence) and `run_end`. Withheld and unsupported attempts are retained with
their reason. Evaluation reports planned-but-unfinished attempts and a truncated
final line. Scoring can be repeated on an old ledger with `evaluate`.

A physical attempt is **invalid** (listed, but excluded from every rate and
latency) when its session transport closes before `session_end`, the relay
reports an error or hard-deadline kill, a relayed line fails framing, or fewer
trace lines arrive than the relay sent. A trace must contain both `run_start`
and `run_end` to be scored.

### Next-turn readiness (`next_turn_ready`)

Whole-turn completion is not listening readiness. Under V1-baseline, input
stays unavailable for the playback tail after the final sample retires (Main's
native run measured 100.865 ms), and the availability update can arrive after
the completion. A `turn_completed` trigger cannot see that; with a short
delay the next question starts while input is still withheld.

A step with `"after": "next_turn_ready"` therefore waits by event. It is ready
at the first event after which all of these hold:

- every turn admitted since the previous step's dispatch (newer than every
  turn then known) has finished, completed or cancelled, and at least one did;
- the latest reported input availability is true;
- the latest reported provider link is `ready`.

Each condition is the latest event-reported state at that point. A value cached
when the turn finished is never used: an unavailable state waits for its
update, and an available state reported once (`vad-only` never withdraws it)
holds. The delay counts from readiness.

| Situation | Result |
| --- | --- |
| Any input candidate or admission after those turns finished | `precondition_lost` ("unplanned input during the next-turn wait"); a whole extra turn is not waited out |
| Input, lost availability or a non-ready link between readiness and the planned start (final dispatch check) | `precondition_lost` |
| Provider link reported `terminal` | `provider_unavailable`, latched for the attempt |
| Provider link `unavailable` | Waits for `ready` by event, within the step deadline, else `trigger_missed` |
| A turn admitted or finished twice, or resumed after it finished | `session_ended` ("runtime owner evidence inconsistent"); on the live cue path `CueOrder` already rejects these as a protocol failure |
| The attempt's events carry no availability or link at all | `trigger_unavailable` at once; nothing waits on evidence the runtime does not send |

Rules:

- `next_turn_ready` may follow only a step that expects an admitted turn.
- Its scene must declare a "Lamp speech ended" trigger.
- Withheld steps stay in the ledger; nothing is replayed.

**Sources.**

- Traces map `turn_taking_availability` (`prospective_input_available`) to
  `input_availability`, and `provider_ready`, `provider_unavailable` and
  `provider_terminal` to `provider_link`.
- The fake runtime reports both before `listening_ready`, as lamp-voice does.
- Live physical sessions need the cue kinds proposed in
  [the cue contract](voice-eval-directed-cues.md#proposed-readiness-cues).
  Without them a physical `next_turn_ready` step is `trigger_unavailable`.
  With the proposed lamp-voice cue sink, `tests/physical_loopback.rs` runs a
  physical follow-up through the real relay on those cues.
- Existing plans keep their `turn_completed` triggers and hashes.

**Tests.**

- `runner::occurrence_tests`:
  - the native e9/b1 timeline (completion, then availability 100.865 ms
    later);
  - availability never withdrawn;
  - a cancelled finish;
  - an extra candidate or extra turn in the wait;
  - input or lost availability before dispatch;
  - terminal and recovering links;
  - a duplicate finish;
  - missing evidence;
  - an audit case showing a zero-delay `turn_completed` trigger starts inside
    the reopen window.
- `events::tests` covers trace and cue parsing; `cue_tests` covers sequencing.
- `tests/next_turn_ready.rs` covers plan rules and the fake runtime end to end.

### Final dispatch check (`MAIN-023`, `MAIN-025`, `MAIN-030`)

**Runner.** At the planned start, the runner takes every event already
delivered (at most 256) and then rechecks every condition the stimulus needs:
- the session is open;
- the provider is available;
- the start is within the 100 ms lateness bound (`trigger_stale`);
- for overlap steps, Lamp is still playing the triggering reply
  (`precondition_lost`).

**Backend.** Immediately before a stimulus starts, the physical backend takes
every relay line already delivered and keeps those events for the runner.
- If any taken event has not yet been observed by the runner, nothing starts.
  The backend returns `revalidate`, and the runner observes those events,
  rechecks and dispatches again.
- The backend's drain is bounded at 1024 relay lines. When the budget runs
  out, it also returns `revalidate` (receipt `drain_exhausted`).
- After 8 such rounds, or a drain that does not settle, the step is withheld
  as `trigger_stale` with the reason.
- Immediately before the player starts, the original target's 100 ms
  lateness bound is checked again on the local clock, even when no new event
  surfaced. A missed bound returns `trigger_stale`.
- The relay streams the runtime's final trace only after the runtime has
  exited or been killed. The first relayed trace record therefore latches
  `session_ended`, while the trace is still kept as evidence.
- The first terminal cause seen on the attempt is latched, and no stimulus
  starts after it. This includes a cause first seen in the backend's final
  drain.

| Cause | Latched from | Step status | Receipt `withheld_cause` |
|---|---|---|---|
| Provider failure | `turn_completed` with `provider_failed` | `provider_unavailable` | `provider_failed` |
| Session ended | relayed `session_end`, a `run_end` cue, or the transport closing without `session_end` | `session_ended` | `session_ended` |
| Protocol failure | an invalid or rejected cue, an unreadable stdout line, or any relay framing error | `session_ended` | `protocol_failed` |

The step's `detail` and the receipt's `reason` name the exact cause, for
example the relay's exit status or the rejected cue. A relay framing error
also ends live scheduling as a protocol stop, because the lost line may have
been a cue or the session's end.

**Tests.**
- `tests/final_dispatch.rs` uses the real backend, a local pipe transport and
  a recording-only player.
- Root's review regressions are `tests/audit_final_dispatch.rs` and
  `tests/audit_runner_dispatch.rs`.
- The runner's scripted cases are in `runner::occurrence_tests`.

### Terminal provider failure (`MAIN-009`, provisional field names)

A `turn_completed` cue or `turn_finished` trace record with outcome
`provider_failed` ends the turn as a failed answer.

- **Scoring.** It is scored `ProviderFailed { partial_audio }` with a
  `provider_failed` finding. It counts in the completion denominator and never
  in its numerator, and it is reported separately from silent missing
  terminals.
- **Audio evidence.** Audio already played stays as evidence. Every started
  playback occurrence must have retired first, otherwise the completion
  evidence is inconsistent.
- **Endpoint.** No normal endpoint is required: an abandoned request may end on
  the drain path, recorded as `input_discarded_endpoint`. That event is never
  treated as a local endpoint and starts no answer latency.
- **Runner.** A failed terminal never satisfies a completion trigger. A step
  waiting on it is recorded as `provider_unavailable`, and nothing is played.
- **Schema.** Field names follow the main agent's description and will be
  aligned with its frozen producer schema. Tests: `tests/provider_failed.rs` and
  the runner's `a_provider_failure_is_never_a_completion_trigger_and_is_reported`.

### Relay framing and cleanup

Both relay ends read lines incrementally with a fixed per-line bound and never
truncate: a line is kept only up to its bound, and an oversized line is consumed
through its newline without being retained. Oversized, non-UTF-8 and cut-off
lines are explicit failures. Bounds (`physical/protocol.rs`):

| Stream | Bound | Failure |
|---|---|---|
| Relay to runner, one encoded line | 64 KiB | protocol error; attempt invalid |
| Relay to runner, one session | 96 MiB | reading stops; transport treated as closed |
| Runtime stdout, one line | 16 KiB (and 64 KiB after JSON encoding) | `stdout_invalid` line and `session_end.error` |
| `events.jsonl`, whole file | 64 MiB, checked before reading | `session_end.error`, nothing streamed |
| `events.jsonl`, one record | 63 KiB | streaming stops at that record; `session_end.error` |
| Runner to relay, one line | 1 KiB | `session_end.error` |

`events_sha256` always covers the whole trace file, including bytes after a
failure, and a file that changes length while relaying is reported. A final
record without a newline is reported as cut off, not parsed.

The relay starts `lamp-voice` in its own process group. A hard-deadline stop, an
early relay error and a closed runner transport all kill that group, so workers
stop with the coordinator. If the runner transport closes before the runtime
ends, the relay stops the runtime and reports it rather than leaving the
microphone and speaker in an unattended session. After a normal exit, any process
still in the group is killed and reported as `stray_processes_stopped` (evidence,
not an invalid trace). The runner kills its transport, room recorder and players
if it fails between session start and finish. Tests: `tests/relay_hardening.rs`,
`physical::framing` and `physical::mac::relay_reader_tests`.

## Evaluation rules

Admissions are attributed to the stimulus whose speech window contains them,
after mapping step times into the event clock (identity for the fake runtime;
ping/pong over the session transport for physical runs, widened by its
uncertainty and a playback-path allowance of 250 ms for loudspeaker replay).
A scene without speech owns its whole duration. Direct-human prompts have no
verified speech window: timed direct-human attempts remain unscored, with raw
records retained, until speech and turn attribution can be reviewed. A prompt
being displayed does not establish speech delivery, silence, or an interruption.
See [acceptance boundaries](voice-eval-acceptance-boundaries.md).
Imported traces use declared `step=turn` pairs; every stimulus step must be
declared (`--turn STEP=none` when it produced no admission), except that a
single-stimulus scenario defaults to the first admission.

### Imported recordings (`MAIN-021`)

An imported trial stays an `imported_trace` attempt, separate from any
runner-controlled physical cohort. Failed, partial and invalid attempts are
kept. The room recording comes from one of two sources:

- **Observer metadata** (`--room-metadata`): the observer's `metadata.json`
  and its sibling `room.wav`.
- **Plain WAV with the source recorder's receipt** (`--room-receipt FILE
  --room-input NAME`, for example `voice` or `ambient`).
  - The receipt must be a completed, valid capture with privacy never revoked.
  - The input must be accepted by the recorder and stopped through its owned,
    graceful stop.
  - The format must be mono 16 kHz `S16_LE`.
  - The frame counts and PCM digests in the receipt must agree with each
    other.
  - The WAV, named as a plain sibling file, must match the receipt's file
    hash, its frame count and the digest of the PCM the recorder received.
    The WAV's `data` chunk is hashed separately to check that last point.

**Retention.** Only bounded, nonsecret receipt fields are kept: recorder
stderr is never copied. Recorder times stay in the recorder's own clock domain
and are not ADC or acoustic times.

**Review time.** The receipt must be byte-identical to the imported one, and
the selected input is parsed and validated again (`MAIN-034`). The WAV path,
its digests and its frame count are re-derived from the receipt, never taken
from the cached ledger evidence; cached values that disagree are refused.
The WAV is then re-read and checked against the receipt's claims.

**Bounds.** Frame counts are bounded before any arithmetic: at most 600 s for
a receipt, and at most the stream byte bound for a diagnostics marker. An
absurd count is invalid evidence, never a panic.

**Invalid recordings.** An invalid or missing recording is retained as
invalid evidence, and that attempt's acoustic boundaries stay unmeasured.
Observer evidence keeps a bounded copy of the observer's terminal report
(`observer_terminal`). It holds status, stop reason, fault bits named by the
observer's own definitions, and requested/written/dropped frames at the
delivered rate. It also holds writer sequence gaps, stream errors and xruns.
An invalid recording is reported in three places:
- a withheld `evidence_missing` finding that quotes this report;
- an attempt-level `room_recording` state;
- the attempt page and the stratum counts.

Its acoustic boundaries are unscored as `room recording invalid (<status>)`, and
`annotation-template` omits it so it is never offered for listening. A
recorder's or wrapper's exit status (for example `open -W`) never validates a
recording.

**Target diagnostics.** The target's certified diagnostics beside the trace
(`render-audio/` and `capture-audio/`) are verified against the trace:
- lamp-voice's own certifier checks each stream's identity and finish receipt;
- each PCM and record file must match the completion marker's hash and the
  certified frame counts.

The result is recorded as `target_audio`, verified or unverified with a
reason, and nothing is adapted from other metadata. Pre-AEC capture is
ALSA-resampled 16 kHz, not capsule PCM. Accepted render is 24 kHz audio that
ALSA accepted, which is not proof of audibility.

Tests: `tests/recording_import.rs`, which uses root's real receipt examples
with digests adapted to generated WAVs, and `tests/target_audio.rs`, which
uses directories written by lamp-voice's `Recorder`.

A `user_interrupted` cancellation is caused by a later turn's admission stamped
within 20 ms of it. lamp-voice revokes the old reply and then admits its
successor in one step, so the admission follows the revoke by microseconds
(older traces stamped it just before). The speech candidate is not the causal
time: in duplex it precedes the revoke by the whole yield (470 ms on Lamp-4ace).
Where the trace records owners, the cause must have the same boot and a newer
authority generation. An orphan cancellation, one whose nearest admission is
outside 20 ms, or one explained only by an earlier turn, another boot or an
older generation stays a runtime failure that names the nearest admission.

A planned `interrupt_and_answer` step yields as planned. A `local_endpoint`
correction bound to the turn it follows supersedes that turn's reply as planned.
If the reply had already started, the correction is still a planned yield, but a
withheld `precondition_lost` finding records that supersession before output was
not tested. The correction's own `answer` expectation never relabels it.
Anything else, including background talk or a correction bound to another turn,
is a **false interruption**. A provider interruption without planned speech is
also false. One turn may
have several playback occurrences, identified by positive `playback_sequence`.
Retiring one occurrence does not complete the turn: only a valid whole-turn
completion after all occurrences can earn transport completion. Cancellation
during a later occurrence or in the gap between occurrences stays truncated
unless it was an intended yield. Overlap steps outside an actual occurrence
(including the gap between generations) are unscored
(`overlap_not_achieved`), and a start more than 250 ms late is flagged
`late_stimulus`. An injected fault pre-empted by another cancellation is
unscored, not failed.

Answers are complete, complete with playback gaps, yielded as planned, late
(right-censored), **truncated** (audio started, not finished), **missing** (no
audio for an admitted request, or never admitted), or unsupported (fixture
second turns). One request answered aloud twice is a **duplicate answer**.
Other findings: missed interruption, unwanted response, turn split, lost opening
words (exact in the fake runtime; physical needs proposal P5), possible lost
opening words (provider transcript hypothesis only), runtime failure, no
recovery, unannounced failure and fabricated completion.

**Voice and ring state through cancellation** is checked on every trace: no
speaker output for a turn after it was revoked, never two replies playing at
once, every admitted turn terminated in a completed run, and, when the runtime
records ring requests (`--ring-channel-ceiling`), no cue for an ended turn and
each listening/waiting/speaking cue consistent with that turn's voice state
within 50 ms, across every playback occurrence. Exposure sums each software
playback interval through retirement/cancellation and excludes the gaps between
generations; it is not audible duration. The first-write latency remains the
first occurrence's start. Stale/mismatched/duplicate retirement and ambiguous
legacy multi-occurrence records invalidate evidence rather than mint completion.
A legacy single occurrence with explicit valid terminal evidence remains
readable. Attempts without ring requests are counted as ring-unchecked.
See [directed cue lifecycle](voice-eval-directed-cues.md).

Latency rows are labeled `simulated` (fake profile; restates configured delays),
`software` (lamp-voice host clock), `runner` (start-request lateness) or
`ACOUSTIC` (annotations). The endpoint boundary states the silence wait from the
runtime's own `local_endpoint` records (`includes_silence_wait_ms`), for example
1,200 ms on every Lamp-4ace endpoint, with counts when values differ or are not
reported. No profile default is substituted. Release targets apply only to ACOUSTIC rows.
Percentiles use nearest rank with the exact n. Rates show numerators and
denominators; answers yielded to a planned interruption and answers the
provider cannot give (fixture second turns) are listed outside the
complete-answer denominator. Completion credit now also excludes confirmed
answer-content and duplicate/split failures while retaining those opportunities.
Reports separate raw completed playback, positively reviewed completions and
unreviewed completions; see [answer review accounting](voice-eval-answer-review.md).
An unreviewed completed playback does not establish semantic success.
Both acoustic boundaries, final
user speech to first substantive audible word and interrupting speech to audible
silence, always appear with measured and unmeasured counts and the reasons.

Annotations (`annotation-template` creates the file) must name the room WAV
SHA-256, set `listened: true` after a person listened, and give both boundaries
in WAV seconds. Hints from runner times locate stimuli but are never scored. A
missing, unlistened, mismatched, one-sided or negative annotation stays
unscored. Reviewer judgments in a usable annotation count: `answer_complete:
false` and `answer_relevant: false` fail the step, and `spoken_failure_notice`
scores physical honest-failure steps. The actual WAV is now opened, structurally
checked and hashed at import/collection and again at scoring. Run identity,
finite in-recording boundaries and a successful supervised recorder exit are
required where applicable. Missing or changed files remain visible and unscored.
See [recording evidence](voice-eval-recording-evidence.md) for bounds, supported
formats, old-ledger migration and the limits of these checks.

## Evaluator self-test

The self-test uses its own embedded canonical plan and catalog, independent of
the run being evaluated. Results retain their versions and content hashes;
[canonical canary identity](voice-eval-canary-plan.md) describes re-evaluation
of historical or custom plans.

`self-test` runs 21 canaries before any report is trusted. Three clean controls
must pass (a conversation, kept silence, a planned interruption). Eighteen
injected failures must each be detected and fail the attempt, injected either
through the fake runtime's own faults (echo leak, acknowledgment, unaddressed
talk, split, supply gap, late answer, silent failure, no recovery) or by
mutating a clean attempt's events (removed interruption, missing, truncated or
duplicate answer, dropped opening words, stale output after revocation, a ring
cue ahead of the voice, overlapping replies, a partial trace, and a physical
attempt with room audio but no annotation, which must stay unmeasured).

## Other commands

From `lampOS/`:

```sh
cargo run -p lamp-voice-eval -- validate | list | self-test
cargo run -p lamp-voice-eval -- fake-run --out NEW_DIR [--profile v2-directed-current|v2-echo-leak-h4|v2-echo-loop] [--scenario ID]... [--repetitions N] [--seed N] [--attempt-seed N]
cargo run -p lamp-voice-eval -- fake-run --out NEW_DIR --cache .cache/acoustic --manifest .cache/render-first.json   # fake_signal
cargo run -p lamp-voice-eval -- evaluate RUN_DIR [--annotations FILE]
cargo run -p lamp-voice-eval -- import-trace --events artifacts/.../events.jsonl --scenario fixed-reply-echo-only --out NEW_DIR [--turn greet=1] [--source loudspeaker_synthetic|direct_human] [--room-metadata room/metadata.json]
cargo run -p lamp-voice-eval -- import-trace --events target/runtime/events.jsonl --scenario quick-chat --out NEW_DIR --source loudspeaker_synthetic --room-receipt acoustic/lamp-52e6-capture-receipt.json --room-input voice --room-independent
cargo run -p lamp-voice-eval -- annotation-template RUN_DIR NEW_FILE.json
cargo run -p lamp-voice-eval -- assets --cache .cache/acoustic --manifest MANIFEST...
cargo run -p lamp-voice-eval -- render-stimuli .cache/acoustic NEW_REPORT.json   # macOS; renders only missing objects
```

Every failed attempt carries a reproduction command; fake attempts reproduce
exactly with `--attempt-seed`.

## Waiting answers held and cut, per route: `waiting-cuts` (#1866)

    journalctl -u lamp-voice --since '<test start>' -o short-iso \
      | sudo lamp-voice-eval waiting-cuts [--since ...] [--until ...] [--sessions DIR] [--json]

Counts, per run and per route, the waiting answers a new sound waited for,
let go, stopped or was cut by (with words or without), short follow-ups
kept, stops, and their times. It reads the journal on stdin and the session
records (`--sessions`, default `/var/lib/lampOS/voice/sessions`): the
journal alone does not carry the wait's decisions, the words or the route.
`--since`/`--until` filter the journal's own stamps. Only counts and
numbers are printed. What each count means: `group-talk.md`, "Counting it".

## A session's minutes, with no words: `export` (#1119)

Builders without the lamp make test cases from what it did. `export` takes a
window of a lamp's session and keeps only what holds no words:

    lamp-voice-eval export --since 17:13:00 --until 17:16:00 --out NEW_DIR \
        --sessions /var/lib/lampOS/voice/sessions --journal journal.json

with the journal as `journalctl -o json --since 17:10 --until 17:20 >
journal.json`. Times are the lamp's local wall time (`--zone system` unless
given) on `--date YYYY-MM-DD` (today's unless given). It runs on the lamp, or
on a copy of its sessions and journal.

- **The voice's trace** (`events-*.jsonl` under `--sessions`, any depth):
  each event keeps its `kind` and `at_us`, its top-level numbers and
  booleans, and the strings of a few fields the voice's code fills from a
  closed set (`outcome`, `reason`, `phase`, `link`, `admission_basis`, ...)
  when the value is one lower-case word of letters, digits and `_`.
  Nested objects, arrays, every other string, and any event whose kind is
  not such a word are left out and counted. A `transcript` keeps who (`heard`
  when it has no turn, `lamp` when it has one), its length in characters,
  its turn and whether it was final: never its text.
- **The parts' journal** (`--journal`): only lines whose shape is known are
  read, into numbers and plain words: the Policy's `decision`, `progress
  decision`, `hello decision` and `outcome of decision` lines (their
  `key=value` pairs, only for the keys the Policy writes: any other key is
  left out and counted, even with a one-word value), the body's `step N gesture NAME`, its faces (`mood`,
  `across`, `down`, `size`) and its goals, and a skill's `SKILL Stage` (its
  why is left out). Every other line is left out and counted by part: one
  of lampOS's units (`deploy/product/*.service`), else `other`, so no other
  program's name or tag is written.
- **The bus** (`--bus FILE`, repeatable: `lamp-bus record` files, such as
  `lamp-bus record DIR humans/faces/tracked voice/state` makes): only
  `humans/faces/tracked` and `voice/state`. A face keeps its box, confidence,
  yaw and whether it faced the lamp; its passing id becomes the export's own
  (`f1`, `f2`, ...), and every other field is left out and counted. A state
  keeps one of `resting`, `listening`, `thinking`, `speaking`. Every other
  topic is left out and counted. The eyes write no face boxes to the journal,
  so a recording is the only source of faces: start one before the minutes
  you want (`lamp-bus record /tmp/rec humans/faces/tracked voice/state
  --seconds 3600`).
- **Wall time.** The voice's `at_us` is the monotonic clock the journal also
  writes, so the journal's own pairs give the boot's offset; without a
  journal, `--wall-offset-us`, or this machine's clocks now (right only on
  the lamp, in the same boot). `summary.json` says which.
- **One boot.** The monotonic clock starts again at every boot, and the
  sessions folder keeps sessions from earlier boots, whose times repeat.
  A session's folder is named by lamp-voice's systemd `INVOCATION_ID`. A
  session is read when the journal's lamp-voice lines for the offset's boot
  name it (`_SYSTEMD_INVOCATION_ID`), **or** when its last event, mapped to
  the wall, is within a minute of its files' modification time (keep it when
  copying: `tar` does). Either is enough: a lamp's journal is volatile and
  forgets this boot's earlier runs, while an earlier boot's mapped times land
  hours from its files' time. `left-out.json`'s `sessions_skipped` counts the
  others by why (`other_boot` with journal runs to ask, `far_from_file_time`
  without); `summary.json` gives `sessions_used` and `sessions_by`, how many
  each rule admitted (`journal_run`, `file_time`). Each session is read a line
  at a time, twice (its last event first), never held whole in memory, and a
  skipped session's lines are not counted at all. A bus recording needs neither: its
  window is its own wall time.
- **The window first.** An event outside `--since`..`--until` is neither
  kept nor counted, so `left-out.json` speaks of the window only.

It writes a new folder (0700, files 0600): `events.jsonl` (both, in time
order, each with `at_us`, `wall` and `source`), `summary.json` (counts by
kind, and by kind and plain-word value, such as
`turn_finished.outcome=user_interrupted`), `left-out.json` (what was left
out, counted, never what it said) and a skill's `tests/` case in `case/`, each
topic as `lamp-bus record` writes it: `voice_state.jsonl`, and
`humans_faces_tracked.jsonl` when `--bus` held faces. The case's files share
one start, timed by the monotonic `at_us` the voice's trace and the bus both
carry. That assumes each publisher stamps `CLOCK_MONOTONIC`: true of
`voice/state` on lamp-cb94, not of `ears/level` there (its `at_us` was about
3,486 s off), so a topic from another clock would land in the wrong place.
`voice/state` is the recorded one when `--bus` has it, else built from the
trace as `bus_out::state` publishes it (the voice does not write the state
itself): from `ring_requested.phase` while a turn owns the ring (`listening`
listening, `waiting` thinking, `speaking` speaking), and resting at
`turn_finished`. `summary.json` says which. With the ring degraded
(`OptionalRing` stops presenting) the trace has no `ring_requested`, so a
built state stops there, while the bus still published the voice's state.

`stretch_breaks_cases_rebuilt_from_the_export_replay_the_same` exports
Stretch break's own recorded cases through `--bus` and replays `case/` with
`lamp-skills`: it makes the same calls at the same times as the recordings
did (#1119's "done when").

`a_session_full_of_words_exports_none_of_them` fills every text field, a
name, a tool's arguments, an unknown kind and field, a closed-set field
holding a sentence, journal lines (a Policy line's unknown keys among them,
one lower-case word each) and a bus recording (a face's passing id, name and
label, a voice state that is not one, an unknown topic) with distinct words,
and fails if any byte of the output, the case files' payloads decoded, holds
one.

**The trust boundary.** A string is kept only from a field the export names
(the voice's `WORD_FIELDS`, the Policy's `POLICY_KEYS` in `export.rs`), and
only as one lower-case word. Every event's `kind`, and the name of every
top-level field kept for its number or boolean, is copied as well when it is
one such word. All of these are trusted to be filled by code from a closed
set: a kind or a key name is written in the voice's source, never taken from
what was heard; a person's words that become a reason go to `marked/`,
never the trace. A field added there, or one made to echo what a person
said, would be exported: add a field only when its values come from code.
A left-out bus message is counted under its topic only when the topic is one
of lampOS's (`BUS_TOPICS`, held to every `TOPIC` in `crates/msgs` by a test),
else as `other`; anything beside a payload's `faces` is left out and counted.

## What a played test's arms cost: `cost`

`lamp-voice-eval cost JOURNAL --prices FILE --arm NAME FROM TO...` prints what
each arm of a played test cost, from `journalctl -u lamp-voice` alone (the first
builder on #1722, 19 Oct 21:18: `cost-night1.py` as a subcommand). An arm is a
stretch of the journal by the lamp's time of day, `HH:MM` or `HH:MM:SS`, as
`score`'s arms are, and may run past midnight.

- **Usage is read from the voice's own lines**, as counts only, never words
  (`src/cost.rs` names each line):
  - requests, by process and number;
  - answered: the second route's `first answer audio`, and Live's answers
    that began, less those held back. Live's begin with its first sound, which
    it says only with `LAMP_NAME_SIGNAL`, or its answer judged played (`its
    answer is played: …`), which it says whenever attention judges, as with
    `LAMP_SPOKEN_TO`. Either switch counts them; with neither, Live's answered
    reads 0;
  - DeepSeek's prompt, cached and completion tokens from each `chat answer`.
    One with "the cache not said" is priced as all uncached, the dearer, and one
    with "no usage given" is counted, not priced;
  - searches, `web_search` called or the chat's plugin searching;
  - the sound sent to Deepgram, Nova-3's and Flux's lines each at their own price;
  - the voice's characters from each request's summary of its pieces (#1935).
    A piece whose first audio never came isn't in it, so this can read a little low;
  - Live's text and audio tokens in and out from `provider: usage`.
- **Prices are dollars in a JSON file**, with none in the code.
  `fixtures/voice-eval-cost-prices.example.json` holds the list prices read on
  19 Oct, until the team's own rates come. DeepSeek's tiers (peak, off-peak)
  each give the second route's arms a row; Live's arms get one.
- **No price for a voice the journal names refuses the run,** rather than
  pricing that voice at nothing. So does an answered line with no summary of
  its voice's pieces, as in a journal from before #1935, unless
  `--voice-estimate VOICE N` is given. Those lines' characters are then
  estimated as N a completion token, over the arm's share of such lines, as
  `cost-night1.py` does (4), and the row says they're estimated.
- **Each row:** requests, answered, the arm's minutes, and the cost per request,
  per answered line and per "conversation minute". That's 5 requests at the
  arm's own cost a request: the research's model of 4 answered and 1 not.
- **Tests:** `src/cost.rs` (each line, each price, the three forms of a chat
  answer's tokens) and `tests/cost.rs` (the command as it is run).

## The routes' table from the journal: `score`

`lamp-voice-eval score JOURNAL --arm NAME FROM TO [--arm NAME FROM TO]...
[--scenes DIR --date YYYY-MM-DD --utc-offset +HH:MM]` prints one Markdown
table, a row a measure and a column an arm of a played
test (the first builder on #1722, 19 Oct 18:44: tonight's Deepgram,
Deepgram with Flux and the prepared round, and Live). `JOURNAL` is
`journalctl -u lamp-voice` text in any of its usual forms; an arm is the
stretch of it between two times of day (`HH:MM` or `HH:MM:SS`, `TO`
before `FROM` runs past midnight), read by each line's first `HH:MM:SS`.
The journal's lines are placed by their time of day alone, so take the
run's stretch only (`journalctl -u lamp-voice --since … --until …`), and
give the arms by the lamp's clock: a lamp whose zone is UTC
(`timedatectl`) needs its arms in UTC. Only the voice's own lines (`lamp-voice second:` and `lamp-voice
provider:`) are read, and of them only counts, reasons and times: no words
a person said reach the table (`score::tests::no_words_said_reach_the_table`).

- **Counts:** lines judged, let out and by what (its name or words, a
  face, a conversation going on, the lamp's news or work, a trusted
  session, a room not known to be busy), held and why (nobody spoke to the
  lamp, a busy room, a short sound, a word or two, said to someone else),
  answered (the second route's `first answer audio`; Live's answer begun,
  by its `first sound`, said only with `LAMP_NAME_SIGNAL`, or `its answer is
  played`, said whenever attention judges as with `LAMP_SPOKEN_TO`, and not
  held back), declined by the model (`called
  not_for_me`; Live's `the model said it was not for it`, not the one said
  after doing what was asked, which is not taken), unanswered in time (`chat
  round N had not begun`; Live's `; closed as unanswered`, not a wait
  sound's line nor Live's sound, reminder or reply said over, which need no
  answer), chat failures, voice tries that failed to start a sentence
  (each is asked once more, so a try that failed need not be an answer
  lost), lines with no words, sound carried from
  the line before (`of sound go first` alone, not a carry that only lends
  its time), and words cut short answered in a held line's place. The
  answered, declined and unanswered rows count a request once, and a
  request is known by its process (`lamp-voice[PID]`) and number, which
  start again with the service (Jarvis on #1899).
- **A row a route doesn't log prints "—"** in an arm of that route alone:
  Live logs no chat or voice failures, no word counts, no carried sound or
  cut words, and none of the times. An arm whose stretch holds no voice
  lines prints "—" throughout, and a line under the table names it.
- **Times,** as the median, the 90th percentile in brackets, and how many:
  line end to words done, the chat's first event, words to the first piece
  to the voice, words to the first answer audio, and line end to the first
  answer audio (a request's words done plus its first audio). These are the
  second route's lines; Live's own times are in its sessions' events, not
  the journal. Each is the value at its share of the sorted list, rounded
  to the nearest place: of an even count, the median is the upper of the
  middle two.
- **The played test's own files,** with `--scenes DIR --date YYYY-MM-DD
  --utc-offset +HH:MM`: every `.txt` in `DIR` that is a scene file
  (`scenario <name> <ISO>Z …`, one event a line) or a battery summary
  (`battery <round>-<arm>-<HHMM> <HH:MM:SS>`), counted into the arm that
  holds its start on the run's day. `--date` is the day the first arm
  begins, by the lamp's clock; an arm that begins earlier in the day than
  the first is on the day after. `--utc-offset` is the lamp's zone (`+07:00`
  for lamp-4ace), and is needed: without it both kinds of file fall on the
  wrong day. A scene's start is its own UTC date and time moved by the
  offset. A battery's is its `HH:MM:SS`, by the Mac's
  clock, which must be in the lamp's zone, on the day its file was written,
  or the day before when that time is later than the writing's. A scene
  folder keeps earlier days' files, and before `--date` one of the day
  before at the same hour was counted into tonight's arms (Jarvis on
  #1899). A line under the table says how many files fell in an arm, and
  an arm no file fell in prints "—" in the files' rows. From
  scenes: lines played (`PLAY`) and heard
  (`HEARD`), the lamp's turns and their end of speech to first sound (`[lamp
  starts speaking N s after that decision]`), how turns finished (`[turn
  finished: <reason>]`), yields and rejected inputs. From batteries (the
  first builder on #1899, 21:01): the two-people rounds first (`== round
  R, N: …`, then a line `Speaker: words | answer` each), as lines naming
  the lamp and how many were answered, and lines between people and how
  many were left alone; a round's words are read only for a word `lamp`
  set off as a name, not after `the`, `a`, `my`, `your`, `this` or the
  like ("turn off the lamp" between people does not call it).
  Then each section, begun by its `-- <suite>` header (`-- day`,
  `-- follow-ups (#1465)`, `-- hand-overs`): lines played and answered, from
  its own `… answered: N of M` line where it has one, or else its `|`
  lines. An answer is read only as blank or not. Rows by their marks and
  columns alone: no line's words reach the table either
  (`score::tests::scene_files_and_battery_summaries_add_rows_without_words`).
- **Not done:** the arms are typed by hand, so each must be one stretch
  of time. Taking them from the bench log's `== HH:MM:SS <label>-<arm>:
  <suite>` lines, per arm and suite (the asker's 18:49 shape), is a
  follow-up.

## Physical runner (prepared, not executed)

When Lamp returns, the operator first performs the readiness checks in the
[handoff](../HANDOFF.md): thermal state, mixer readings, physical privacy,
motor inhibit, exclusive ownership (stop installed services) and a rollback
plan. `lamp-voice-eval` does not stop or restore services, change mixers, move
motors or open SSH by itself; `lamp-voice` still refuses to run while legacy
owners are active, and such an attempt is retained as failed.

1. Build natively on the Lamp with the pinned toolchain:
   `cargo build --locked --offline --release -p lamp-voice-eval -p lamp-voice`.
2. Render the twelve new utterances once on the Mac that holds the existing
   cache, then check that `assets` resolves every scene to be run.
3. Write a private config (no credentials; it reuses the authorized SSH setup):

```json
{
  "lamp_command": ["/usr/bin/ssh", "-S", "/private/tmp/lamp-4ace-voice-ssh", "-o", "BatchMode=yes",
    "orangepi@172.168.20.159", "/home/orangepi/.local/share/lampOS-build/BUILD/lamp-voice-eval",
    "lamp-session", "--runtime", "/home/orangepi/.local/share/lampOS-build/BUILD/lamp-voice"],
  "lamp_work_root": "/home/orangepi/.local/share/lampOS-build/voice-eval-runs",
  "fixture_reply": "/home/orangepi/.local/share/lampOS-build/reply-fixture-76x62xf9/reply.wav",
  "fixture_sha256": "d5bd0d6f880434b81846accf5181b9bf5ab28ac91479827aa9f73ddf8b52e144",
  "noise_suppression": "on",
  "ring_channel_ceiling": 24,
  "stimulus_source": "loudspeaker_synthetic",
  "output_device": "iMac Speakers",
  "room_recorder": ["open", "-n", "-W", "-a", "/private/tmp/LampRoomObserver.app", "--args",
    "record", "--input", "EXACT INPUT NAME", "--seconds", "{seconds}", "--out", "{out}"],
  "room_independent": true
}
```

   Use `provider_config` (a private provider JSON on the Lamp) instead of the
   fixture pair for a provider, or `serve_config` for one long-lived serve runtime
   per attempt (see [serve-session runner](voice-eval-serve.md)). Either runs on
   the second route unless the config names `"voice_route": "live"`, with the
   second route's settings in `runtime_env` (the owner's 19 Oct 19:3x rule; the
   serve page says how). For a direct-human cohort set `"stimulus_source":
   "direct_human"` and `"human_speaker": "person-1"` (a label, not a name); the
   console then shows each line to say at its trigger and nothing is played.
   Prompt-only timed attempts stay unscored until reviewed attribution is supplied.
   Without `room_recorder` every acoustic boundary is unmeasured. Set
   `room_independent` to false when the recorder shares Lamp's hardware (for
   example the C-Media ambient input). Omit `ring_channel_ceiling` to run
   without the ring worker.
4. Dry run (the default) prints which attempts would run or be withheld:
   `cargo run --release -p lamp-voice-eval -- physical-run --config FILE --cache .cache/acoustic --manifest M... --out NEW_DIR --scenario fixed-reply-echo-only`
5. Add `--execute` for the authorized run. Each attempt starts one finite
   `lamp-session` (fresh private work directory, cue socket, `lamp-voice`, hard
   kill at `seconds + allowance`), maps clocks by ping/pong during the session,
   starts the room recorder and waits for its `ready.json`, starts each stimulus
   on its trigger, joins playback within a bound (cancelling through the
   sentinel file if needed), and imports the relayed `events.jsonl`. Then fill
   the annotation template and run `evaluate`.

With the fixture provider the ready scenarios are `fixed-reply-echo-only`,
`fixed-reply-acknowledgment` and `fixed-reply-topic-change`. Gemini sessions now
receive the same lifecycle cues, including admissions and local endpoints, so
follow-up and interruption scenarios can use actual playback events. Missing,
stale or invalid cues cannot be replaced by guessed delays. Provider-specific
and fault-injection restrictions still apply. See [directed cue evaluation](voice-eval-directed-cues.md).
An admission cue reports the runtime's decision; it does not prove that a human
addressed Lamp or that echo was rejected.

## Measured behavior of the runner itself

On the local loopback (debug build, one host, fake lamp-voice over the real relay
and cue socket), intended stimulus start to runner start request was 166.5 ms
(n=1) when stimulus timing was computed after the trigger, and 2.6/3.8/4.7 ms
(n=3) after moving it before the session. Clock-map uncertainty was about
±1.1 ms. These are runner software boundaries on one host; real SSH,
`lamp-observer` stream preparation and acoustic output add delay that the player
report and room audio must measure.

## Current offline results (simulation only)

The table below retains Agent 2's reported simulation baseline through
`3eaa0365a`. It has not been rerun with the integrated answer-review and
recording-provenance fixes and does not qualify the combined source.

`fake-run --repetitions 3` (seed 1, 126 attempts per profile, self-test 21/21)
against the lamp-voice directed policy at 23bde487. These describe what the
current turn policy does with declared speech; they are not Lamp measurements.

| Profile | Outcome | Main findings |
|---|---|---|
| `v2-directed-current` (perfect echo cancellation) | 60 passed, 60 failed, 6 incomplete | 90/120 complete answers; acknowledgments and asides interrupt 15/15; 15/18 expected-silence steps admit unaddressed speech; 6 turn splits from pauses; 6 sessions end instead of recovering; 9 silent provider failures; 0/21 missed interruptions; no voice/ring inconsistency in 123 ring-checked attempts. |
| `v2-echo-leak-h4` (one residual-echo burst) | 3 passed, 120 failed, 3 incomplete | Complete answers fall to 33/123; 33 overlap steps are withheld because the answer was already cancelled. |
| `v2-echo-loop` (a burst after every reply) | 3 passed, 123 failed | Self-sustaining admission chains as in zejfwxq7: 0/102 complete answers. |

Timing mode is level-blind: quiet, rapid and noisy-question scenarios pass by
construction there; `fake_signal` with the real cache or physical runs are
needed for them. See [voice defects](voice-eval-defects.md) for the ranked list,
owners and exact reproduction commands.

## Runtime integration and remaining proposals

- **P1 implemented:** Gemini-directed sessions now use [conversation lifecycle cues](live-session-cues.md),
  including admissions, endpoints and original cancellation reasons. Host producer
  and relay checks do not substitute for ARM64 or real Lamp qualification.
- **P2: utterance timing in render manifests.** Adding each utterance's cache key
  and measured active-speech span to `RenderReport` would replace text/energy
  estimates of clip boundaries.
- **P3: fault injection for physical provider tests.** Optional, test-only
  provider delays, stalls and disconnects for the recovery and delay scenarios.
- **P4 implemented for serve:** the serve runner arms `lamp-observer`'s
  prepared player before readiness and starts it with one expiring grant
  enforced at the first source copy. Finite directed runs keep their
  single-phase player.
- **P5: opening-word retention from capture diagnostics.** Align the stimulus in
  `pre_aec.pcm16le` with `prefix_first_host_read_us` in the same Lamp clock.
- **P6: a trace event for spoken failure notices**, once the runtime has one.
