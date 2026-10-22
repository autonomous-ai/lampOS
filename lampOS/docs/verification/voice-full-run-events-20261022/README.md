# Voice full-library fixture events, 22 October 2026

Part of #1569. These are host fixture checks, not microphone, speaker or
on-device latency measurements. No production lease, RPC limit, cancellation
rule or assertion is relaxed. Injected setup pauses and overlap hooks live
only in the evidence patches; they are not compiled into the submitted tests.

The seven assigned library cases are:

- `provider_worker::tests::requested_audio_crosses_the_live_worker_only_after_its_tool_reply`
- `provider_worker::tests::policy_holds::a_timer_in_a_turn_that_follows_the_lamp_is_set_though_the_policy_said_stay_quiet`
- `provider_worker::tests::reminders::a_reminder_rung_into_a_busy_room_is_no_answer_to_end_it_by`
- `remember::tests::the_oldest_of_a_long_kept_list_is_found_and_the_rooms_are_marked`
- `second_provider::worker::tests::stop_during_music_rpc_revokes_scope_and_finishes_without_waiting_for_the_reply`
- `second_provider::worker::tests::the_lamps_own_answers_to_talk_keep_no_conversation_going`
- `coordinator::tests::requested_audio::cancelled_gapped_or_expired_music_never_acknowledges_its_confirmation_news`

## Unchanged baseline

Baseline: `69f2228338a8df63a81d23087c3e55308fa125ca`, including the Tasks
signed-out fix #2954 (`2202bf2f5`). On x86_64 macOS, Rust 1.96.0, with
`RUST_TEST_THREADS` unset:

```sh
cargo test -p lamp-voice --lib --offline
```

The full library run returned 101: **3,350 passed, 2 failed, 14 ignored**
(3,366 listed; 246.67 s reported by libtest). All seven cases named in Queue
#2602 passed in this particular observation. This run does not reproduce
Blue's complete earlier five-failure set. Its two failures were:

- `coordinator::tests::requested_audio::an_empty_or_gapped_confirmation_cancels_instead_of_starting_the_recording`:
  the existing `wait_requested_audio_ready(WAIT)` assertion failed.
- `provider_worker::tests::review_2026_10_14::a_music_action_before_decline_keeps_its_single_offer_and_confirmation`:
  the expected confirmation was replaced by an unavailable-Skills error.

`x86-before.json` records the commands, exact base, source aggregate digest,
platform, counts, exits and timing. `x86-before-failures.txt` retains the raw
failure section, including both original assertions. The complete passing
lines and Cargo compilation JSON remain outside the repository; their
recorded hashes do not make them portable evidence. The failure-section
hash before its wrapper is in `x86-before-failures.sha256`.

## Controlled setup and ownership failures

Every entry below was predeclared, run without retrying for green, and kept
its original test assertions. The four pause cases ran once per side. The
conversation overlap ran three times per side.

| Control | Before | After | What distinguishes it |
|---|---:|---:|---|
| Timer: host stalled 200 ms after speech, before Policy decision | fail | pass | Old host-clock authority expired; existing paused fixture clock retains the same 100 ms logical lease |
| Busy-room reminder: host stalled 200 ms after ringing | fail | pass | Same clock-domain isolation; provider event and busy-room checks remain |
| DeclineMusic: setup held 4 s before starting the provider | fail | pass | First 3 s Skills accept watchdog starts at dispatch rather than fixture construction |
| Live requested audio: setup held 4 s before starting the provider | fail | pass | Same dispatch rendezvous, with direct and wrapped tool calls still checked |
| Music Stop and own-answer conversation overlap | fail 3/3 | pass 3/3 | Old fixture replaces shared conversation state; new fixture takes the existing process guard |

`controls/commands.json` contains every command, exit, timeout flag, duration
and raw output digest, including successful compilations. Complete small
control outputs are beside it. A final `END RAW CONTROL OUTPUT` line is a
wrapper; command digests describe the original bytes before that wrapper.
The compilation JSON itself stays outside the repository.

The overlap is scheduled with positive rendezvous events: `OwnReady`, then
`MusicPending` before the original own-answer assertions in the control;
`MusicContended`, successful own-answer assertions, then `MusicPending` in
the candidate. All three baseline runs have `own_question_before=true`,
`own_question_after=false`, the original reply-vector assertion failure and
`own_ok=false music_ok=true`. All three candidate runs retain the question
and finish `own_ok=true music_ok=true`. A watchdog or different assertion
would be inconclusive, not a reproduced defect.

