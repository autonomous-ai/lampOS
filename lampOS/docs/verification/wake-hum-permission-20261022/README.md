# A wake waits for permission before its hm

Part of #2487; unblocks Fifth's #2857 and draft #2948. Original base `fe49ed3c4`; full local gate on `e2232b836`. The task files
were byte-for-byte unchanged by that rebase.

## Failure and change

Fifth's lamp-52e6 run at `172f9c94c` (22 Oct 13:18–13:21) unmuted on a
wake tap, but recorded `head_wake_cue` refused as `microphone_not_allowed`.
The old coordinator made that denial terminal even though normal privacy
reopening requires 60 ms of stable observations. Its 1.5 s limit bounded
retries only; a newly successful decision after the deadline could play.

The pending wake now waits for microphone permission, quiet and listening
readiness, without moving its original conversation start. An expiry check
runs before readiness and rejects a cue at or after 1.5 s. Physical mute,
renewed hand sleep or an error still ends the conversation, discarding its
pending cue. A consumed wake still pending at that exit now records one
`head_wake_cue` refusal with `why: conversation_ended`, through the same
bounded recorder as other decisions. An accepted final record request preserves
the original result, including a physical-mute restart token; recorder
failure is reported and cannot retain that token.
The wake marker's separate two-second freshness and all existing speech-first,
reply-owed, permission and readiness gates remain in force. This changes
only the existing head-touch path; it enables no product switch.

The two bounds are separate: the marker can already be almost 2 s old
when the conversation consumes it, and the cue then has the original
conversation's 1.5 s window. The code therefore permits a decision to play
almost 3.5 s after the hand. This pre-existing limit is not the latency
target: wake acquisition to first write must still be measured against
500 ms on the lamp. This follow-up does not change either bound.

## Regressions and boundaries

The stepped coordinator fixture begins with Unknown permission and uses the
real privacy gate. Its capture peer waits for Allowed authority and
StartCapture, then sends fresh silent frames with that microphone generation.
The provider data socket is read, so no-provider assertions inspect commands
rather than inferring them from a trace label. Other fixtures do not opt in.

Cases: one hm after startup stabilization and fresh capture; physical closure
at 0, 40 and 100 ms; renewed sleep at 30 ms; first capture available 20 ms before,
at and 20 ms after expiry. Pure readiness cases one microsecond before, exactly
at and one microsecond after independently pin the exclusive 1.5 s bound. The existing marker freshness and once-only tests remain.

The review follow-up also checks a pending wake's refused decision on
physical closure, renewed hand sleep and a privacy-worker fault at 20 ms.
The original error is retained. A separate scheduling-fault scenario runs
one pass with Unknown permission, pauses from 2 ms to 1.5 s before the
first fresh Open observation, and checks expiry while permission is still
Unknown. Permission later stabilizes and fresh capture starts without
reviving the cue. This covers unavailable permission beyond the wake
deadline, not continuously missing privacy readings: the existing one-second
startup-unavailable grace still applies to ordinary running passes.

The fixture's latency boundary is wake acquisition to the first speaker socket
send, on its stepped scenario clock. It is not device or acoustic latency.
The lamp target remains wake acquisition to `head_wake_cue_first_write` within
500 ms. Host tests cannot prove the person's first words were retained.

The corrected startup scenario measured permission and fresh capture at
60 ms, the cue decision at 62 ms and the first speaker socket send at 64 ms.
These are stepped-clock fixture times. Removing the coordinator's permission
retry failed the startup regression with `microphone_not_allowed`; making the
deadline inclusive failed the exact-boundary test. Restoring the source passed
all 18 wake-filtered tests.

The initial sandbox run could not bind Unix sockets; the unrestricted rerun
then exposed two fixture omissions: its capture-start report and an assertion
that assumed a single two-millisecond lifecycle pass. The fixture now sends
the real start report. Integration cases cover capture near/beyond expiry;
pure gate cases pin the exact microsecond. Production checks were not relaxed.

## Gate

