# Supervised-pilot fault qualification

Agent 2, branch `lamp-a2-pilot-faults`, base `lamp-v2-chat` `e63f3152`. Main
owns scheduling, integration and release. Canonical `e9bdcea28` adds only the
reviewed Gemini Stop fence to this base; as Main agreed, the matrix stays on
`e63f3152`.

| Commit | Content |
| --- | --- |
| `ff22184a` | First matrix (12 scenarios) and the F1 reproducer, ignored |
| `1151380d` | Matrix completed and review fixes applied. Test file only |
| `33a86363` | **F1 proposal**: production change in `coordinator.rs`, with its regression. Separate from the matrix commits |
| `d787ba93` | This report, as reviewed |
| `r2` commits | Main's F1 review gaps closed: required revocation, labelled Stop-only stall, partial-then-stall, control progress without settlement, Duplex controls. Branch `lamp-a2-pilot-faults-r2`; the reviewed branch is unchanged |

## What this is, and is not

Every result here comes from an injected fault in a test. Nothing is a Lamp,
microphone, speaker, cloud or acoustic measurement, and no runtime flag was
enabled. The tests show what the production coordinator does when one
subsystem fails in a specific way. Whether real hardware fails in that way is
not shown.

## Harness (`crates/voice/src/coordinator/tests/pilot_faults.rs`)

**Evidence boundary.**

- Each worker's lifecycle is a real child process (`cat`) with the
  production Unix-socket channels, and the production supervisor reaps it.
- Each role's behaviour (capture, privacy, provider, speaker, ring, camera)
  is simulated on those channels. The 32 single-cycle scenarios explicitly
  use the stepped clock in [Voice fixtures](voice-test-fixtures.md).
  Rapid mute/reopen keeps real-time peers. The stalled metadata writer stays
  on a separate real thread, with an entered barrier before scenario work;
  only its synthetic peers use stepped time.
  Stepped scenario timestamps, including stall and revocation intervals,
  are policy time, not wall-clock latency measurements. Printed overall
  conversation/cleanup elapsed time remains real host time. The metadata
  scenario separately measures actual SIGTERM-to-reap and logging finish
  with host instants; its release guard also runs on assertion unwind.
- This is real coordinator, channel and reaping evidence. It is not
  evidence about the production worker processes, their drivers or the
  hardware.

- **Real loop.** Each test runs the production `conversation()` service loop
  (`Lifetime::Serve`, default options, V1-baseline as in the pilot), then
  `finish_active_run`. Rapid reopen also runs the production
  `service::supervise` and `service_privacy::wait_worker`.
- **Real children and IPC.**
  - Every worker is a real child process (a cooperative fixture) on the
    production Unix-socket channels.
  - Peers answer for capture (10 ms frames), the provider (credit-limited
    output), the speaker, and the optional ring and camera.
  - Capture begins on `StartCapture`. Delayed delivery retains each block's
    original 10 ms acquisition timestamp, with at most 64 blocks per peer
    step. Catch-up neither manufactures a source gap nor refreshes old audio.
  - In threaded scenarios privacy runs on its own thread and sends a fresh
    observation every 5 ms. Stepped scenarios run every peer before advancing
    2 ms, giving this peer a 6 ms cadence; production freshness is unchanged.
- **The speaker applies the production rule.**
  - It accepts PCM at the device rate (240 samples per 10 ms).
  - It installs every authority snapshot, and each chunk's own, in the
    production `BoundaryGuard`.
  - It checks each permit immediately before the write, as the real speaker
    does.
- **One fault per test, at an observed event.** Faults fire on events such as
  "the speaker accepted 3 chunks", "the provider received End" or "the
  authority snapshot shows no owner", not after fixed sleeps.
- **No widened bounds.** Production leases, freshness bounds and deadlines are
  unchanged. Emulated sessions run one at a time.

**Invariants asserted after every run:**

| Invariant | How it is checked |
| --- | --- |
| No stale dispatch | The coordinator never handed the speaker a chunk under an authority view issued after its owner finished, privacy was observed muted, or Stop was requested |
| No stale write | At every write the speaker allowed, its installed authority named that owner with the microphone allowed, and was at least as new as the chunk's own. The speaker had not yet installed a state revoking that owner. Audio already accepted before revocation is counted, not hidden |
| Bounded revocation | After the coordinator ended a turn, a speaker that installs authority must have installed authority without that owner within one channel packet age (100 ms). This is required: a missing revocation fails. It is exempt only for a speaker that installs nothing by design (the Stop-only stall, an abandoned peer) or had already stopped. Measured: 0.26–4.9 ms |
| No duplicate answer | One `turn_finished` per turn; no request and no chunk sequence twice (a set count, so `[1, 2, 1]` fails) |
| No false readiness | No `listening_ready` after a mute, a Stop or a terminal provider end. Across service cycles, readiness only inside a cycle, never while waiting |
| No orphan | Every role saw Stop unless its peer was abandoned by design. Only an abandoned role may be force-killed, matched by role. Verified cleanup succeeded unless the scenario expects a failure. Every role's child had exited and been waited for when the run returned |
| Bounded | Each run, including cleanup, ends within 12 s. Otherwise a test-only capture fault ends it and the test fails |

## Matrix

All scenarios use V1-baseline unless marked otherwise.