The patches and family READMEs in `controls/` record the exact scratch
mutations used. The after lease probe predates folding its existing logical
clock opt-in into the submitted test, so its two clock additions must not be
applied twice. `controls/restoration.json` proves the scratch baseline files
were restored after execution. No scratch test or fixed sleep is submitted.

## ARM64 full baseline

[Run #5876](https://github.com/autonomous-ai/lampOS/actions/runs/39767114240)
was dispatched with `full=true` on the same unchanged `69f222833` base.
All four Linux ARM64 jobs are green: **10,452 passed, 0 failed**
(2,608 + 2,591 + 2,511 + 2,742). One passing test is reported leaky.
There are 10,519 listed nextest cases and 67 not run across the four
partitions. Per-shard skip counts include the other partitions; they must
not be added together as ignored tests. Doc-test/own-harness summaries
are retained separately in `arm64-before-counts.txt`. Run metadata and
exact per-shard nextest summaries are recorded beside this file.
No ARM64 compilation was done on the Mac.

## Final comparison base

Lamp-voice advanced four commits while the controls ran, to
`e2232b8369fb125af6c79e8bf6927ec590a612e9`. Those commits include Voice
product changes, so the final comparison takes a new unchanged full-run
baseline at this head rather than treating a different tree as identical.
The earlier `69f222833` observation and all of its failures stay recorded.

## Missing-dispatch negative control

Suppressing the dispatch under the new rendezvous still fails the original
live requested-audio case: **0 passed, 1 failed**, exit 101, 3.02 s in
libtest. The helper reports `TimedOut`; the original `skill.join().unwrap()`
then fails. It was compiled and executed once, not retried. The earlier
external launch could not find Cargo and started no compiler or test;
`launch-provenance.json` retains that distinction. The raw output and exact
source restoration proof are in `controls/live-dispatch/negative-executed/`.
The final gates rebuild because this intentional mutant was the last
artifact in that control target.

For replay on the submitted candidate, `controls/lease-and-decline/after-pauses-only.patch`
contains only the three host-stall injections; its `git apply --check` passes
with the clock opt-ins already present. The historical after patch is retained
to identify the source used by the executed controls. External preparation
scripts are not submitted under lampOS. Apply a control patch in a disposable
worktree, compile `cargo test -p lamp-voice --lib --no-run --offline`, and run
the exact case named in `controls/commands.json` with `--exact --nocapture
--test-threads=1`. The scheduled overlap patch adds its one scratch test;
its extra listing is not part of the submitted library inventory.

## Memory response preparation and real deadline

The long-list memory fixture now prepares the same 150-entry response before
each real request begins and waits for the fake worker's readiness event.
The two `hits_note` calls, all three assertions and the production 60 ms
transport deadline are unchanged. Readiness proves prepared fixture setup;
it does not prove response receipt or prevent descheduling during a real call.
The original case contained no fixed settling sleep.

The [memory control record](controls/memory/README.md) retains three one-shot
outcomes: changing Hue to Da Nang fails the original Hue assertion; holding
the reply 160 ms after readiness fails the original first `hits_note` unwrap;
and holding setup 160 ms before readiness passes both calls and all original
assertions. Exact bytes were restored after each control and finally.

The initial sandbox denied a local socket bind before Cargo ran. The
socket-enabled run succeeded. Its first intended failure then exposed a
runner parser mismatch with Rust's numeric thread-ID suffix; the raw failure
and runner stop were retained, the parser was corrected, and only the two
unrun controls were executed. No control was retried. These native x86_64
fixture checks do not establish device latency or complete the final gates.
They ran on the earlier `69f222833` candidate. The memory fixture body is
byte-identical after the `e2232b836` rebase; newer product changes remain part
of the separate final-tree validation.

At this refreshed base, the unchanged x86 full library returned **3,355
passed, 0 failed, 14 ignored**, 3,369 listed, 304.72 s in libtest.
`x86-before-current-results.json` and `x86-before-current-summary.txt`
retain the exact command and digest. This later green observation does not
erase the earlier two-failure full run or establish those cases deterministic.

`controls/before-live.txt` trims one trailing space from libtest's unfinished
progress line for whitespace checks. `controls/before-live.raw.json` preserves
the exact original UTF-8 bytes, including that space, with their SHA-256;
all failure diagnostics remain visible in the text rendering.

The refreshed ARM64 full baseline is also green:
[run #5895](https://github.com/autonomous-ai/lampOS/actions/runs/39770980758),
`e2232b836`, **10,456 nextest passes, 0 failures** across four shards
(2,610 + 2,591 + 2,744 + 2,511). Of 10,523 listed cases, 67 are not run
across the partitions; one passing case is reported leaky. Doc-test and
own-harness results are separate in `arm64-before-current-counts.txt`.
The final candidate library's compiled list is byte-identical to this base's
x86 3,369-case list: SHA-256
`994422cff3884b14f27f75e3b47c2a990b3cf0f44477c8230c795a7600a9e3cd`.

The first native final-gate attempt stopped at formatting: one new dispatch
method chain needed wrapping. Only that layout was corrected; no compiler or
suite result is erased. The subsequent formatting gate and strict Voice
Clippy pass. Its failed formatting output and command metadata are retained.

## First full x86 after observation

The frozen candidate returned **3,352 passed, 3 failed, 14 ignored**,
299.96 s in libtest, exit 101. All seven assigned cases passed. This is a
failed gate, not readiness evidence. Its three failures were:

- `coordinator::tests::termination::sigint_retains_failure_and_acknowledged_synthetic_diagnostics`
- `coordinator::tests::termination::sigterm_retains_failure_and_acknowledged_synthetic_diagnostics`
- `coordinator::tests::second_route::the_second_routes_answer_nods_and_sways_the_head_as_it_is_spoken`

The two signal fixtures reported that a ready child did not finish within
the existing real four-second bound. The second-route fixture reported its
existing scenario host watchdog and failed child-exit observation. Both raw
failure streams and command metadata are retained in
`x86-after-first-failures.txt` and `x86-after-first-results.json`.
Unchanged source alone does not establish that these are inherited; a matching
baseline run under the same parallel workload checks that attribution.

## Requested-audio clock, loaded-ticket pause and unwind

The [requested-audio control record](controls/requested-audio/README.md)
retains a whole-child pause after original admission, Start authorization,
WAV loading and readiness. The unchanged `e2232b836` base fails the original
`test.prepare()` assertion **3/3**; the final fixture passes **3/3**, including
all five cancellation/expiry reasons in every run. Requested pauses were
300 ms; actual pauses were 303.857–305.044 ms. All 142 original coordinator
assertions remain. The real 250 ms host handoff gate, explicit expiry checks
and unchanged runtime cache-contention cancellation test remain in force.

A separate forced-zero park watchdog reproduces a destructor abort in the
initial test harness and an ordinary failing-test exit in the final harness.
All eight candidate source hashes stayed unchanged during these isolated
controls, and the probe files were restored exactly. Full raw UTF-8 streams,
including the pre-marker socket-permission failure, are preserved losslessly
in JSON with byte lengths and SHA-256. The record also discloses that numeric
child PIDs were not saved; signal targeting is audited from the direct-child
runner code, without attributing other suite failures to these controls.

The unchanged e223 baseline comparison under parallel compiler work returned
**3,355 passed, 0 failed, 14 ignored**, 262.86 s in libtest. The three failed
after cases all passed there. This comparison therefore does not establish
those failures as inherited. Source/process audit found no direct coupling:
the two signal tests run fresh execs without the new fixture clocks, and
second-route has its own existing thread-local scope. The latter's cleanup
errors aggregate by stage/owner and may follow its already-expired host
watchdog; `(5 occurrences)` does not prove five waits on the same PID.

The native `cargo build --workspace --offline` gate passes on the frozen
candidate. Its command, content digest and exit are in
`workspace-build-results.json`. The remaining package gate is a separately
required check; no later success erases the failed 3,352/3/14 observation.

## Complete native package observation

The separately required `cargo test -p lamp-voice --offline` run at the same
frozen e223 candidate passed the full library: **3,355 passed, 0 failed,
14 ignored**, 229.98 s in libtest. All seven assigned cases and the earlier
three child-watchdog cases pass. Its binary unit test also passes (1/0).
This does not erase the earlier failed full-library observation.

The command then failed in the integration binary: **265 passed, 1 failed**,
`light_call::twenty_light_calls_are_each_answered_once_the_ring_says_it_shows_them`.
All 20 calls complete, but their sorted upper median is **43.437335 ms**,
beyond the unchanged **25 ms upper-median assertion**. The range is
1.700034–136.91273 ms; that maximum is descriptive, not the asserted bound.
No latency assertion is weakened. The modified
fixture code is cfg(test) library code, absent from the product library linked
by this integration binary. That source isolation does not make the package
gate green: its exit remains 101, and doc tests did not run after this failure.
The exact command/counts and raw failure are in `native-package-results.json`
and `native-package-failure.txt`.

`submitted-source-hashes.json` identifies the eight edited Rust files used by
these observations. Their fixture assertions and product timing bounds stay
unchanged. The native suite therefore has retained failures to disclose, even
though the seven assigned cases and their controlled event/clock probes pass.
The PR is published as a draft for final-head ARM64 and reviewer qualification.