The full local implementation gate on `e2232b836` passed, from `lampOS/`
on the x86_64 Mac, with `CARGO_BUILD_JOBS=2`:

```sh
cargo fmt --all --check
cargo clippy -q --offline --locked --workspace --all-targets -- -D warnings
cargo test -q --offline --locked --no-fail-fast -p lamp-body -p lamp-cloud -p lamp-expression -p lamp-keeper -p lamp-lean -p lamp-memory -p lamp-peek -p lamp-setup -p lamp-skills -p lamp-update -p lamp-voice -p lamp-voice-eval -- --test-threads=2
cargo build -q --offline --locked --workspace
```

Tests: **6,728 passed, zero failed, 29 ignored**. The updater's 77-test and
106-test recovery groups completed normally (439 s and 496 s); the whole
test command took 2,482 s including compilation. The workspace build passed.
The sandbox initially blocked socket fixtures; the gates above ran with local
socket/process access and no device or external service.

The final rebase onto `c9df78414` preserves upstream's arrival/skill changes.
The production patch and test-only wake setter are identical after ignoring
hunk line offsets. Final test setup also consumes provider Ready through its
control socket before entering every wake scenario and checks its trace
precedes the cue. This closes the review's no-provider coverage gap; older
fixtures remain opted out. On this final base, formatting and strict
workspace Clippy passed again. The final socket-enabled checks were:

```sh
CARGO_BUILD_JOBS=2 cargo test -q --offline --locked -p lamp-voice --lib hand_controls -- --test-threads=1 --nocapture
CARGO_BUILD_JOBS=2 cargo test -q --offline --locked -p lamp-voice --lib wake -- --test-threads=1 --nocapture
```

Hand controls: **101 passed, zero failed, three ignored**. Wake: **18 passed,
zero failed or ignored**. Provider Ready is consumed before the cue, no
provider input is sent, and the same 60/60/62/64 ms fixture timings hold.
The review's coverage finding is closed. These repeats are not added to the
6,728-test full-gate total. The workspace build also passed again on `c9df78414`. Native ARM64/Clippy
receipts follow in the PR body. No ARM64 linking runs on the Mac.

## Review follow-up on 22 October

On `2297f0f3d` plus the four-file review correction, the same full
12-crate test command above passed **6,760 tests, zero failed, 29 ignored**.
Strict workspace Clippy and the workspace build passed too. The focused
hand-controls run passed **103 tests, zero failed, three ignored**, with
the same 60/60/62/64 ms scenario timings. These counts are separate from
the earlier gate, not added to it.

Formatting on that base failed only at the inherited `own_echo.rs` test
call fixed by #3044. After the full run finished, the branch was rebased
onto `0766811ad`, which contains that fix. The original commit's
range-diff is equal; the four-file review delta is identical after ignoring
diff index hashes and hunk offsets. Upstream's body-push cancellation and
skill journal changes in the coordinator are preserved. Formatting now
passes. On that rebased source, strict workspace Clippy and the workspace
build passed again, as did **103 hand-controls tests / zero failed / three
ignored** and **20 wake tests / zero failed or ignored**, using the same
focused commands above. The synthetic 60/60/62/64 ms timings are unchanged.
Exact-head native results are reported in the PR and the builder log.
No ARM64 linking runs on the Mac.

## Lamp handoff

Fifth owns lamp-52e6 and the hand try, using one coherent build and #2948's
product units on the second voice route. Verify the running flags before the
try. With the body standing and someone at the arm: tap; a 3 s hold
with the breath, sleep and fold; tap to wake with the hm; a quiet tap's hm;
kill while listening, then one tap opens it; and 20 s with hands away. Keep
the original quiet-tap rarity rule when spacing the gestures. Record the
actual build/flags, acquisition/decision/first-write times, perceived sleep
and wake, and whether the first spoken words are retained. Observe cleanup
and the lamp lease; Third accesses no device. #2948 stays draft until that
required hand try passes. No lamp result is claimed here.
