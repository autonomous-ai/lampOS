# Reminder reset and skill speech result

Native Mac validation, Rust 1.96.0, 25 October 2026, 23:23–23:40 +07.
The gate used one compiler job, a fresh owned Cargo target, disabled
incremental compilation, offline/locked dependencies and serial tests.

## Behavior and reproduced failure

A playback-clock reset invalidates the reminder receipt without necessarily
adding a playback gap. Previously the whole-reply join could abandon that
receipt while the skill's say result reported `spoken`. The say now also
requires the local `Complete` settlement already recorded by the join.
It reports `dropped` with `reminder_completion_unconfirmed` otherwise.
This local result does not claim that the worker has durably stored or
acknowledged the completion.

The regression attaches an actual begun say ticket, retires the accepted
audio, sends the clock reset through speaker control, and checks both the
reminder settlement and the ticket. On unchanged production at `56cacbddd`,
with only the candidate test edits applied, it reaches the Abandon assertion
and then fails at the paired ticket result:

```text
left: Ok(Done(Spoken))
right: Ok(Done(Dropped("reminder_completion_unconfirmed")))
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 3781 filtered out
```

That command exited 101 because of the expected assertion, not a compile
failure. No failed observation was replaced or repeated.

## Commands and results

From `lampOS/`, with `CARGO_BUILD_JOBS=1`, `CARGO_INCREMENTAL=0` and an
exclusive `CARGO_TARGET_DIR`:

```sh
cargo +1.96.0 test --offline --locked -p lamp-voice --lib coordinator::tests::reminders::reset_or_playback_gap_cannot_be_relabeled_whole_reminder_completion -- --exact --test-threads=1
cargo +1.96.0 fmt --all --check
cargo +1.96.0 clippy --offline --locked -p lamp-voice --all-targets -- -D warnings
cargo +1.96.0 test --offline --locked -p lamp-voice -- --test-threads=1
```

The first command is the unchanged-production reproduction above. After
restoring the candidate production bytes, formatting and strict Clippy pass.
The full changed-crate test command exits zero:

| Harness | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| Library | 3,767 | 0 | 15 |
| Main | 1 | 0 | 0 |
| Integration | 275 | 0 | 0 |
| Release pair | 0 | 0 | 6 |
| Render-line example | 4 | 0 | 0 |
| Stop-word replay example | 6 | 0 | 0 |
| Documentation | 0 | 0 | 0 |
| **Total** | **4,053** | **0** | **21** |

The full run includes clean two-generation completion with partial-acceptance
and retirement holds, reset/gap rejection, failed or silent provider output,
privacy revocation after partial playback, and ordinary say/sound controls.
All 7,074 source inventory entries remain fixed within each gate phase.
No new switch, wire field or enum value is added. The changed-name grep finds
only Voice; there is no changed external API reader.

## Current-base carry and limits

The full execution above remains on `56cacbddd`. The carry to `e1c69457a`
preserves the complete authored patch byte-for-byte. That base delta changes
only Body's private supervisor test and its explanation; the test file is
absent from both actual Body dependency records used by the Voice gate.
Voice and its compiled dependency inputs remain unchanged. Fresh formatting
and strict Voice Clippy qualify the carry; it is not another full execution.

Third's public reminder-admission source at `ef1637d7d` was checked:
`heard_whole` does not re-read pre-completion receipt eligibility. Production
patch composition passes; the shared schedule-doc insertion must retain both
explanations. This source check is not a combined execution.

The new read is synchronous in the existing completion path, with no I/O,
timer or additional wait. Existing playback, owner and microphone checks
remain. These synthetic PCM/cursor fixtures establish the paired outcomes,
not acoustic fidelity, hardware latency, a person's hearing or durable Ack.
No workspace build, local Linux ARM64 gate or device action ran for this fix;
the PR's original ARM64 and Linux checks provide its native CI qualification.