| Fault | Injected when | Result | Test |
| --- | --- | --- | --- |
| None (control) | — | `audio_written_unscored`. Exactly 11,520 samples in 48 blocks of 240, no padding, only the last final, one `speech_final_sample_retired` | `control_one_question_is_answered_once_then_mute_ends_the_run` |
| Provider TurnFailed before output | after End | `provider_failed`, no speech | `provider_failure_before_output_settles_failed_without_speech` |
| Provider TurnFailed during output | after the first accepted chunk | `provider_failed`; audio that arrived before the failure plays; never a completed answer | `provider_failure_during_output_never_becomes_a_completed_answer` |
| Provider process lost | after End / after 3 accepted chunks | `runtime_failed`; no speech / answer cut; run error "provider worker exited" | `provider_process_lost_before_output_ends_the_run_without_speech`, `provider_process_lost_during_output_stops_speech_and_fails_the_turn` |
| Provider worker `Fault` | during output | `runtime_failed` | `provider_worker_fault_during_output_fails_the_turn_and_ends_the_run` |
| Terminal provider link | during output | `provider_failed`; buffered audio retires; "terminal provider output settled; service stopped without retry" | `terminal_provider_link_during_output_settles_failed_and_ends_service` |
| Speaker stall, **Stop-only and non-installing** (consumes control, installs no authority, acknowledges nothing; no evidence of authority progress) | after 3 accepted chunks | `runtime_failed`, "speaker write evidence expired", 200–210 ms after the stall (asserted 190–300 ms) | `stalled_speaker_during_output_is_bounded_and_never_completes_the_answer` |
| Partial write, then stall (half of the next chunk accepted, nothing settled) | after 3 accepted chunks | V1: "speaker write evidence expired", 210 ms. `vad-only`: "speaker acknowledgment expired", 207 ms (both asserted 190–300 ms) | `partial_write_then_stall_is_bounded_under_v1_baseline`, `…_under_vad_only` |
| Control progress without settlement (installs every authority snapshot, never settles) | after 3 accepted chunks | V1: "speaker write evidence expired", 201 ms. `vad-only`: "speaker acknowledgment expired", 207 ms. The revoked turn is observed at the speaker (asserted) | `control_progress_without_settlement_is_bounded_under_v1_baseline`, `…_under_vad_only` |
| Duplex control and Duplex stall | — / after 3 accepted chunks | Answered once with exactly 11,520 samples. The stall ends with "speaker write evidence expired" at 208 ms: Duplex keeps V1-baseline's playback mask, whose lease reports first | `duplex_control_answers_once`, `duplex_stalled_speaker_is_bounded` |
| Same, under `vad-only` (F1) | after 3 accepted chunks | Before `33a86363`: unbounded. After: `runtime_failed`, "speaker acknowledgment expired", 199–209 ms (asserted 190–300 ms) | `stalled_speaker_under_vad_only_is_bounded_too` |
| Capture worker fault | during output | `runtime_failed` | `capture_fault_during_output_cancels_the_answer_and_ends_the_run` |
| Capture stops sending frames | mid-question | `runtime_failed`; no End sent, no answer | `capture_stall_during_a_question_ends_the_run_without_an_answer` |
| Ring fault (SPI) | during output | Ring degraded, blanked and exited; the answer completes in full; no new readiness; cleanup verified | `ring_fault_during_an_answer_keeps_the_conversation_and_cleans_up` |
| Ring exits without blanking | during output | The answer completes in full. Cleanup fails truthfully ("ring shutdown has no verified black-write receipt"), so the mute is not restartable | `ring_lost_without_blanking_keeps_the_answer_but_cleanup_fails_truthfully` |
| Camera stops reporting progress | during output | One `camera_unavailable`; only sight revoked; the answer completes in full; no new readiness; revoke and teardown receipts verified | `camera_loss_during_an_answer_revokes_only_sight` |
| Mute while answer speech is queued | after 3 accepted chunks | `privacy_muted`; nothing dispatched or written after revocation; nothing resumes | `mute_while_answer_speech_is_queued_cuts_it_and_never_resumes` |
| Mute while the failure notice plays | after 3 notice chunks | `privacy_muted`; the notice is cut and never retired; never an answer | `mute_while_the_failure_notice_plays_cuts_it_and_keeps_the_failure` |
| Rapid mute and reopen, 4 cycles | each cycle muted after 3 accepted chunks, privacy reopened at once | Each `privacy_muted`. Every chunk carries its own cycle's controller boot. The next cycle spawns only after every child of the previous one was reaped (about 110 ms later). No readiness while waiting | `rapid_mute_and_reopen_never_overlaps_workers_or_carries_speech_across` |
| Real SIGTERM | while answer speech is pending | `service_stopped`, clean `Ok` | `sigterm_while_answer_speech_is_pending_stops_cleanly` |
| Real SIGTERM | while the failure notice plays | Clean `Ok`; the notice is never retired | `sigterm_while_the_failure_notice_plays_stops_cleanly` |
| Real SIGTERM with metadata storage stalled from the first event | after 10 accepted chunks | Audio flows while the actual writer is blocked. Clean `Ok`; real SIGTERM-to-verified-reap stays below 1 s. `finish_logging` keeps its real 500 ms bound (250 ms host scheduling margin in the assertion) and reports unacknowledged loss. Checked release and a later writer-finish receipt clean up without changing that report | `sigterm_while_metadata_storage_is_stalled_stops_cleanly_and_reports_the_loss` |
| Failure notice | provider fails before output, then the person speaks during the notice | `provider_failed`; one attempt. The next owner is admitted only after the notice retires (the V1 mask holds speech over it), then answered. The notice is never replayed | `failure_notice_plays_once_keeps_the_failure_and_yields_to_the_next_owner` |
| Same, `vad-only` cross-check | as above | The notice yields mid-play. A notice chunk in flight is refused by the guard. The record keeps the provider failure (N1) | `failure_notice_yields_mid_play_to_a_new_owner_under_vad_only` |

Unit checks:

- `distinct_rejects_an_item_that_returns_later`;
- `speaker_progress_is_due_strictly_after_lease_plus_report_age` (F1).
- `capture_schedule_tests::a_delivery_pause_keeps_contiguous_original_acquisition_times`;
- `capture_schedule_tests::late_delivery_cannot_refresh_a_stale_admission_trigger`;
- `capture_schedule_tests::a_dropped_block_or_real_source_gap_still_faults`.

The capture checks use the real turn detector and input admission with
controlled timestamps. They preserve the 100 ms stale-input boundary.
Failure summaries report capture send-attempt gaps and source age at those
attempts, including unsuccessful sends. Rapid-reopen cycles print a failed
cycle's summary before the top-level service assertion; these producer
diagnostics are not consumption-age or lamp measurements.

## Findings and proposals

### F1. Stalled speaker unbounded outside V1-baseline (proposal `33a86363`)

**What happened.** Under `vad-only` (the option default; the same holds for
any policy without V1's write-evidence lease), the following leaves the
answer open until something else ends the run:

- the speaker stays connected and reads control,
- but it stops acknowledging chunks.

In the test, only the 12 s scenario limit ended it. The reproducer failed on
macOS and on the Linux VM at `1151380d`. V1-baseline was already bounded at
200 ms by its playback mask.

**The proposal (`coordinator.rs` only, plus one field in a test literal):**

- Each dispatched chunk carries a `report_due_us` deadline: the dispatch time,
  plus the output lease requested for every speaker chunk (100 ms, now named
  `SPEAKER_PERMIT_LEASE_US`), plus the 100 ms packet age every worker channel
  already enforces.
- **Why that deadline.**
  - A permit never outlives its lease; it may expire sooner, clamped to the
    input lease.
  - Once it has expired, the speaker may not write the chunk.
  - A live speaker therefore settles it (written, refused or discarded), and
    its report arrives within the channel age.
- If the chunk is still pending past the deadline, the run fails with "speaker
  acknowledgment expired", under every policy.

It is not a turn-taking decision:

- The check is evaluated once per capture block, after that block's policy
  checks, with the same block timestamp.
- The total equals V1's decision budget (`200 ms`; a unit test asserts the
  equality), and the check is strict where V1's is not. So V1-baseline's
  existing lease always reports first, with its own reason.
- **Measured over 6 runs each:**
  - V1 always reported "speaker write evidence expired", 200–208 ms after the
    stall;
  - `vad-only` reported "speaker acknowledgment expired", 201–207 ms after
    it.

No lease was widened.

An earlier draft used the permit's actual expiry. Because that expiry is
clamped to the input lease, it fired before V1's mask and changed V1's
reported reason. It was replaced before commit.

Main's static review (GO, subject to combined gates) notes:

- The deadline is checked on capture ticks.
- It is not a hard acoustic stop guarantee.

Not covered:

- A speaker that also stops reading control is force-killed at shutdown.
  That path existed before and is not changed.
