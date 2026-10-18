# End-of-turn detection

Status: an opt-in pilot candidate, not a default, and never run on a Lamp.
Main owns activation, device measurement and every physical cohort. This branch
(`lamp-a2-pilot`, based on `lamp-v2-chat` `892ec0a1`) carries only the
conservative `--endpoint smart-turn` path. The pilot package, its manifest,
stress evidence and acceptance/rollback rule are in
[endpoint-pilot.md](endpoint-pilot.md).

Not on this branch:

- **The early end of input** (`--endpoint smart-turn-early`, MAIN-052) stays on
  `lamp-a2-continuation`. It was parked after Main's strict continuation probe
  failed with `BarrierTimeout`
  ([result](verification/strict-continuation-20261011/README.md)) and stays
  off. The later single `kept-start` successes do not qualify an early end.
- **The evaluation crate** (`lamp-endpoint-eval`: probe, replay, cohorts,
  profiling) also stays on `lamp-a2-continuation`. The numbers below were
  produced with it.

## On the lamp, 2026-10-14

Tried once on lamp-4ace with `--endpoint smart-turn` beside
`--end-silence-ms 600` (the quick profile: a verdict of "finished" ends
the turn at 400 ms, one of "clearly unfinished" holds it to 1.5 s, and
anything else is the 600 ms window).

- **It decided nothing.** Thirteen pauses in two played tests, thirteen
  decisions on the silence window: eleven because the verdict had not
  come when the window closed, two because the worker was still busy
  with the pause before. The lamp behaved as it does without it.
- **Why: the model is too slow where it ran.** The worker's own check
  (`lamp-turn-end --check-model`), on the lamp: 383 ms a verdict on the
  two fast cores (Cortex-A76, cpu6 and cpu7), 1,292 ms on the six slow
  ones (Cortex-A55). The worker is not pinned, runs at nice 10, and over
  the two tests used about 2.5 s of processor time for each request.
- **Even pinned to the fast cores it could not answer sooner as built:**
  the pause is put to it after 200 ms and its verdict would come 383 ms
  later, 17 ms before the window it is meant to beat. It could still hold
  a clearly unfinished turn open.

So it is off again. For it to make the lamp quicker, a verdict has to
take well under 100 ms on this board: a smaller or quantised model, a
faster runtime, or asking earlier than the pause.

The same day the second builder built the fast model: `lamp-turn-end
--fast`, the pinned model made 8-bit in memory and run by ONNX Runtime on
two threads on the fast cores. On the Linux ARM64 VM a verdict takes 30 ms
against 87 ms, with 3 of 1,167 held-out verdicts on the other side of 0.5;
about 130 ms on the lamp by the exact model's ratio, not measured there.
What was tried, the measurements, the cost of the native library and the
commands to run on the lamp are in [endpoint-pilot.md](endpoint-pilot.md),
"The fast model". Asking at 100 ms in place of 200 ms is measured there
too.

## Summary

- **Separate inference process.** The pinned Smart Turn v3.2 model runs only in
  `lamp-turn-end`. The interaction core links `lamp-turn-end-wire`, which
  has no model runtime. `lamp-voice` gains no third-party crate.
- **Unchanged behaviour without a usable model.** A missing executable or
  model, loading, a crash, a hang, a malformed answer or a busy worker leaves
  every pause on the existing 1,200 ms silence window. A test shows that the
  fallback ends every turn on exactly the block the baseline does.
- **Resumed speech keeps the turn.** Any speech-scored block ends a pause and
  makes its verdict obsolete. The utterance continues with its opening and
  resumed audio retained, and a late verdict can never end a later turn.
- **The parity profile** ends a confident complete verdict at 1,000 ms of
  pause, holds a clearly unfinished one to 1,500 ms and otherwise keeps
  1,200 ms. On 1,167 fresh held-out human clips (digital replay):
  - 77% of finished turns end 200 ms earlier;
  - it adds 6 cutoffs (0.5% of clips), none in 300 short-form English clips;
  - 3 finished turns wait 1,500 ms instead of 1,200 ms.
- **The gain is small.** At most 200 ms of the endpoint wait is removed. Main's
  runs measured 1.1–1.6 s from the local endpoint to the first ALSA write, in
  addition to the 1.2 s wait.
