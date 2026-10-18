# One retry for an ended line lost before output (#705, Step B)

Host verification for `second/resend-guards`, a follow-up to #1434 at
`541fec889`, based on `lamp-voice` `0f49ed2c4`. The running gate checkout is
`154df7fae` plus these fixes; all nine changed Rust files are byte-identical
to this follow-up. The two parents differ only in the switch census. No lamp, cloud service, microphone or motor was used. The switch
remains off unless `LAMP_RESEND_LOST_LINE=on`; neither deployment unit sets it.
The final source hashes and exact commands are recorded in `gates.json`.

## Behavior and bounds

The provider worker keeps up to 15 seconds of 16 kHz PCM for the current
request, at most 480,000 audio bytes. After a recoverable connection failure,
it may resend an ended request once on the replacement session if that
session accepts it within five seconds of End. Any answer audio, model text,
output transcript or tool call disqualifies the request. A newer request,
cancel, mute or privacy change discards it. An open request still fails as
before because the coordinator abandons its input on a connection change.

The worker reuses the same request ID, preserves audio order and suppresses
the duplicate Started notification. The Gemini supervisor also enforces one
accepted resend per request across successive connection losses. Its single
request watermark is constant space; toggling the switch cannot restore a
consumed retry, and a newer request has its own retry.

The replay uses existing nonblocking queue operations. If its last audio
block fills the queue, End remains pending until capacity returns. The
latency target for this boundary is the next existing 2 ms worker tick after
space returns; capture and control do not wait for replay. This target is
verified with a controlled sink, not measured on a physical lamp.

The worker journal contains `line_resent` and `resend_refused` JSON records.
`line_resent` means the replacement accepted the retry, not that an answer
was received or heard. These records are not coordinator trace events.

## Regression evidence

- A peer-close recovery sends exactly one Start, audio block and End on the
  replacement, and receives one answer under the original request ID. With
  the switch off, it fails the request and sends nothing on the replacement.
- The actual Require cancellation barrier expires after 3.5 seconds. A mock
  replacement delays setup for 1.2 seconds. On a paused clock, the completed
  replay observation is at least 4.7 seconds and below five seconds after the
  original End. The original connection never commits that End.
- A two-slot sink accepts the last two audio blocks, then reports full.
  Repeated full ticks offer nothing; the first tick after capacity returns
  offers End once. Cancel and privacy-change cases discard the pending End.
- A retried request lost before output cannot start on a third connection,
  for all combinations of InputOpen and AwaitingResponse at the two losses.
  A newer request can still use its own single retry.
- Audio, model text, output transcription and tool-call events drained before
  the loss settle the request as Responding and prevent a resend. The worker
  tool case observes one timer call and no duplicate input on the replacement.
- The no-replay tests observe both the failure and the ready replacement,
  then service it for another 20 ms to catch late duplicate input or outcomes.
- Existing guards cover stale privacy, a newer line, the size and deadline
  bounds, non-recoverable failures and a refused replacement admission.

Two negative controls were run before the final upstream rebase. Removing
only the End-capacity guard makes its regression fail because the sink
reports a disconnected link. Removing only the supervisor watermark check
makes its regression fail because the twice-lost request is eligible again.
Both changes were restored before the full gate. `gates.json` identifies the
control baseline separately from the final gate source.

## V1 comparison and limits

The inspected V1 baseline is
`hal/drivers/voice/_internal/realtime_turn.py::_commit_turn_output`, with
`hal/test/test_capture_session_replay.py`: an uncommitted upload can be
replayed once, while a session loss during response consumption is not
replayed. This opt-in deliberately extends eligibility to an ended request
whose output has not been observed; it keeps the single-retry and no-repeat
tool/output safeguards. The [reuse decision](../../reuse-decisions.md#one-lost-spoken-line-retried-705-step-b-2026-10-18)
records that difference. There is no HAL runtime dependency or claim of
complete voice parity.

The complete affected-crate gate includes Gemini, voice, memory, skills,
surround and voice-eval, followed by a workspace build. Native ARM64
qualification is separate and must pass on the published PR head before the
change is ready for integration. Physical reconnect time, scheduling under
load, acoustic response latency and the benefit on a lamp remain unmeasured.
The switch must remain off until the named lamp check in `switches.md` passes.

## Final local gate

All commands ran from `lampOS/` with offline locked dependencies, one reused
build cache, six build jobs, incremental compilation off, and dev/test debug
info disabled. Local fixture sockets were permitted. No failure was retried
to obtain this result.

- `cargo fmt --all --check`: passed.
- `cargo clippy --locked --offline -p lamp-gemini -p lamp-memory -p lamp-skills -p lamp-surround -p lamp-voice -p lamp-voice-eval --all-targets -- -D warnings`: passed with warnings denied.
- `cargo test --locked --offline -p <crate> -- --test-threads=1`, for every
  affected crate and reverse dependency:

| Crate | Passed | Failed | Ignored |
|---|---:|---:|---:|
| lamp-gemini | 278 | 0 | 2 |
| lamp-voice | 1951 | 0 | 11 |
| lamp-memory | 208 | 0 | 5 |
| lamp-skills | 84 | 0 | 0 |
| lamp-surround | 260 | 0 | 0 |
| lamp-voice-eval | 536 | 0 | 0 |
| **Total** | **3317** | **0** | **18** |

- `cargo build --locked --offline --workspace`: passed.

[Machine-readable commands, results and source hashes](gates.json).
