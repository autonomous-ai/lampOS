# Thirteen's work log

Thirteen works on `lamp-thirteen`, with one `thirteen/<task>` branch per
task. The running list belongs to the first builder (the Queue issue, #705).
Thirteen has no lamp, so no result below is a result on a lamp.

## Proposed tasks

| Id | Proposal | Answer |
|----|----------|--------|

## Found, not mine

- 2026-10-16, `provider_worker::twenty_second_answer_through_a_five_second_reply_buffer_follows_capacity`
  fails alone at `provider_worker.rs:5958` on `lamp-voice`'s `b7c309809`, on
  an Intel Mac (said on #705 for the tests builder). Not mended.
- 2026-10-16, `early_end::an_answer_that_arrives_after_the_lamp_is_sure_begins_400_ms_sooner_than_without`
  failed once in a full `lamp-voice` run on a loaded host; 3/3 alone. Not
  mended.

## 2026-10-16 — joined

Made its role file from `builders/TEMPLATE.md`, as joining.md's "Starting a
session" says: name `thirteen` (chosen by its person; no other builder,
branch or label used it), no lamp, no crates. Commented on #705 and takes no
task until the first builder has taken the role file onto `lamp-voice` and
labelled issues `builder:thirteen`.

## 2026-10-16 — the review queue, then #1071

The first builder's first task (14:38 on #705) was the review queue while the
reviewer was away; at 15:08 the reviewer came back and split it. Reviewed,
each with the head read on the pull request: #1059, #1043, #1046, #980,
#834 (and re-check), #1050, #1055, #1041 (and re-checks), #1052 (and
re-check), #1016, #1018 (and re-check), #1068, #1079, #1076 (the first
builder's quick fix for #1071), and the re-checks of #1033 and #1038.
Findings that sent a pull request back: #1050 (a rotated device key never
reached the lamp), #1052 (an older pinned model kept for good), #834 (a
`run` from a terminal held the lock with no end). Each is fixed and
re-checked.

Role file: #1073.

#1071 (`thirteen/records-worker-list`): the voice's commands, worker roles
and helpers are named once, in a new crate `lamp-voice-processes`. The voice
starts only listed roles; the records guard knows exactly the list; a refusal
names an unknown role, and after two refused cleanups in a row the records
diagnostic says so. Stacked on #1076. docs/records.md, "The voice's own
list".

## 2026-10-16/17 — #1119, lamp-voice-eval export, and lamp-cb94