- **Audio-only endpointing cannot give 200–400 ms decisions without cutting
  people off.** At 400 ms, held-out cutoffs rise from 13 to 351.

## Architecture

| Crate | Linked by | Content |
| --- | --- | --- |
| `lamp-turn-end-wire` | `lamp-voice` (core) | Request and verdict types, the `Classifier` trait, the bounded frame format, the worker's serving loop, and the core-side supervisor `ProcessClassifier`. Depends only on `lamp-ipc`. |
| `lamp-turn-end` | the worker; lamp-eyes uses its ONNX Runtime pin and check for `--mouth-runtime` (#720) | Whisper features, pinned model loading with tract, the fast model (8-bit, ONNX Runtime loaded at run time with `--fast`), the installation self-check, `bench` and `agree`, and the `lamp-turn-end` binary. |

### Process boundary

- **Transport.** The worker's stdin and stdout are pipes carrying
  length-prefixed little-endian frames. Every frame kind has an exact size or
  an explicit bound, checked before allocation.
  - A request holds at most the last 8 s of 16 kHz PCM (256 KB).
  - An answer body is at most 523 bytes.
  - Stderr is inherited for diagnostics; the protocol never depends on it.
- **One request at a time.** `submit` refuses a second request as `Busy`.
  - Each request carries the caller's token (utterance, generation, check)
    and a request number.
  - Each worker process has an incarnation number.
  - An answer that does not match the outstanding request is a protocol
    fault.
- **Nothing waits on the caller's thread.** `submit`, `poll`, `availability`
  and `cancel` take one short lock. A writer thread copies the request into
  the pipe, a reader thread validates answers, and a supervisor thread owns
  the child process.
- **Privacy and cancellation.**
  - `cancel` drops queued audio and any unread answer. `close` does the same
    for an answer already on its way. No outcome is delivered after either.
  - The worker writes no files, and on Linux it cannot write a core file.
  - It zeroes its request and feature-extraction buffers after every answer.
    Freed allocations and the inference runtime's internal memory are not
    scrubbed.
- **Lifetime.**
  - On Linux the worker is killed if the core exits first
    (`PR_SET_PDEATHSIG`, after checking the parent PID).
  - Closing the client closes the worker's input and allows a 300 ms grace
    period. The supervisor thread then kills the worker and reaps it.
  - Runtime teardown waits within the shared shutdown bound (at least
    500 ms) for a confirmed reap. Without one, cleanup fails, so no later
    service cycle can start beside the old worker.
  - Losing the worker while running only means the silence fallback.
- **Identity.** `WorkerCommand` pins the model SHA256 the worker must report in
  `Ready`. The core refuses any other identity before sending a request.
- **Resource bounds (Linux).** Before it reads the model, the worker:
  - sets its core file limit to 0;
  - caps its address space at 256 MiB, 2.2 times the measured peak;
  - lowers its priority by 10 niceness steps.

  It never raises a limit the service manager already set lower. An
  allocation beyond the cap aborts the worker, which the core handles like
  any other worker failure.

### Failure handling

| Condition | Effect | Test |
| --- | --- | --- |
| Worker executable missing | `Failed` at once; every pause uses the baseline window | `missing_executable_fails_at_once` |
| Model missing, truncated, same-size corrupt or a different file | The worker refuses it before the ONNX parser sees it; `Failed`, never restarted | `worker_process_refuses_missing_or_wrong_models_without_blocking`, `worker_process_refuses_truncated_and_corrupt_models_once` (real binary), `missing_model_session_ends_on_the_unchanged_silence_fallback` (full session) |
| Not ready within 10 s, or a crash or protocol violation before ready | `Failed`, never restarted | `startup_failures_are_final` |
| `Ready` reports a model other than the command's pinned SHA256 | `Failed` before any request is sent; never restarted | `a_worker_reporting_another_model_is_refused_before_any_request` |
| Crash, malformed or unexpected answer, or inference error while serving | Killed if needed and reaped; the outstanding request returns an error to its own token; a replacement starts after a 1 s backoff, at most twice | `serving_failures_are_replaced_within_budget`, `protocol_violations_are_replaced`, `a_worker_failure_falls_back_through_recovery_and_then_resumes` (full session) |
| No answer within 2 s | Treated as hung: killed, reaped and replaced within the same budget. The detector has already fallen back after 500 ms | `hung_worker_never_delays_capture_endpoint_or_session_end` (full session) |
| Close during inference | The core returns at once; the worker is killed after the grace period | `close_during_inference_never_waits` |
| Cancel with an answer in flight | The answer is discarded; the slot frees when it arrives | `cancelled_answers_are_never_delivered` |
| Close with an answer on its way | Nothing is delivered after close; the answer counts as discarded or lost | `no_outcome_is_delivered_after_close_even_for_an_answer_on_its_way` |
| Worker exit not confirmed by a native wait, or not within the teardown bound | No replacement starts; runtime cleanup fails, so a physical mute is not restartable | `unconfirmed_endpoint_exit_fails_mute_cleanup_and_never_reopens_service`, with the control `confirmed_endpoint_exit_keeps_the_restartable_mute` |
| Long idle | The worker stays ready with the same process; only a request in flight is timed | `an_idle_worker_stays_ready_across_a_long_quiet`, `worker_process_stays_ready_and_bounded_across_idle_and_repeated_checks` |

The process tests (`crates/turn-end-wire/tests/process.rs`) re-execute their
own binary as a scripted worker. Every path above runs through a real child
process and real pipes.

The six in-process classifier scenarios explicitly use the stepped peer
clock described in [Voice fixtures](voice-test-fixtures.md), including
resumed speech, delayed verdicts, recovery and answer ordering. Their original
endpoint and audio assertions remain; real process-classifier cases keep
their host clocks. Scenario timestamps do not measure lamp or host latency.

The finite real-worker ordering fixture explicitly selects a test-only
100,000 us privacy allowance and a 12-second real conversation watchdog for
missing-model, hung-worker, loading-worker and duplex missing-model cases
(also the existing ignored real-model provenance case). The allowance is
capped at the existing IPC ceiling. The six stepped cases and all three
mute/cleanup scenarios keep the default 50,000 us gate. Normal builds,
source/channel freshness, reopening and actual classifier deadlines are
unchanged. A host-clock guard runs before real child construction. No Ready
barrier hides the intentionally missing or still-loading worker. Privacy
producer diagnostics remain distinct from unmeasured coordinator consumption
age; these fixtures do not qualify the lamp's real privacy margin.
The synthetic capture source retains its scheduled 10 ms acquisition times
when a delivery thread is delayed. Fresh transport envelopes do not refresh
old audio; controlled-delay tests retain the 100 ms source-age refusal.

Endpoint authority never depends on trace metadata. Decisions come only from
detector state and the client's in-memory availability; the trace events below
are records written afterwards.

## Decision policy (`crates/voice/src/activity.rs`)

`TurnDetector::v1_baseline().with_confirmation(classifier, ConfirmPolicy::default())`

1. **Pauses.** A pause is a run of consecutive blocks scoring below 0.35,
   counted exactly as the baseline counts its 1,200 ms window. Any
   speech-scored block, including a breath or a click, ends the pause and makes
   its verdict obsolete.
2. **One request per pause.** After 200 ms of pause the detector submits the
   utterance's latest 8 s: pre-roll, opening and any resumed speech, in order.
   A busy slot is retried each block. The request always ends 200 ms into the
   pause, because the model's verdict drifts as trailing silence grows.
3. **Three bands** (defaults in parentheses):
   - complete (probability ≥ 0.9): the turn ends at 1,000 ms of pause, or on
     the verdict's arrival if later (`model_complete`);
   - clearly unfinished (< 0.01): held until 1,500 ms (`model_incomplete`);
   - anything between: ends at 1,200 ms (`model_uncertain`).
4. **No usable verdict** (missing, loading, failed, recovering, closed, busy,
   error, or no answer within 500 ms): ends at 1,200 ms (`silence_fallback`,
   with the reason).
5. **Invalidation.** These make pending verdicts obsolete and cancel the
   client's queued audio:
   - a new utterance;
   - `clear_prospective` or `reset`;
   - a capture fault;
   - a speech-scored block.

`ConfirmPolicy::responsive()` ends a complete verdict at 400 ms. It exists for
evaluation only and is not reachable from any command line.

Each `Activity::End` records an `EndpointDecision`:

- `basis`, `silence_ms`, `probability` and `fallback`;
- `verdict`: the worker incarnation and request number, the pause length at
  submission and on arrival, and the snapshot, delivery, features, inference,
  return and round-trip costs.

## Coordinator integration

- `--endpoint smart-turn` requires `--turn-taking v1-baseline`. `silence` stays
  the default; without the flag no worker is started.
- The worker and model are found beside the `lamp-voice` executable
  (`lamp-turn-end`, `smart-turn-v3.2-gpu.onnx`).
- The client starts at the beginning of `spawn_rig`, so loading overlaps the
  other workers' startup.
- Traces:
  - `endpoint_policy`: the policy, worker command and model hash;
  - `endpoint_worker`: each availability change, with the worker report;
  - `local_endpoint.endpoint`: the decision and its verdict provenance;
    `includes_silence_wait_ms` is the actual pause;
  - `endpoint_worker_final`: counters, and whether the worker was reaped.

## Evidence

The cohorts are kept separate. Every result below is a digital replay on a
development host; no audio passed through Lamp microphones or a loudspeaker.

| Cohort | Status |
| --- | --- |
| Digital model-reference parity | Features within 1.2e-6 of Pipecat, probabilities within 8.7e-6 of ONNX Runtime on 829 clips; rerun through the worker process |
| Offline replay of recorded human clips | This section |
| Offline replay of synthetic `say` speech | This section |
| Synthetic loudspeaker replay, recorded-human loudspeaker replay, direct human at the Lamp | Not run; Main schedules physical cohorts |

### Recorded human clips

- **Source.** `pipecat-ai/smart-turn-data-v3.2-test`, revision
  `dfc179241fee65b0efe81ca37786f41ce26f38ec`, CC-BY-4.0. Shards were
  downloaded with SHA256 checks and decoded locally; no audio is committed.
- **Clips.** Only human clips (English and Spanish), each labelled finished or
  unfinished at its end. Each clip is replayed with 600 ms of lead-in, a
  2.5 s tail and a −63 dBFS noise floor.
- **What is measured.**
  - A split is an endpoint followed by a new speech onset in the same clip:
    the person continued.
  - The delay is the detector's pause length at the final endpoint.
  - All counts are against the unchanged baseline on the same audio.
- **Partitions.**
  - Development: shards 0 and 1 (1,180 clips). Every choice was made here.
  - Fresh held-out: shards 4 and 5 (1,167 clips), probed only after the
    profiles were frozen.
  - Shards 2 and 3 were held out from an earlier selection whose pause
    counting did not match the detector. That selection was discarded, so
    they are not reported as held-out evidence.
- **Method.** All reported numbers come from the real `TurnDetector` with the
  model in process and each verdict released 130 ms after submission.

Fresh held-out, real detector, 130 ms verdict latency:

| Profile | Splits (added) | Clips with an added split | Finished turns: mean delay | Ended before 1,200 ms | Waited longer | Unfinished ended early |
| --- | --- | --- | --- | --- | --- | --- |
| baseline (1,200 ms) | 13 | — | 1,200 ms | 0% | 0 | 0 / 569 |
| parity (default) | 19 (+6) | 6 | 1,047 ms | 77% | 3 | 19 / 569 |
| strict (≥ 0.99 at 150 ms, ends at 800 ms) | 15 (+2) | 2 | 1,136 ms | 16% | 0 | 2 / 569 |
| responsive (400 ms) | 351 (+338) | 271 | 583 ms | 77% | 4 | 23 / 569 |
| MAIN-043 candidate | 447 (+434) | 333 | 545 ms | 82% | 0 | 30 / 569 |

Development, same method:

- baseline: 20 splits;
- parity: 21 splits (+4, −3), with 76% of finished turns ending earlier and a
  mean delay of 1,049 ms;
- responsive: 408 added splits.

By source on fresh held-out data, parity versus baseline:

| Source | Clips | Added splits | Finished turns ending earlier |
| --- | --- | --- | --- |
| Short-form English (`human_5`, `midcentury_1`, `human_convcollector_1`) | 300 | 0 | 75–90% |
| Spanish (`mundo_1`) | 91 | 1 | 80% |
| Long-form English (`liva_1`, 12–16 s recordings) | 776 | 5 | 73% |

The six added held-out splits are reviewed case by case in
[endpoint-pilot.md](endpoint-pilot.md#review-of-the-six-added-held-out-splits).

**Why earlier commits cut people off.**

- At internal pauses of 0.8 s or longer, after which the person kept speaking,
  the model scores 43% as complete 200 ms in.
- At true turn ends it scores 86% complete, with a median of 0.98. At
  unfinished clip ends it scores only 5% complete.
- A pause after a finished sentence looks like a finished turn in the audio.
  On the development data, neither pause history, utterance length nor the
  early score trajectory separated the two cases.

### End-to-end turn model

The evaluation crate also applies the coordinator and provider seam as a model:
the first reply audio is ready a fixed time L after the end of input. On fresh
held-out clips (598 finished turns), L = 1.1 s as in Main's measurements:

| Profile | Whole thoughts if Gemini settles an interrupted request | if it never settles | Audible splits | Clips taking the interruption barrier | Speech end to first audio, mean / p50 |
| --- | --- | --- | --- | --- | --- |
| baseline | 592 | 586 | 4 | 9 | 2,300 / 2,300 ms |
| parity | 594 | 585 | 3 | 16 | 2,147 / 2,100 ms |

The interruption barrier is the path a person takes when they resume after
the endpoint but before the reply is audible.
- **History.** With an immediate successor Start, Main's strict
  continuation probe failed on this path with `BarrierTimeout` (11 October).
- **Since then.** With Agent 1's retained Start (`kept-start`, on
  `675c71c62`), one service-only probe settled and answered a before-output
  continuation. One native serve run superseded a pending turn before output
  with its correction. Physical continuation evidence remains limited to
  such single cases.

Parity takes this path in 7 more clips than the baseline; [endpoint-pilot.md](endpoint-pilot.md) makes that the first
rollback trigger.

### Synthetic `say` cohort

`crates/endpoint-eval/endpoint-cases.json` on `lamp-a2-continuation` holds 55
spliced macOS `say` cases, 50 of them single-turn. Their final
intonation is artificial, so they remain a stress test, not a tuning target.

| Profile | False splits | Never ended | Fragment cut by the model | Delay p50 / p90 |
| --- | --- | --- | --- | --- |
| baseline | 7 | 2 | 0 | 1,196 / 1,586 ms |
| parity | 6 (adds `planet-slnc-1200`; avoids `filler-1200`, `filler-slnc-1200`) | 2 | 1 | 1,192 / 1,519 ms |

"Never ended" means noise blips kept resetting the baseline's window within the
3 s tail. The fallback behaves identically.

## Latency and resources

Neither environment is Lamp hardware, and neither predicts the A733:

- **macOS:** M2 Max, Rust 1.96.0. tract uses Apple AMX here.
- **Linux:** an ARM64 Colima VM with 6 vCPUs on the same host, running Docker
  `rust:1.96.0-bookworm`. NEON only.

Through `ProcessClassifier` (5 cold starts, then 100 requests each for 1, 4
and 8 s windows of one recorded human clip):

| Measurement | macOS | Linux ARM64 VM |
| --- | --- | --- |
| Spawn to ready, observed by the core | 164–165 ms (the first launch of a new binary took 1,022 ms) | 241–257 ms |
| Model load (hash check included) / cold inference | 125–157 ms / 35–36 ms | 148–158 ms / 91–98 ms |
| Submit call on the core's thread, 8 s window, p50 / max | 67 / 346 µs | 72 / 148 µs |
| Features / inference, p50 | 2.8 / 34.6 ms | 3.0 / 91.2 ms |
| Round trip, p50 / p90 / max | 37.6 / 40.9 / 44.3 ms | 94.5 / 97.2 / 100.9 ms |
| Worker CPU per verdict | about 37 ms (one thread) | about 95 ms (one thread) |
| Worker memory | 169–191 MB resident after ready; 177 MB after 300 requests | 59 MB resident, 102.5 MB peak; unchanged after 300 requests |

The installation self-check (`--check-model`) and the pilot's bounded worker
are measured in [endpoint-pilot.md](endpoint-pilot.md#measured-bounds).

- **Core memory.** The core's added memory is the client: three threads per
  worker and at most one 256 KB request frame.
- **Feature reuse.** Feature extraction is 3% of each verdict's cost, and the
  parity profile does not act until 1,000 ms, so neither feature reuse nor
  streaming classification is implemented.

## Installation

```sh
cargo build --locked --release -p lamp-voice -p lamp-turn-end
curl -fsSL --max-filesize 33000000 -o target/release/smart-turn-v3.2-gpu.onnx \
  https://huggingface.co/pipecat-ai/smart-turn-v3/resolve/f766f81d3cfdf7737ac64aad813d91bbfd56bf93/smart-turn-v3.2-gpu.onnx
target/release/lamp-turn-end --check-model target/release/smart-turn-v3.2-gpu.onnx
```

- `--check-model` loads the file under the worker's own bounds and verifies
  its size (32,411,198 bytes) and SHA256 before parsing.
- It then classifies the `voiced_9s` reference signal ten times. Each result
  must match ONNX Runtime's probability within 1e-4.
- It prints one JSON line of costs and exits 0, or names the problem on
  stderr and exits 2.
- Delete the file if it reports a mismatch. The worker refuses any other
  bytes, and the core then keeps the silence window.

Model tests are ignored by default. With the model under `.cache/endpoint/` or
`LAMP_SMART_TURN_MODEL` set:

```sh
cargo test --locked --release -p lamp-turn-end -- --include-ignored
LAMP_ENDPOINT_WORKER=target/release/lamp-turn-end \
  cargo test --locked -p lamp-voice --lib endpoint_session -- --include-ignored
```

`LAMP_ENDPOINT_IDLE_SECS` sets the quiet period of the two idle tests.

The fast model (`--fast`) also needs the pinned ONNX Runtime library beside
the worker: `libonnxruntime.so.1.30.0` from Microsoft's
`onnxruntime-linux-aarch64-1.30.0.tgz` (25,135,496 bytes, SHA-256
`64e903a4…a4b`; on macOS `libonnxruntime.1.30.0.dylib` from the
`osx-arm64` archive, for development). Its tests (`tests/fast.rs`) read it
from `.cache/endpoint/` or `LAMP_ONNX_RUNTIME`:

```sh
cargo test --locked --release -p lamp-turn-end --test fast -- --include-ignored
target/release/lamp-turn-end --check-model target/release/smart-turn-v3.2-gpu.onnx --fast
```

## Limitations

1. **No device evidence.** Startup, inference and memory on the A733 are
   unmeasured; the host and VM numbers above do not predict them. A slower
   verdict does not change the parity profile's 1,000 ms commit unless it
   exceeds the 500 ms result timeout.
2. **Digital replay only.** None of the human clips passed through the Jieli
   voice board or a loudspeaker. Physical cohorts may score differently.
3. **Labels and context.**
   - The upstream labels describe only each clip's end, and there are no
     transcripts.
   - The long-form recordings' conversational context is unknown.
   - Only English and Spanish are covered.
4. **Small gain.** The parity profile saves at most 200 ms of the endpoint
   wait. Larger gains need a continuation path that lets a person keep
   speaking after an early endpoint. That path depends on provider
   settlement. It failed with an immediate Start and has single successful
   `kept-start` cases, not a qualified rate.
5. **Memory and size.** The worker is a separate 24 MB binary with its own
   resident memory. The fast model adds ONNX Runtime, a 25 MB native library
   loaded only by the worker, and doubles the worker's peak address space
   (243.5 against 114 MiB); see [endpoint-pilot.md](endpoint-pilot.md), "The
   fast model". tract's own 8-bit path is not used: built in a Debian
   bookworm container it lacks its fast kernel.
6. **Endpointing only.** This does not identify an addressee and is not an
   interruption gate.
