# Two-face fixture repair preparation

This fixture repair follows diagnostic commit
`eaac115773fa18687321e7983e231deee6d304cc` (#3955), train-delivered as
`a9a97969e`. The diagnostic PR and its receipts are preserved. The repair PR
targets `lamp-voice` and excludes that delivered diagnostic commit. The
initial eaac-based qualification and later 9bde integration qualification
below are separate frozen host runs, each executing the six controls once.
Native repair gates remain separate and have not run yet.

Original #3544 ARM40588908587 remains 6,466 PASS / three FAIL / 1,336 selected
unrun. The later diagnostic's host/native passes do not explain or waive that
failure. Its host privacy report showed a fresh header rejected at an old
receive epoch, leaving an empty deque; it did not establish fresh-single-look
admission. Its native successful output contained no five-scenario JSON data,
so no host timing/epoch receipts are relabelled as native measurements.

## Preserved route contract

The existing five-scenario function retains its array and exact
`(room, held)` assertions. It now seeds historical `FacesHeld::look`
admissions and queries in the existing explicit TLS logical-clock domain.
There is no asynchronous reader in this deterministic layer. Each historical
look is admitted at its own timestamp; the latest is 100 ms before the verdict,
with 300 ms spacing. The unchanged admission and persistence methods are used.

The real latest bus fixture is explicitly **non-expiring only in this
deterministic layer**. Wall expiry is not claimed here; a distinct actual
expiry control below exercises it. No production clock API or global clock
is added, and no runtime threshold, retention, gap, privacy or default-off
policy changes.

| Scenario | Required deterministic outcome |
| --- | --- |
| Eight two-face looks, held on | Count 8/span 2.1 s/newest 2: Busy and the exact face-alone refusal. |
| Counts 1/1/1/2, held on | One stray two-face sample: Unknown/allowed, despite latest 2. |
| Counts 2/2/2/2/2/2/1, held on | Earlier six already span 1.5 s; newest 1 must keep Unknown/allowed. |
| Privacy close/reopen, then one fresh look | Old positive history must clear. Advance the existing scope 2 ms, admit a timestamp strictly after cutoff using current epoch 2, require deque length 1 and that exact new look, then Unknown/allowed. Empty/rejected cannot satisfy this case. |
| Held off, one two-face look | `faces_held=None`, no held reader construction: immediate Busy and exact face-alone refusal. |

The two existing pure persistence/freshness and privacy/late-input/clock-step
controls retain their complete function bytes.

## Separate real-bus boundaries

Two new controls use the actual asynchronous reader, actual clocks and an
ordinary 2-second publisher TTL. They activate a per-observer test-only phase
gate **before spawning**, captured once into the reader. The gate waits after
the existing held-state mutex has been released and before `recv`. It holds
no Heeding/FacesHeld reference, adds no held-state lock or read, and changes
nothing when not explicitly enabled.

One-slot notice/command/end channels and a shared 10-second host deadline
bound the gate. A notice identifies a turn and the actual sampled receive
epoch; the matching release permits only that turn. A mismatched release,
full/disconnected channel, poison or watchdog is a setup failure. Controller
Drop disconnects without waiting or locking, including assertion unwind.
The gate holds no strong reference to Heeding or FacesHeld.

The fresh-admission control drops Heeding first. The cross-epoch control
calls Stop and observes the loop-exit ACK **while Heeding remains alive**,
then checks pathname removal before dropping Heeding. These exercise distinct
cleanup orderings. The ACK is a loop-exit receipt, not an OS-thread join or a
closed-FD claim. Explicit unwind execution is not one of these six controls;
disconnect/unwind release is source-reviewed, not runtime demonstrated.

`Bus::subscribers` is not an unlink proof: it suppresses directory/entry/type
errors. The controls instead strictly walk only the fresh private `bus.root()`
with fallible directory/entry/type operations, no symlink following, at most
256 entries and depth 16. Heeding first creates its NameHeard socket, so the
baseline is captured before starting the face reader. After the first gate
notice, require no baseline removal and exactly one new socket; retain that
actual DirEntry pathname and its parent, both with device/inode identity.
After loop exit, require that exact path to return **NotFound only**, while
the same parent remains a directory. Other I/O errors, type/identity changes
or timeout fail. The observer never unlinks the path to make the check pass.
This proves the captured own pathname is absent, not that a thread was joined
or all descriptors were independently inspected. The outer process watchdog
owns final process-group teardown. New private roots are created explicitly
with Unix mode 0700 and refuse a pre-existing path.

- **Fresh current epoch:** first prove real parsed ingestion/admission. Pause
  at the next old-epoch boundary, clear privacy twice, and complete one normal
  empty receive. Require the next actual boundary to sample epoch 2 before
  publishing. The new actual header must be fresh and the actual parsed
  receipt must carry epoch 2. Require its actual admission, exact single-look
  deque and insufficient persistence. Queue-delivery counts alone never count
  as ingestion.
- **Cross-epoch refusal:** pause at sampled epoch 0, then clear privacy twice
  and publish a fresh header strictly after cutoff. Release that recorded old
  wait. Require actual parsed receipt epoch 0/current2, fresh wall and monotonic
  inputs, actual privacy rejection, empty deque and no persistence. This
  assertion is distinct from the deterministic fresh-single-look case.

Both controls correlate the publication's monotonic argument to the actual
header and branch receipts. Only valid parsed messages appear in that log;
the existing parse-failure observability gap remains explicit. Logs remain
bounded at 64 events, and success requires zero dropped receipts. Assertions
emit finite phase data on unwind too. No frame/audio/key/person data is kept.

The third new control publishes TTL 0 on the real bus. It independently
computes the header's wall expiry plus existing 50 ms grace and 1 ms strict
boundary, waits for that wall instant under a real 10-second watchdog, and
checks the actual retained header's `expired=true`. Both existing turned/count
readers must return None. A detected backward wall sample or real watchdog failure is reported as
clock-step/setup failure, with no retry or host clock change. Each wall sample
is bracketed by host Instant reads for observation only: a wide bracket is
uncertainty, and no wall-versus-host drift classifier is used. In particular,
a scheduling pause between clock reads cannot itself become a drift assertion.
The packet lifetime is not extended to make a test pass.

## Initial eaac-based host six-case gate

After exact source review and an explicit bank allocation, formatting, metadata
without dependency expansion, strict Voice/all-target Clippy and one current
Voice-library compile passed. Actual opaque package IDs/features/profile, raw
dep-info, 297 source inputs and an independent saved driver were bound. All
six exact names were discovered before the independent admission audit and
execution:

- `second_provider::worker::tests::with_two_faces_held_looks_between_lines_make_the_first_line_busy`
- `second_provider::worker::tests::two_faces_reader_admits_a_fresh_current_epoch_look_from_the_real_bus`
- `second_provider::worker::tests::two_faces_reader_rejects_a_post_privacy_look_taken_across_the_old_wait`
- `second_provider::worker::tests::two_faces_latest_expiry_refuses_turned_and_count_on_the_real_bus`
- `second_provider::worker::tests::with_two_faces_held_two_faces_must_stay`
- `second_provider::worker::tests::with_two_faces_held_privacy_late_looks_and_clock_steps_leave_no_busy_room`

Each ran once in a separate process with one thread, zero retries and a
30-second / TERM 2-second / KILL 2-second outer watchdog. All six passed;
failed, ignored, selected-unrun and unexpected-panic counts were zero. The
independent final audit passed. All 13 process groups were reaped and absent,
the owner lock was released, and all 195 prior saved copies plus the new
current driver were unchanged through the final guards. No normal product
child, full-suite, physical, provider, release or original-failure clearance
is inferred.

The first strict-Clippy attempt stopped on an unused test-only `Scope::report`
method after replacement of its sole timed-fixture caller. Its exit 101,
raw diagnostics and zero-runtime boundary are preserved. Only that unused
method was removed, without lint suppression; all six function bodies and the
normal production token projection remained equal to the reviewed source.
The corrected-source gate completed in 544.033 seconds: fmt 14.099 seconds,
metadata 0.080 seconds, strict Voice 102.200 seconds and library compile
176.600 seconds. The six process wall times were 0.074277, 1.054132, 0.053160,
0.123191, 0.035719 and 0.041928 seconds in the order listed above. These are
host fixture measurements, not device response latency.

The five model reports showed Busy/Unknown/Unknown/Unknown/Busy. The privacy
row genuinely admitted one current-epoch look strictly after its cutoff.
The real fresh-admission control recorded receive epoch 2 and deque length 1;
the distinct old-wait control recorded receive epoch 0/current epoch 2,
privacy rejection and an empty deque while controller Stop worked with the
owner still alive. Both bounded calling-thread logs reported zero dropped
receipts, loop-exit ACK and exact captured-path absence. The TTL-zero control
observed actual expiry at the independently computed header time plus 51 ms,
and both existing turned/count readers refused the expired value. No old
failure cause, native timing or thread join is inferred.

The frozen runtime source hashes, executable and retained raw dependency-file
hashes, bounded receipts, preserved first lint failure and final guard hashes
are recorded in the accompanying sanitized host packet,
[host-packet-eaac.json](host-packet-eaac.json). The post-gate result
annotation in this document is not a new runtime input or a replay.


## Delivered-prerequisite integration on 9bde

Diagnostic #3955 was train-delivered as `a9a97969e`, patch-equivalent to
`eaac11577`. Only the repair commit was rebased onto frozen Main
`9bde744147`; qualified `580fa1f242` and its original gate remain preserved.
The integration commit is `397b2c587`. Its authored added/removed payload,
author/message and all six function bodies are unchanged. Its normal
cfg(test)-elided production projection equals the **new Main**, including
upstream sleep behavior, rather than the old diagnostic parent.

The old 297 captured inputs partitioned into 274 unchanged and 23 changed
Voice inputs. A fresh integration gate therefore ran; none of the earlier
six passes was transferred. Current capture binds 298 inputs, adding the
upstream `sleep_asked.rs`, actual features `[]`, a debug-assertions-enabled
library test driver and retained raw dep-info. Formatting, no-deps metadata,
strict Voice all-target Clippy, library compile, discovery and independent
admission passed. The same six exact controls each ran once: 6 PASS,
0 failed/ignored/unrun, zero retries or unexpected panics. Independent final
audit passed, all 13 groups closed, the bank released, and 196 prior saved
copies plus this integration driver were guarded.

This separate gate took 471.587 seconds: fmt 12.668 seconds, metadata
0.082 seconds, strict Voice 91.422 seconds and library compile 125.304 seconds.
Its six process wall times, in the listed order, were 0.051711, 1.062616,
0.056394, 0.098839, 0.025450 and 0.026030 seconds. Again the five model
outcomes were Busy/Unknown/Unknown/Unknown/Busy; the privacy row admitted one
post-cutoff epoch-2 look. Real fresh epoch-2 admission and old epoch-0/current-2
rejection remained distinct, both zero-drop and with loop-exit ACK plus exact
socket-path absence. Actual expiry was observed at header plus 52 ms, beyond
the independently computed plus-51-ms boundary. No drift classification,
OS-thread join, historical failure cause or native repair result is inferred.

The integration driver SHA256 is
`40a898b86dc20b7baff51c590e1db595937883208544a21a9434c46b8da133d7`;
retained raw dep-info SHA256 is
`867f2f91f2ee05a89279eecbf3c1be2b76d01808ecae047debc28b30f920b6b3`.
The sanitized integration host packet,
[host-packet-9bde.json](host-packet-9bde.json), has SHA256
`fbcb57f20b9ee22a2988d30b9949500c61c2dd14f3c945444be524e2d0e9745d`.
This result annotation is outside captured driver inputs and does not replay
the gate. All prior failed runs and the initial strict-lint failure remain
preserved, unexplained where previously unexplained, and unwaived.