- The check runs per capture block. If capture also stops, the capture
  lease ends the run first.

### N1. Interrupted notice keeps the provider failure (no change needed)

When a new owner interrupts a playing notice (`vad-only` only), the failed
turn's single `turn_finished` record contains:

- `"outcome": "user_interrupted"` for the output;
- `"provider_outcome": {"kind": "failed", …}`, the provider's failure,
  unchanged;
- `"failure_notice": {"state": "attempted", "answer": false, "retired_at_us": null}`.

`provider_turn_failed` also remains in the trace. The test asserts all of
these. The cause is not erased, and the notice cancellation is separately
visible. Under V1-baseline this case cannot arise.

### Retained failures and observations

- **FL1. Freshness loss, root cause in the harness.**
  - Two failures ended a test with "service privacy freshness lost before
    next observation" (50 ms production bound): one in the first batch, one
    in the camera case.
  - The retained log of the second shows the peer's own longest gap between
    privacy sends was 52.8 ms. The coordinator correctly failed closed.
  - Privacy now runs on its own thread. In the 13 later macOS runs the peer's
    longest gap was 9.8–43.5 ms, still within 50 ms.
  - Host scheduling on this busy M2 leaves little margin; Linux VM gaps are
    listed below.
- **FL2. One unidentified failure.** One serial macOS run had 1 failing test
  whose log I discarded by filtering it. That was my error. Nothing
  unidentified has recurred in the 34 later full macOS runs, whose complete
  logs are kept. One of them failed, with FL1, and that log is kept too.
- **O1. Intermittent failure in Main's test.**
  `failure_notice::failure_notice_failed_transport_keeps_attempt_then_uses_existing_cancellation`
  failed once in a full parallel macOS `lamp-voice` run with F1 applied.
  - It passed 10/10 alone.
  - The test expects a send to a just-dropped speaker socket to fail, which
    the Unix socket may accept once.
  - F1 does not touch that path. It is reported, not changed.

## Commands and results