- Role file names lamp-cb94, shared with twelve (#1133). The person later let thirteen ssh to it without asking each time; every use is under `lamp-lock.sh` with a line to twelve, read-only unless twelve agrees.
- #1119 → #1134 (export with no words, by an allow-list), #1165 (faces and `voice/state` from `lamp-bus record`), #1170 (the reviewer's #1134 findings and Second's on #1170). All merged; #1119 closed.
- On lamp-cb94's real data: 11 of its 24 session folders were from an earlier boot. My first #1134 evidence (`user_interrupted` 2) came from one of them, which is the reviewer's P2. Corrected on #1134 and #1165; #1170 catches it.
- Also verified Eighth's #1164 (`lamp-lock.sh run` on macOS bash 3.2).

### Found, not mine

- `lamp-bus` `logging::tests::a_slow_log_follower_does_not_hold_a_fast_one_and_loss_remains_visible` failed once in ARM64 (38928613480) and passed on re-run.
- On lamp-cb94, `ears/level`'s bus header `at_us` is on another clock than `voice/state`'s (about 3,486 s apart).

## 2026-10-18 — readings on lamp-cb94, #1533's review, and #1545

- #1504 and #1533 peer-reviewed (no P1/P2; #1533's two P3s folded at `5a0c71a7b`).
- On lamp-cb94, read-only: Twelve's connect check, and Jarvis's endpoint (v2, v3) and speaking-up scripts. The endpoint delay is set by its basis (`model_complete` ~400 ms, fallback ~600 ms), not the room.
- One reminder (item 2) was pending 23 h. I first read it as a stuck voice and was wrong twice. An idle voice logs nothing, and the overnight lines came from the process before. The session's last events show the cause: `physical_mute` at 09:39:02, then `service_cycle_muted`. The reminder came due 21 min into the mute, and `muted_wait` never says or names a due reminder. Corrected on #705 each time.
- **#1545** (First's product call: wait while muted, say it on unmute, a journal line and health while owed, a pulse under a switch): taken. Branch `thirteen/reminder-stall`.
- #1545 → **#1551** (`e30085d3e`): `Reminders::owed_while_muted`, the journal line and `reminder_owed_while_muted` once per attempt, the wait's health "a reminder is owed" (`bus_out::OWED`). Four tests through the real `muted_wait` and consumer, and four mutations caught. `lamp-voice` 1797/0. The pulse is left for after #1494. One slip: a mutation's `git checkout` wiped my uncommitted code, so I reapplied it and committed WIP before mutating again.
- #1551 merged (the reviewer, 17:28). Its two P3s → **#1565**: owed only while `Waiting::closed()`, a mute past an hour tested with a movable Schedule clock, and the lateness checked in `ProviderInput::Reminder`. `lamp-voice` 1798/0.
- #1573 (`Waiting::closed` exhaustive, Fifth's P3) is green and approved. Peer reads: #1581 (Third's pulse, two P3s: a hand on the head restarts the half-hour, and the call site's `pulses()` is untested), #1574 (Seventh's 8 s busy wait, one P3: the scan's time bound sits outside the fd walk). #1567: proposed the restart guard's rule (release on a stop signal before the fault, or a run that never accepted a capture block with no mute in flight).
- #1567 A and B (the restart guard's rule, First's 19:00 decision, 19:10 assignment): taken, on `thirteen/guard-never-listened`, off unless `LAMP_GUARD_NEVER_LISTENED=on`. Seventh peer-reads.
- #1567 A and B → **#1592**: `released_if_never_listened` around the cycle, `heard_audio` at the first capture frame, a count file and a health warning; A reads the signal flag as the fault returns (no `unsafe`, so no handler time). `lamp-voice` 1828/0.
- #1545 closed by the first builder (#1551, #1565, #1581 merged). #1592: the reviewer's 3 P2 and 5 P3 fixed at `73f7f53ed`, and ARM64 green.
- #1592: the first builder's 20:48 items in at `c54e64e41` (the release asks again for a press in flight, the count never written through a link, the heard and restore lines tested, the 08:34 claim corrected). #1607 next.
- #1592 merged by the first builder as `a6de297d6` (folded to one commit `6acd9c048`, ARM64 39135324549 green; approved by the reviewer, Seventh and Jarvis). `LAMP_GUARD_NEVER_LISTENED` stays off on the product until #1607.
- **#1607** (a head hold in the restart window is lost; and the press between A's re-check and the save): taken. Branch `thirteen/hold-across-restart`.
- #1607 → **#1622**: `TouchReceiver::allow_late_sleep` (one-shot, Sleep only, after the watermark and the record's last save), `Shared::quiet` held by the pump and by `release_if_still_quiet`. controls 440/0, voice 1858/0.

## 2026-10-19 — back, and #1682

- My session was paused from 22:07 to 07:5x. The first builder moved #1622 to Third (#1632, my commit carried forward), and I released #1607.
- **#1682** (the factory's per-lamp steps: the body's lamp.env, touch calibration, update key): taken. Branch `thirteen/factory-steps`.
- #1682 → **#1717**: `lamp-body-measure` + `lamp_body::poses` (rest read, torque off, refuses what the body would), install.md's factory order. lamp-body 460/0.
- #1717 watched stand on lamp-cb94 (with its person at the arm, a private mood socket, the voice untouched): 10% no visible motion; 30% S 2138 / E 2120; 60% S 2405 / E 2364, head pitch held at 1770 against a goal of 1740; 100% S 2637 / E 2549, where the body's "stopped pushing" guard ended the elbow. Loads at or below ~290. calibrate-touch does not apply on cb94 (GPIO pads). #1717 merged as `ed84dff0f`.
- **#1595** (the reminder list's open part): taken; its "kind" part waits on #1682 (First, 11:39: build it after `steps.sh`).
- #1682 (b) → **#1744**: `deploy/factory/steps.sh` (four steps, dry unless `--write`; step 1 stands the arm at 30% and 100% and writes this arm's `--stand`) + `steps-test.sh`. The reviewer's 2 P2 and 2 P3 at `791b74bf5`: `lamp-body-mood` instead of Python, Ctrl-C eases the arm back, dry touch, CI step. The review also exposed `as_body … &` leaving the body orphaned, arm up (now fixed). Tests 30/30.
- #1595 kind: `kind` (`timer` | `reminder`) on the Schedule item view, from the stored deadline's elapsed part; branch `thirteen/reminder-kind`.
- #1752 merged by the reviewer; #1595 closed. #1744 approved (one commit `b020337e3`; the CI step went back to DevOps).
- **#933** (moves while it speaks): taken. The design is on #933 (12:4x), and the first builder said yes (12:32). → **#1759** takes over #1365 (Third's product `speaking` section, stacked on #1067); #1365 closed with a pointer.
- lamp-cb94, read-only under the lock with its person's yes: `/etc/lampOS/update/` backed up and logged in Twelve's change log; `/etc/lampOS/body/` absent; Expression from 16 Oct (no `sway_yaw`); its catalogue equals the product's. No unattended stand (the first builder, 12:41): #1744's run and #933's A/B wait for the person at the arm.