From source-only exports (`git archive` of the commit) on my own M2 and its
Colima ARM64 VM (Rust 1.96.0, Debian bookworm); no office or device
resources.

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --no-fail-fast
cargo test --locked -p lamp-voice --lib pilot_faults
cargo test --locked -p lamp-voice --lib pilot_faults::stalled -- --nocapture --test-threads=1
cargo test --locked -p lamp-voice --lib
```

| Commit, host | Result |
| --- | --- |
| `1151380d`, Linux ARM64 VM | F1 reproducer (`--ignored`) fails, ended only by the 12 s scenario limit; `pilot_faults` 22 passed and 1 ignored, in 2 of 2 runs |
| `1151380d` content, macOS | fmt and Clippy clean; `pilot_faults` 22 passed and 1 ignored in 13 of 13 runs (10 serial, 3 parallel) |
| `33a86363`, Linux ARM64 VM | fmt and strict workspace Clippy clean. Workspace tests exit 0. Stalls: V1 "speaker write evidence expired" 204 ms, `vad-only` "speaker acknowledgment expired" 199 ms. `pilot_faults` 24/24 in 3 of 3 runs, with the peer's longest privacy gap 7.9, 32.1 and 10.0 ms. Full `lamp-voice` library 501 passed, 0 failed, 4 ignored, in 3 of 3 runs |
| `33a86363`, macOS | Clippy clean; the two stall tests in 6 of 6 runs with the reasons above; full `lamp-voice` library 500 passed and 1 failed (O1) in one parallel run |

| `95523a26` (r2), Linux ARM64 VM | fmt and strict workspace Clippy clean. `pilot_faults` 30/30 in 3 of 3 runs, with the peer's longest privacy gap 6.9–8.9 ms. Every stall control ended 199–210 ms after the stall with its expected reason. Full `lamp-voice` library 507 passed, 0 failed, 4 ignored, in 2 of 2 runs |
| `95523a26` content, macOS | Clippy clean; `pilot_faults` 30/30 in 5 of 5 serial runs, with the peer's longest privacy gap 14.5–32.6 ms |

Earlier failures, all retained: FL1 (2), FL2 (1) and O1 (1), above. The
`ff22184a` gates are in its checkpoint.

## Main integration verification, 2026-10-11

Main integrated only the reviewed four-file subset from `4505ccbe`, on
`6bf2a17b2`, with production code unchanged from `33a86363`. The source-only
composition passed `cargo fmt --all -- --check`,
`cargo clippy --locked --offline -p lamp-voice --all-targets -- -D warnings`,
and `cargo test --locked --offline -p lamp-voice --lib -- --test-threads=1`: 507
passed, zero failed, four existing ignored. Independent review verified R2's
required revocation and installing-stall assertions, plus the Duplex controls.
The partial-write fixture sends half a chunk but does not independently assert
that the coordinator consumed that particular report.

Under Linux/ARM64 emulation, the same composition passed format, strict live
all-target Clippy, all 30 `pilot_faults` tests and a release build. Main first
used an incorrect `coordinator::pilot_faults` selector: Cargo ran zero tests,
and the expected-count check correctly rejected that gate. The retained second
attempt uses `pilot_faults`; no source changed and passed format/lint stages
were not repeated. The ARM binary was exported and hash-verified locally.
[Exact commands, digests and limitations](verification/pilot-fault-integration-20261011/results.json)
remain separate from Agent 2's branch gates and all physical measurements.

This adds a bounded speaker-acknowledgment failure under every policy. The
V1/duplex playback-mask deadline still reports first. These are coordinator
and injected-fault results, not a measured acoustic stop or a device deployment.
The installed pilot continues to use the earlier qualified binary.

## Start-up after a restart or a power cut (#762)

Two of the voice's start-up failures, found on lamp-52e6 and lamp-4ace on
2026-10-15, no longer end the service:

- **The microphone busy at start.** `systemctl restart lamp-voice` on
  lamp-52e6 found the microphone busy (`snd_pcm_open failed with errno 16`)
  330 ms after the old process had gone, once in three restarts; the power
  cut that morning did the same. The capture worker now tries the open again
  for up to 8 s (`BUSY_AT_START`); any other open failure, or busy for
  longer, ends the worker as before. The bound was 3 s until lamp-4ace on
  2026-10-18 (17:48, #1567): the mic was still busy 3.5 s after the new
  voice started, most likely held by the desktop session's PulseAudio, whose
  `module-udev-detect` opens a card nobody holds and keeps it until
  `module-suspend-on-idle` lets go, 5 s idle by default. On the first busy
  answer the worker says who holds the device, from `/proc/*/fd` links to
  its `/dev/snd/pcmC<card>D<dev>c`, by pid and process name only (never the
  command line): `lamp-voice: capture: microphone busy at start (held by
  pulseaudio[80598], looked up in N ms); waiting up to 8 s`, or `holder not
  visible` when no holder can be read. The scan runs on the capture thread
  before the mic is open, so it stops after 100 ms (checked at every
  descriptor too) or 4,096 `/proc` entries and says `holder not visible
  (scan capped)`; `N` is the scan's measured time. A name that is no card (no `/proc/asound/<id>`: on the ReSpeaker and
  XVF3800 images `device_micro2` is only an `asound.conf` alias) is not
  looked up, and the line says `holder not looked up: device_micro2 is no
  card`. Until
  2026-10-17 this held only for the scripted microphone: the check looked for
  a failed call named `open`, the scripted device's name for it, while ALSA
  names it `snd_pcm_open`, so on a lamp a busy microphone still ended the
  worker. Both now name it `lamp_audio::device::OPEN_CALL` (ALSA's), and a
  test checks that name against ALSA itself (`capture_linux.rs`). A busy
  microphone is now tried every 50 ms, not on every 2 ms tick.
- **The microphone off USB at start.** On lamp-4ace on 2026-10-17 the voice
  mic fell off USB for 4-5 s, twice (12:29:20 and 12:49:06); the new run's
  open failed with `snd_pcm_open failed with errno 19`, the run ended and the
  restart guard muted the lamp (#1178). The capture worker now tries an open
  that fails with `ENODEV` again every 250 ms for up to 10 s
  (`GONE_AT_START`), inside the coordinator's 25 s for input to be ready, and
  says once how long it waited. Longer, it ends the worker as before. Each
  reason's bound runs from its own first answer, so a microphone that comes
  back from off USB and answers busy for a moment still gets its 8 s. A mute
  during the wait opens nothing and ends the worker. Only the start is
  covered: a microphone lost mid-run still ends it. A wait that ended with
  the microphone open is said in one plain line, the same in three places:
  the journal (`lamp-voice: capture: the voice mic was off USB for 4.3 s at
  start; opened on try 18`, or `was busy`), the session's events
  (`mic_waited_at_start`, with `said` and the numbers) and the voice's
  health, so `lamp-health show` names it beside `voice` (after the link's
  warning, if the link is down) until the next capture start: the voice's
  own, or the one after a mute or a sleep, which clears it if the microphone
  opened at once.
- **The speaker busy at start.** On lamp-06c1 on 2026-10-30 (boot
  `b8c51349`) PulseAudio held the C-Media speaker after a restart: busy from
  10:17:01, free again at about 10:17:10, some 9 s (`module-suspend-on-idle`
  lets a card go 5 s after its last use). Two runs in a row ended on the
  speaker's one open (`EBUSY` at 10:17:01 and 10:17:06) before the third
  started (#4332). The speaker worker now waits out a busy open as the
  microphone does, every 50 ms, but for up to 20 s
  (`SPEAKER_BUSY_AT_START`, in `open_at_start.rs`), not the microphone's
  8 s, which those 9 s would beat: a wait inside the run is cheap beside a
  restart, which an update's 60 s watch does not ride out. Off USB, any
  other open failure, or busy for longer ends the worker as before. It
  waits before it says it is ready, so no authority has been sent to it and
  no lease can lapse. At the first busy answer it tells the coordinator
  (`busy_at_start`), which waits for its readiness up to 20 s longer, once,
  keeps the other workers' leases renewed meanwhile, and still ends the
  start on a stop. The journal says `lamp-voice: speaker: the speaker
  plug:device_speaker is busy at start; waiting up to 20 s for it, as the
  microphone waits for its own`, then, once open, `the speaker was busy for
  9.2 s at start; opened on try 185`. Its holder is not looked up.
- **The privacy worker's lease lapsing in the muted wait after boot.** On
  lamp-06c1 on 2026-10-30, a cold boot, run 1: `onboarding skill loaded`
  at 15.82, `schedule-consumer: due reminders available` at 15.89, then
  `privacy controller lease expired` at 17.47 and `privacy switch
  unreadable: privacy.c.p: Connection refused (os error 111); verified
  cleanup failed: privacy worker exited abnormally: exit status: 1` at
  17.78: the voice exited 1 and systemd restarted it (#4332). It died in
  the service's muted wait, before any session. The wait's privacy worker
  got its authority only after each look's gate (`Waiting::step` in
  `service_privacy_controls.rs`), and the gate (the reminders' poll, the
  say socket's refusal, the light, the controls' quiescent observation,
  which may write the mute record, and the health's report, each with its
  trace lines) took past the worker's 250 ms lease on that boot. The look
  now also renews the worker's authority before its gate and between the
  gate's checks, when 50 ms or more have passed since the last renewal.
  It is the wait's microphone and camera Denied authority, so nothing is
  granted, and no lease length, freshness or fail-closed rule changed: one
  check held past 250 ms still lapses the lease and ends the wait closed.
  A look whose gate takes 50 ms or more after the switch has been read
  still finds its readings stale and ends that wait as unreadable, closed,
  read again after 1 s with a fresh worker, as before; only the lapse that
  ended the voice is gone. With `LAMP_STALLS=on` each look is timed in
  `stalls-wait.log` and each check in its own `stalls-wait-<check>.log`
  (`reminders`, `say`, `light`, `controls`, `owed`, `health`, `state`), so
  the next slow boot names the slow one. A cold boot of lamp-06c1 with that
  in, run 2, still logged `privacy controller lease expired` 250 ms after the
  worker's Ready, before the wait's first line, and exited 1: the first
  renewal came only inside the first look, after the worker's start, the
  wait's controller, its stall recorders and the first look's start. The
  wait now builds its controller before it starts the worker and sends the
  first authority as soon as it has read the worker's Ready, with no 50 ms
  rule holding it back; a refused send there fails closed like any other.
  Each refused renewal now ends its line with which it was: `(renewal after
  Ready)`, `(renewal before the gate)`, `(renewal in the gate after
  <check>)` or `(renewal after the gate)`; the rest of the line is as
  before. Unit tests: a look whose gate takes ~400 ms in ~100 ms checks
  keeps the stand-in worker's authority arriving within 250 ms (it lapsed
  at 250 ms before), and one 320 ms check still lapses it; a worker whose
  Ready is read ~100 ms after its start, then ~200 ms to the first look and
  ~100 ms in its gate, keeps its lease with the authority at Ready and
  lapses without it; each refused renewal names itself. A fixture worker,
  not tried on a lamp. Then 1 cold boot of 6 on lamp-06c1 with both in
  still ended: one pass of the wait took 890.8 ms at 15.28 s, with 5 major
  faults, 6 blocked processes and 1250 dirty pages on the board, and the
  privacy worker ended on `authority snapshot has expired`, exit 1: the
  wait's Denied authority had lapsed in the worker's queue, and installing
  it was fatal. The worker now rides a lapse of a lease that grants
  nothing (`DeniedLapse` in `privacy_bus.rs`): an expired snapshot with the
  microphones and the camera Denied and no turn is skipped, the worker
  publishes nothing meanwhile, and the next fresh authority re-arms it, said
  once as `lamp-voice: privacy worker re-armed its denied lease after <N>
  ms lapsed (microphones and camera stayed closed)`. The coordinator is
  unchanged: it never refuses a snapshot of its own, and after such a stall
  its own stale readings already end that wait as unreadable, read again
  after 1 s with a fresh worker, which the worker's clean stop now allows.
  A lapse of a lease that grants anything, a Denied lapse past 1.5 s from
  its expiry, no authority within 250 ms of the start (now ridden, below),
  the publisher gone and a worker that ended abnormally all end as before. Unit tests: a
  Denied lease lapsed by a 900 ms stall is ridden with nothing granted and
  re-armed with one line; the 06c1 queue (an expired Denied authority) is
  skipped and an expired granting one still ends with "authority snapshot
  has expired"; granting, Unknown, past-the-bound and no-authority lapses
  still end; a worker dead in the wait still ends the voice. Two Linux
  tests of the real worker (a 900 ms Denied lapse ridden, a granting lapse
  ends) run on the ARM64 gate only. Not yet tried on a lamp. Then cold boot
  6 of lamp-06c1 still ended: the voice's main thread stalled from 16.341 s
  to 17.013 s, the privacy worker had had no authority 250 ms after its
  start and ended on `privacy controller lease expired`, exit 1, the
  voice's renewal after the gate got ECONNREFUSED, and the cleanup came back
  `CleanupFailed`, so the voice exited. No authority yet grants nothing, so
  the worker now rides it like a Denied-only lapse from 250 ms after its
  start, publishing nothing, for at most 1.5 s more, said once as
  `lamp-voice: privacy worker has had no authority 250 ms after its start,
  riding it up to 1500 ms (microphones and camera stay closed)`. A Denied
  authority during that ride re-arms it at once; none by 1.75 s still ends
  it as before. Retrying a worker that ended with status 1 on the voice side
  was rejected: the exit is how every other worker failure ends the wait,
  and a retry would hide those too. Unit tests: no authority is waiting at
  250 ms and just before 1.75 s and ends at 1.75 s; a Denied authority
  during the ride re-arms it at once as Denied, and a lapsed one keeps it
  closed. Not yet tried on a lamp.
- **A killed privacy worker's sockets left in the muted wait.** On
  lamp-06c1 on a cold boot of stock 9528 (`on4`), the wait found no switch
  reading within 50 ms of Ready, its privacy worker did not stop within the
  300 ms grace and was killed, and at 17.96 the voice exited 1 on
  `privacy switch unreadable: Controls privacy unavailable while waiting;
  verified cleanup failed: privacy worker privacy wait shared stop deadline
  exceeded; reaped with signal: 9 (SIGKILL); verified cleanup failed:
  Directory not empty (os error 39)` (#4332). Each socket's owner unlinks
  its own path as it ends; a killed worker cannot, so its `privacy.c.w` and
  `privacy.d.w` stayed and the session directory could not be removed. The
  wait's cleanup (`killed_privacy_worker` in `service_privacy_controls.rs`)
  now, once that worker is reaped, removes those two paths and only them,
  each only while it is a socket, read with `symlink_metadata`, so a
  symlink is never followed. The kill is then the switch unreadable, read
  again after 1 s with a fresh worker, also when the look had allowed; a
  wait that had failed otherwise keeps its own error, and the kill is said
  on its own line. On this route the worker only reads Controls' bus and
  holds the wait's Denied authority, with no microphone and no GPIO line,
  so its end grants nothing. Any other path there, a worker not reaped, a
  ring indicator that failed without its verified black write (see the next
  entry) and any other cleanup failure still end the service as before. Unit tests: a stalled stand-in killed in the wait
  leaves no socket and no directory and is read again (before: a cleanup
  failure); a regular file or a symlink to a socket at either path is left
  and still fails; a worker that stops keeps its sockets for itself to
  unlink. Not yet tried on a lamp.
- **An update's stop taking the muted indication down with the voice.** On
  lamp-3e83, after every update, the voice was in its muted wait (switch
  closed, microphones closed) with the slide's indicator writer (`worker ring
  --privacy`) showing the mute. The update stopped `lamp-voice.service` at
  07:15:25.170 and systemd took the writer down at the same moment; at .209
  the wait's cleanup said `verified cleanup failed: ring.c.p: Transport
  endpoint is not connected (os error 107); unexpected privacy indicator
  event: Fault { code: "privacy ring…`, at .218 the voice ended
  `status=1/FAILURE`, and at 26.199 the next start said `muted by the restart
  guard after a fault, not by choice`, which only a head tap cleared (#4332).
  The wait (`run` in `service_privacy_controls.rs`) now takes the indicator's
  trouble as the stop's when the stop was asked for (SIGTERM or SIGINT seen
  by the end of the wait's cleanup) and the indicator either failed after it
  or at most 1 s before it (`STOP_WINDOW_US`; on 3e83 about 40 ms, as
  systemd signals the whole unit at once). The cleanup still runs as before
  (the writer's Stop and verified black write tried, the writer reaped), and
  then, if the privacy worker's own part of it was verified, the indicator's
  errors are said on one line (`lamp-voice: the muted indication ended with
  the stop, not a fault: …`) and the wait ends stopped, so the service ends
  with 0 and the restart guard is not armed. A privacy worker whose cleanup
  failed, or an indicator failure more than 1 s before the stop, still ends
  the service as before.

  An indicator failure in the muted wait with no stop near no longer ends
  the voice either. The wait stays muted, its microphones closed (its
  authority is Denied throughout), says `muted indication failed: …`, and
  reaps the failed writer there: its Stop sent, its events read for its
  black write (the shutdown receipt for a write after that Stop, or the
  control-lost one a writer makes as it ends on a fault), killed after
  300 ms if it has not ended. Once it has exited, its black write verified
  and its ring paths removed, it is gone and the wait starts it once more
  at the next muted look. If that one fails as well, or cannot start, the
  wait goes on muted with no light, as with a writer that never started
  (`RingStartup::Unavailable { worker: None }`). Leaving the wait still needs
  what it always has: the software latch clear, a fresh reading of the
  switch open and nothing else queued. A head tap's clear alone never
  leaves it, so a voice that lost its light comes back unmuted only once
  the slide is opened, and then its conversation's ring shows it listening.
  The one case that stays fatal is a writer whose black write is not
  verified (an SPI fault with no receipt, a writer killed with no receipt,
  or one that failed at its start and did not write black on its Stop):
  the person may still see a mute light, or none, that the voice cannot
  vouch for, and no second writer may follow it. That failure ends the wait
  with the same error as before. Unit tests: an update-style stop with the
  writer dying with it, and one 100 ms after the writer died, end the wait
  stopped with no second writer; a failure 1.3 s before the stop is still
  the error; a failure with no stop, its black write verified, keeps the
  wait muted through 200 ms of a clear software latch, starts one writer
  more (one that comes back, and one that cannot start), and leaves only
  when the slide opens; a failure with no receipt starts no other writer and
  still ends with the error; a writer that failed at its start without its
  black write still ends with the error, and with it the wait goes on. Not
  yet tried on a lamp.

- **A worker's first control message older than 100 ms.** A worker sends its
  first control message as soon as its peer is there; at start the
  coordinator may read it only after starting its other workers. lamp-4ace
  ended twice on such a message ("stale, reordered or wrong-boot runtime
  message") after a plain restart. The first message on a worker's control
  channel may now be up to 1 s old (`FIRST_CONTROL_MAX_AGE_US`); later control
  messages and every data message but the provider's input (below) keep the
  100 ms bound, and a refused message names its socket (for example
  `capture.c.p: stale, …`). A receiver held up later in its run is #784's
  (`HELD_UP_US`), separate from this.
- **The microphone's first frame older than 100 ms.** Capture sends its first
  frame as soon as it is asked to; lamp-0c89 at build 8545 read one 154 ms
  old at boot (`capture.d.p: stale, … sequence 1 after 0`) and the voice
  exited 1 (#4332). Capture sends a frame every 10 ms, so the frames behind
  the first are stale too. The run of frames at the start of `capture.d.p`,
  in order from the worker's boot and stale by their age alone, is now
  dropped up to the first fresh one and said once, with its count, the
  oldest and the bound (`capture.d.p: 6 stale first messages dropped, the
  oldest … us (bound 100000 us), the next taken`). The run is bounded: once
  its first frame is over 1 s old (`FIRST_CONTROL_MAX_AGE_US`), a stale one
  ends the voice. A frame from another boot, out of order or stamped after
  now ends it anywhere in the run, and once a frame is taken the frames
  after it keep the 100 ms bound. If nothing fresh is waiting yet, nothing is
  taken and the next look goes on with the run. The bound itself is
  unchanged, and no other channel drops.
- **Every worker's lease lapsing in the voice's startup prologue.** On a
  test lamp at build 8545, the voice's first start after boot set itself up
  for about 0.9 s between starting its workers and its loop's first publish
  (#4332). `spawn_rig` renews each worker's authority every 20 ms while the
  workers start, but nothing renewed it in the conversation's setup after
  that (storage reads, sockets, configuration loads, ~30 bus endpoints). The
  field timeline puts the last renewal at about 02.00, right after the
  voice's last configuration line (`asker first …`) and its bus writers, and
  every 250 ms lease lapsed together at 02.254: the privacy worker ended on
  `privacy controller lease expired`, fail-closed as it must, the voice
  exited 1 and came back muted by the restart guard. The setup now renews
  them as it goes: between its groups it re-sends the controller's current
  snapshot, freshly timed, to every worker when 50 ms or more have passed
  since the last renewal, exactly as the loop's first publish does. Nothing
  is granted there (the microphone stays denied until the loop allows it),
  and no lease length or expiry rule changed: a single setup step held past
  250 ms still lapses them, and the privacy worker then ends closed unless
  it rides that lapse (`DeniedLapse`, above). With
  `LAMP_STALLS=on` the coordinator's stall recorder now starts with the
  setup, so a slow stretch of it is a gap line in `stalls-coordinator.log`.
  The privacy worker's observations queue up through the setup, each older
  than its 50 ms freshness when the loop reads them, so the voice's 1 s start
  window for a stale one now counts from the loop's start, just before its
  first publish, not from inside the setup: a setup past a second no longer
  ends the voice at the loop's first pass. The window's length and the gate
  are unchanged; the microphone stays denied until the loop allows it.
  Stepped coordinator tests make the setup take 0.8 s and 1.3 s of logical
  time and check that no worker's authority lapsed and that the queued
  observations did not end the voice; they use fixture workers, not the
  real privacy worker, and were not tried on a lamp.
- **The second worker's startup renewals queued behind a slow first look.**
  While the voice starts, the coordinator sends every worker an authority
  every 20 ms. A second worker (`LAMP_VOICE_ROUTE=second`) that did not read
  for about 120 ms or more after Ready took the first on the first message's
  grace and the second late, then ended on the third (`second.c.w: stale, …
  sequence 3 after 2`), and the voice with it: a candidate for #4332's
  unexplained `exit status 1` at boot, not yet matched to that lamp's
  journal. Its control channel now passes over a stale authority renewal
  with the next renewal right behind it and judges that one, as the audio
  workers do (`receive_renewed`, #1569 row 42), and says the run
  (`second.c.w: a renewal … passed over`). Anything else stale, out of
  order, from another boot or stamped after now still ends it, as does a
  stale renewal with nothing fresh behind it; the 250 ms lease still ends it
  when the coordinator stops renewing.
- **The first direct capture-ready message while the speaker starts.** On
  build 6963 after setup, lamp-dbda reported `ref.c.speaker` sequence 1
  after 0, aged 143402 us against 100000 us, then restarted muted (#3530).
  The report does not establish why that message was delayed. Unlike the
  worker control channel, the direct reference channel had no startup
  allowance. Its speaker now accepts only the initial, in-order
  `CapturePrepared` with the same less-than-1-second base startup allowance.
  The existing receiver-stall allowance still applies, so this is not an
  unconditional wall-age cap; the exact expiry test uses a clean first look.
  A wrong boot, future stamp or noninitial sequence does not gain this
  allowance. Prime acknowledgements, subsequent controls and reference
  data retain their existing bounds and receiver/single-sender stall rules.
  Capture readiness grants no playback authority: the fresh authority,
  privacy generation and completed prime are still required. The restart
  guard is unchanged. Deterministic socket cases cover the reported age,
  upper boundary and refused alternatives; the Linux null-device worker
  witness can establish message consumption, not successful acoustic
  startup. An assembled-lamp setup repeat remains required for acceptance.
- **The provider's input, late in order.** On lamp-4ace at 05:08 on 19 Oct
  the voice ended on `provider.d.w: stale, reordered or wrong-boot runtime
  message (sequence 3916 after 3915, sent 100181 us before now, bound 100000
  us, same boot true)`, and came back muted by the restart guard. The
  provider worker reads its input one message at a time, and none while a
  command waits for room in Gemini's input queue; with each look under the
  100 ms bound, no look counted as the receiver held up, and the backlog
  behind grew past the bound (which of its waits held it at 05:08 the journal
  does not say): the first
  late message was taken as a held-up sender's, the second ended the voice.
  Nothing on that channel carries authority of its own: every Start, Audio,
  End, Called, Written and Reminder is submitted only under the newest
  snapshot from the worker's control channel (`provider.c.w`, a lease with
  its own expiry and heartbeat), with its privacy generation and its turn's
  owner. So the provider's input (`Channel::taking_late`) now takes a
  message that comes late, in order and from the parent's boot, and says the
  first of a run (`taken late, in order: the receiver was behind`) and the
  run when the next is on time. A message from another boot, out of order or
  after one missing is still refused, and every other channel keeps its
  bound (a test binds each worker's four channels and refuses the second
  late message on all but this one). How late a message can be is bounded by
  the worker itself, not by its control heartbeat, which it drains while a
  command waits (Tests on #1660): the command waiting for room may be
  `ROOM_WAIT` (250 ms, 1.05 s with patient writes) plus 100 ms old
  (`check_retained`), the parent's sends fail once the socket's buffer is
  full, and audio more than 2.5 s from its capture
  (`lamp_gemini::MAX_INPUT_AGE`) ends the worker as stale. A run of late
  messages is said as it ends, `on time again`, `ended by an error` or
  `closed` with the channel, with its count and its oldest, and its first
  line names the bound it failed, the receiver's held-up time included (the
  reviewer on #1660).
- **An audio worker's stale authority renewal, with the next renewal behind
  it.** A worker that starts slowly (the speaker opens its device before it
  first reads) finds its control messages queued: the first passes on its
  grace, the second as the one late message in order, and the third, the
  20 ms authority renewal, was refused as stale and ended the worker
  (`speaker.c.w: … sequence 3 after 2, sent 113195 us before now, bound
  100000 us`, three times on ARM64 on 20 Oct, #1569 row 42). The capture and
  speaker workers now read their control channel with
  `Channel::receive_renewed`: a renewal stale by its age alone, with the next
  renewal in order queued right behind it, is passed over, since that one
  carries the authority now, and at most it costs the gap a lapse costs
  (#1178). The next is judged by the channel's own rules in turn. The first
  pass of a run is said (`a renewal … us old (bound … us) passed over: the
  next renewal is behind it`), and the run as it ends, with its count and
  its oldest, as for late messages (`on time again after 2 renewals passed
  over, the oldest … us`), not a line a renewal. A renewal stamped after the
  reading is not stale by its age: it is refused as before, its channel
  named. A stale renewal alone or followed by any other
  message, a stale message of any other kind (a `ConnectReference`, a
  `Stop`), and reordered and wrong-boot messages still end the worker, since
  those carry state a skip would lose (the first builder on #1722, 20 Oct
  17:46).
- **A late privacy observation while the voice waits to open.** lamp-4ace's
  power cut at 18:15 ended the voice at 13.56 s with "invalid or stale
  privacy observation while waiting", while every other part was starting.
  The privacy worker reads the mute switch every 5 ms and an observation is
  fresh for 50 ms (`PRIVACY_FRESH_US`); one read late by a busy boot was
  fatal. Every clock of the observation itself is `CLOCK_MONOTONIC`; the
  bus header's lifespan and lateness were judged by the wall clock until
  queue #3997 C (the next entry). While nothing is open, a
  late observation now only keeps the gate closed; the wait ends only when
  no fresh observation has come for 1 s (`UNAVAILABLE_WAIT`). The gate's own
  rules are unchanged: only fresh, ordered, stable observations open it.
- **A stale privacy observation as the conversation starts.** On lamp-4ace
  at 01:19 on 19 Oct, 0.7 s after an install's start, the voice ended on
  "invalid or stale service privacy observation" and came back muted by the
  restart guard: the first observation the conversation read had waited
  past its 50 ms in the queue while the voice started its other workers.
  The same rule as in the wait above now holds there until the microphone
  is first allowed, within the first second of the conversation loop, whose
  clock starts once the workers are started (`PRIVACY_START_STALE_US`): a
  stale observation closes the gate, is said once (`a privacy observation N
  ms old was read as the voice started`, traced as `privacy_stale_at_start`),
  and a fresh one behind it opens the gate as usual, 60 ms stable. So is any
  freshness lost in that second: a worker that sends one fresh observation
  and then goes silent ends the run at 1 s, not at 50 ms. Nothing is heard
  meanwhile; it fails closed, and it is bounded (the reviewer on #1666).
  Still ending the run: a fresh observation out of order (a stale one is
  waited past like any stale one, the gate closed and its high-water kept);
  no fresh one by the end of that second (`service privacy freshness lost
  before next observation`); a worker that never says anything, at its 50 ms
  as before; and any stale observation once the microphone has been
  allowed. 18 Oct's 08:34 exit (`service
  privacy freshness lost …`) is the same kind only if it came before the
  microphone was allowed; its journal does not say.

- **The speaker underrunning at start.** On lamp-52e6 at 09:18:44 on
  2026-10-29, 0.39 s into a plain `systemctl restart lamp-voice`, the
  speaker's `snd_pcm_avail_delay` failed with "Broken pipe (32)", the
  speaker worker exited 1 and the restart guard muted the next start
  (#3697). An underrun now stops the device, keeps the guard and primes
  again, saying `lamp-voice speaker: it underran while held up with only
  zeros queued; priming again`. Mid-answer it cuts that answer, as a lapse
  does, and says `it underran while held up mid-answer; the answer is cut,
  priming again`. More than 3 in 10 s still ends the worker
  (`audio-device-seam.md`).
- **Capture still opening the microphone when the voice stops.** On
  a cold boot of lamp-06c1 (boot 1 of 4), the conversation lost its privacy
  freshness at 18.29 s (`service privacy freshness lost before next
  observation`), which the voice waits out with the microphones closed.
  The coordinator sends Stop to every worker at once. Capture was still in
  `prepare()`; the speaker took its Stop and closed its end of the
  reference link; capture then told the speaker it was prepared, and the
  send was refused (`ref.c.capture: Connection refused (os error 111)`).
  Capture ended with that fault, the cleanup failed (`verified cleanup
  failed: capture WorkerFault: … capture WorkerExit: … exit status: 1`),
  and a failed cleanup is never waited out, so the voice exited 1 (#4332).
  Boot 4's same freshness loss had a clean cleanup and the voice was back
  in the wait, ready 0.2 s later. Now, once `prepare()` returns, capture
  reads what the coordinator queued meanwhile (`peer_stop.rs`), keeps it in
  order for its next tick, and when a Stop is among it tells neither side
  it is prepared and ends stopped at that tick. Either audio worker whose
  reference link (`ref.c.*`, `ref.d.*`) is refused with the coordinator's
  Stop already queued for it ends stopped too, saying `lamp-voice: capture:
  the speaker had gone at the voice's stop (…); capture ends stopped` (or
  the speaker's line the other way round). The speaker's end there is its
  Stop's: the device stopped, a paused answer's gap reported cancelled,
  the reset recorded. The coordinator still sends the speaker's Stop
  before capture's, unchanged. Either order leaves one worker a window,
  of one send on an unblocked socket, in which its link is refused
  before its own Stop is queued; that end is still a fault. Sending
  capture's first would move the window to the speaker, not close it, and
  would mostly lose the speaker's `Stopped` notice, on which capture
  ends cleanly. A refused link with no Stop
  queued, a refused coordinator channel, and any other error still end the
  worker with its fault, and a failed cleanup still ends the voice with
  the first reason first in its line (`privacy switch unreadable: …;
  verified cleanup failed: …`). Tests: unit tests of the look for a Stop
  (a refused link with a Stop queued or read ahead is clean, without one
  or on another socket it is not, and what is read is held in order,
  bounded); four Linux tests of the real capture worker with the
  microphone's open held (a Stop then the speaker gone ends it cleanly,
  where it ended on `ref.c.capture: Connection refused` before; a Stop with
  the speaker there tells neither side it is prepared; the speaker gone
  with no Stop still ends it on the refused link; renewals queued in the
  open are still taken, and the stream and stop are as before); two Linux
  tests of the speaker's end on the `null` device with a send refused by
  a closed capture side (with its Stop queued it ends stopped, the device
  stopped and the gap reported; without one it is still the fault). The
  Linux tests run on the ARM64 gate only. Not yet tried on a lamp's cold
  boots.

- **A held turn's leases lapsed when its capture frames stopped.** On
  lamp-e82d on 3 Nov, on stable lamp-voice, during the DeepSeek 503 outage,
  the awake voice exited 1 on `privacy controller lease expired`: the ring's
  and the speaker's authority lapsed within 13 ms of each other, then
  `worker_exit_failed` (privacy, exit 1), and the run ended with status 1,
  which the restart guard counts (#4332). From the code: while a turn is
  held, every worker's lease is clipped to the input leases, the last kept
  capture frame's read time + 100 ms, and the 20 ms renewals do not move
  that until a newer frame is kept. Inferred from the journal: frames
  stopped being kept about 0.29 s before the end of the failed turn, so
  the clipped expiry froze and every lease lapsed before the lapsed
  capture lease cut the turn; the privacy worker ending on a lapsed
  granting lease is by design (fail-closed). The coordinator now cuts a
  held turn as a capture gap once its lease ends within 40 ms
  (`HELD_TURN_LEASE_MARGIN_US`), judged on the frames its last capture
  drain kept, so a pass held up while frames keep coming cuts nothing,
  and only once a frame has been kept, through the same cancel and publish a
  lapsed capture lease takes, so the workers hold its revocation, a full
  250 ms lease with no turn, before that expiry; it says so once per gap
  (`lamp-voice: no capture frame kept for <N> ms while a turn is held: it
  is cut 40 ms before its lease lapses, as a gap cuts it`). Nothing is
  extended: the capture lease still ends at the last kept frame + 100 ms.
  Unit tests: in the harness a 500 ms capture stall while the lamp answers
  leaves no peer, the privacy peer included, holding a lapsed granting
  lease at any step (before: all four peers held the turn's lease at its
  expiry, 90 ms into the stall, as its revocation was only being sent),
  nothing is heard after the last frame's lease, and
  the reply is cut once as `capture_gap`; a pass held up 36 ms after its
  drain while frames come 30 ms late, on time, cuts nothing (before: the
  next pass cut it as `capture_gap`); the margin check is true only
  for a held turn whose lease ends within 40 ms. Not yet tried on a lamp.

- **The voice's processes stalled together on a page read back from the
  card.** On lamp-06c1 at 21:30:02 on 6 Nov, during an SD log flush,
  capture, the ring and the coordinator stalled 120-132 ms together, each
  on exactly one major fault, with 606 dirty pages and nothing blocked
  (`stalls-*.log`, #5611). 120 ms is past a held turn's 100 ms lease, so
  the leases lapsed. Inferred: the page was `lamp-voice`'s own shared text,
  evicted by the flush. With `LAMP_VOICE_LOCK_CODE=on` (off by default) the
  voice and each worker `mlock` their executable file mappings, the
  executable's and its shared libraries' text, and say the bytes locked in
  one line; a failed lock is that line and the voice runs on. The lock's
  thread takes the ordinary policy first, not the inherited RR 20. The
  writable segment stays unlocked and a fault served from zram is not
  removed; the lamp row's major-fault check is the gate for both, and it
  also times the start to `listening_ready`, cold and warm, on and off
  (`serve-runtime.md`, "The voice's code kept in memory"). Unit tests: the
  maps parser picks only `r-xp` file mappings; a failing `mlock` is counted
  and the rest still locked, one line, no panic; switched off nothing is
  read or locked; the thread lowers itself before any lock, and a refused
  lowering is a line and the lock goes on. Not yet run on a lamp.

- **A slide closed during a held turn ended the voice as a fault.** On
  lamp-e82d on 4 Nov 11:44 the slide closed just after a question, while
  the turn was held (#4332). Relative to the run's end: input admitted at
  −1.44 s, the local endpoint at −0.774 s, `physical_mute` at −0.106 s,
  `privacy controller lease expired` about −0.046 s, `turn_finished` at
  −0.018 s, then `stop_delivery_error` for the privacy and second workers,
  `worker_exit_failed` for both, and the run ended as `physical mute
  observed…; verified cleanup failed`, which the restart guard counts. From
  the code: the mute returned at once with nothing revoked, and the
  cleanup's cancel and Denied publish came about 88 ms later, after the held
  turn's lease (the last kept frame + 100 ms) had lapsed at the workers;
  each ended on a lapsed lease that grants, as it must. The mute now
  cancels the reply, Denies the microphones and the camera and publishes a
  turnless snapshot first, before the camera is told no and before any
  record or teardown (`revoke_for_mute`); a failed publish there is
  recorded (`mute_revocation_unpublished`) and the run stays the mute.
  Nothing is extended and the lease lengths are unchanged. Unit tests: in
  the harness a mute right after the local endpoint of a held question,
  with the teardown held up 120 ms, is a clean mute, no peer holds a lapsed
  granting lease at any step, nothing listens after the mute, the speaker
  installs the Denied turnless authority within the teardown's first steps
  and before any Stop, and `turn_finished` `privacy_muted` comes before
  the first Stop (before: all four peers held the held turn's lapsed
  granting lease at every step of the held-up teardown); the mute's snapshot has no turn, grants nothing and is a
  full 250 ms lease, published before anything is recorded; a failed
  publish leaves the controller Denied and is recorded. Not yet tried on a
  lamp.

- **A wall clock stepped by chrony during start-up.** On lamp-06c1, in 4 of
  4 cold boots, chrony stepped the wall clock by about +20 s some 46 s in,
  and the voice then said "privacy publisher unavailable" (the step read as
  a 20 s silence of `controls/state`, `Notice::Late`) or "invalid Controls
  message discarded" (a state in flight across the step expired by its
  `at_unix_ms`). The bus judged lifespan and silence by the wall clock, and
  the voice repeated that check. Controls' state header now says its
  `clock`, the kernel boot id, and the bus and the voice judge such a
  header by `at_us`, CLOCK_MONOTONIC: no silence, no expiry from a step
  either way, a `latest` file of an earlier boot expired, and a Controls
  without `clock` (a mixed update round) read by the wall clock as before,
  with one journal line (`docs/bus.md`, "Which clock judges a message").
  It likely also ended a running voice with exit 1 in the field; that has
  no journal yet.

Not found yet: the power cut's first exit (exit 1 at 19.8 s, with no message
of its own) and the provider's "Connection refused" at 24.7 s; that boot's
journal is gone, and both need a second power cut with this build to see.

## A late SPI write to the ring (#911)

The ring's writer measures each SPI frame write against a 10 ms budget
(`lamp_ring::WRITE_BUDGET_US`). On lamp-4ace the SD/MMC host re-tunes about
once an hour and holds the bus for about 0.3 s; one write then returns after
11 to 15 ms, and until #911 that one late write latched the ring's fault and
left it dark until the voice restarted. A write that returns late did put its
frame on the strip, so it now counts as a late frame: the ring goes on, and the
voice's journal says it once ("an SPI write came back over its 10000 us
budget; the frame was kept"). Only writes that stay late for
`lamp_ring::LATE_RUN_US` (2 s) in a row are the fault they were before, with
the same blank and the same report. "In a row" means
back to back: an on-time write, or more than `lamp_ring::LATE_GAP_US` (100 ms)
with no write (a held cue or a dark ring), ends the run, so a late write after
such a pause is one late frame again.
