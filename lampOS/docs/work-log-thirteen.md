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
- #1759's review: no finding. Its merge is the first builder's, after #1067 and the cb94 A/B.
- #1744 + head-pad discovery (the first builder on #1362, 12:54): it uses `lamp-controls discover-head` (cb94 has no gpiod tools); exactly two driven lines with two touches each, pull following the polarity; `--to N`.
- lamp-cb94, with its person, under the lock:
  - 06:11Z: step 1 refused, `joint 1 has torque on` (a SIGTERM-ended body run keeps holding).
  - 06:13Z: step 2 found 97/98 (97 active-low, 98 active-high), but Controls then read "touched" at rest, so the previous pins were restored. Line 97's idle level changes between readings: posted on #1362 and as Found by thirteen on #1764. lamp-controls hit its start limit; `reset-failed`.
- **#1776**: `lamp-body --let-go-at-stop` (the first builder's yes, 13:14). A lamp asked to stop that got down is let go; one that didn't keeps holding. Tests on the fake bus; lamp-body 310/0.
- #1744 folded to `e43d429f3`: stands pass `--let-go-at-stop on`, and a held lamp is let go first, after a yes. jq is checked. Tests 41/41. Step 1 on cb94 waits for #1776's build.
- lamp-c0a4 replaces cb94 (the person's change). #1789 (role file) merged. #1776 merged (reviewer's P2: `LET_GO_NEAR` 20, the short-elbow test).
- #1744 on c0a4, with the person at the arm: dry run passed; the dry run found that step 1 must refuse an `activating` lamp-body (fixed). Step 1 wrote `lamp.env` (this arm's stand 2626/2602; both stands let go at the stop, #1776's first lamp run). Head pads refused (three driven lines, idle level changes; Found by thirteen on #1764). Folded to `db05c999a`.
- #933 A/B: `tools/speaking-ab.sh` on #1759's branch (10/10 tests; staging moved to `/var/lib`, since `/run` is noexec). Not run: c0a4's voice holds answers back (no name models).
- **thirteen-P1** (on #1722): the product lamp-body unit can't start on a factory `lamp.env` (`--gaze-socket` without `--gaze`).
- #933 A/B on c0a4 (person at the arm, `LAMP_SPOKEN_TO=off` and the body without gaze, both Twelve's yes, restored and logged):
  - The token limit was lifted by the person.
  - Run A (off): 11 speaking steps, the base still.
  - Run B (on): first held back by the busy-room gate; with the person facing the camera it played, 45 steps. **The person: B more alive.**
  - Only 3 settles fired, 0 nods and 0 sways. Expression was "still for work" in 372 of 796 samples.
  - **thirteen-P3** (concentration shouldn't zero the lamp's own speaking moves). P2 withdrawn: resumable voice takes `voice/sound` by design.
- `speaking-ab.sh`: `--arms`, staging in `/var/lib`, and it now counts voice levels and Expression's offsets (13/13 tests).
- **#1744 merged** by the first builder as `09108bad1` (step 1 run on c0a4; head pads with the hardware lead on #1764).
- **reviewer-two** (from #1814, merged 18:15): read #1779, #1821, #1826, #1860 and #1861; handed back #1783, #1795, #1846 and #1901 (outside the lane). **Merged #1891** (`e64f66fc3`, the base strip off unless set) and **#1887** (`461cb2549`, lamp-station, with my P3 taken). #1807 waits for its rebase.
- Own: **#1867** (thirteen-P1, gaze off without `--gaze`; reviewed, Fifth's peer read, folded `2b0c7b91f`), **#1868** (thirteen-P3), **#1898** (`steps.sh` skips the strip calibration while the strip is off). #1867 and #1868 wait for a c0a4 person window.
- **#1898 merged.** #1919 (lamp-voice's build: `UNITS` 24, `speak_up`'s `breath`) merged 13:50.
- **#1922** (the second route's answer nods and sways the head, onto #1712): its dispatched ARM64 run is green, and the merge is the first builder's.
- **#1953** (the head's speaking moves follow the voice's own level: the quiet, speaking and stress bars scale with the turn's running level against 0.34, with floors; engine tests at 0.33×, 1.0× and 1.2×; a hiss nods nothing): green, **merged into #1712** by the first builder (22:16).
- reviewer-two: read #1937 (P2: a watch counted visits before the ring showed its mark; fixed), then #1963, the same patch on today's lamp-voice (lamp-skills 181/0); merge on green ARM64 and the third builder's privacy read. #1956, #1957 and #1968 handed back (outside the lane).
- Smoke and bad-air (the first builder's 21:51): already Jarvis's #1836. c0a4 has no air sensor (read-only, under the lock), so the traces go to lamp-4ace. Row stays with Jarvis (the first builder, 21:54).
- **#1965**, the touch pads row (the first builder, 21:54; plan on #1722):
  - Each lamp's head pads read from their own rest. `discover-head --rest-seconds` counts changes with hands away, and the factory step refuses a pad whose rest changed (cb94's 97).
  - The step writes `head_rest` (`rest`, `reads: level|pulses`). A pulse pad is touched only while it pulses, so a flip is no touch.
  - The reviewer's P2 is fixed (`c1ddc42ca`): `pulses` needs a burst of fast changes, and two quick taps on a level pad give no `reads`, so the step asks again.
  - It's the first builder's merge after the 08:00 arm-hour trial.
- **#1965 merged** (touch pads: each lamp's head pads read from their own rest; the reviewer's P2 fixed). **#1976 merged** into #1712 (the scaled stress bar stops at 0.9).
- **#1990 merged** into #1712: the second route's nod test was flaky. Its syllables were past the meter's top, and one keeper thread lost sound pieces on Linux's small socket queue (Second's read). Each topic now has its own thread.
- **Row "moves with its words":**
  - `speaking-ab.sh --levels-only` (Expression in shadow, nothing moves). The reviewer's P2 is fixed: lamp-bus tap's header and VoiceSound's flattened `.level`.
  - `tools/sound-shape.sh` (levels, rises, pauses, how far ahead each piece was said).
  - `ends=` (how the turn ended). All on #1759.
- **Why the head never nodded on a lamp:** the voice says its sound 200 ms ahead (`FORETELL_US`), but the catalogues' `body_lag_us` is 300 ms, so the speaking motion always saw level 0. Shown offline: lag 0–200 gives 8 nods, 300 gives 0. The coordinator test's lag of 0 had hidden it.
- **#2030 merged** into #1712 (option 1, the first builder's choice):
  - the farthest piece said is used when none is at the lag;
  - then the rate rule (at most one nod a sentence, a word 0.15 over its sentence, never two within 4 s), as catalogue keys for the arm hour's dense/sparse comparison;
  - on lamp-4ace: 75 nods to 10–12 a story, one settle each.
- Row 2's lines are in `arm-hour.sh` (#2034). #1963: Third's P2 open (merge held).

## 2026-10-28 — #3945, #4006, #3999, #3743

The entries between 20 and 28 October are on the issues and pull requests
themselves (#2901, #3711, #3743, #3923, #3945), and on the Queue (#2602,
#3126, #3504, #3997).

### #3945, a "Ma'am, …" said to a person is heard as the lamp's name

Ready for lamp-voice: yes (#4082)

- Off unless set: `LAMP_NAME_SAID_TO_A_PERSON`. It counts only with
  `LAMP_NAME_SECOND_SIGN=on`.
- Commit `e5b2584947`, on `thirteen/said-to-a-person`.
- Gate:
  - full lib 3969/19, the 19 timed tests on a loaded Mac; all 19 pass one
    by one on the head;
  - attention 64/0, name_near 5/0, said_to_a_person 3/0, second_sign 8/0;
  - voice-eval switches 2/0;
  - clippy and fmt clean.
- lamp-c0a4, once, with a local ARM64 build of `0bd5317fe4` (the same change
  before its rebase), as a /run drop-in: a person's "Ma'am, …" heard as the
  name at 1.00 was held, and two "Lamp, …" calls were answered. Readings are
  in `docs/verification/said-to-a-person-lamp-c0a4-20261028/`.
- Not shown: more hearings of "Ma'am" (one in six raised one), and #3923's
  played clips with the switch on.

### Merged today

- #4006: how_you_sound counts a call that speech to text heard (Part of
  #3923), in train 294.
- #3999: `LAMP_SECOND_SPEECH_TAGS` on in product (#2901), in train 285.
- #3947: lamp-name logs its scores under the bars.

### #3743, the no-Wi-Fi line

Ready for lamp-voice: no

- A test render of the new line from the Mac is 13.37 s, and the notice
  loader refuses it: notices may be at most 10 s.
- `thirteen/notice-15s` raises the limit to 15 s. A notices release with a
  clip longer than 10 s must ship with or after a build carrying that limit.

### Later on 28 Oct

All merged by the train:
- #4082: #3945's switch, off unless set, plus the Reviewer-lead's `going_on` P3 and the docs fix ARM64 caught.
- #4094: #3743's 15 s notice limit. The notices release goes with or after it.
- #4102 and #4110: the early-end P3 on #4006. #4110 carried the span fix that train 309 took without.
- #4105: lamp-4ace's speech-tags numbers in switches.md.
- #4106: Sixth's P3 on #3947.

Ready for lamp-voice: nothing open.

### #4141, every line that asks a person to act (ship week)

Ready for lamp-voice: yes (#4147)

- The table of 20 rows is on #4141. Small fixes are in #4147, tested at `24501cb60d`: setup's page no longer mentions a "joined" light, setup.md names the RESET pinhole, and onboarding.md matches the product's `LAMP_ONBOARDING=on`.
- `cargo test -p lamp-setup`: 38/0, 166/0, 6/0. No lamp run (none needed).
- Larger items:
  - Product lamps have `LAMP_SETTINGS_WIFI` and `LAMP_WIFI_RECOVERY` off, so a lamp whose Wi-Fi changes says nothing. Words for #3020's recovery are proposed to Sixth.
  - The pinned `wifi_lost` clip still has the old words (#3743).
  - The app's Head line has gone to Mobile.

### #3743, the recovery hotspot's line (with Sixth's #3020)

Ready for lamp-voice: yes (#4157)

- Behind `LAMP_WIFI_RECOVERY`: as the hotspot first opens for a loss, `recover` plays the recording `wifi_recovering`: "I can't reach my Wi-Fi, so I've opened setup. Set my Wi-Fi again in the app." Tested at `3601148422`.
- Gate: setup 38/0, 167/0, 6/0; audio notice 9/0; voice notices+say 70/0; switches 2/0; clippy and fmt clean.
- Test render from the Mac: 5.44 s. Not committed.
- No lamp run: a product voice refuses a test clip, and the production render needs a person to listen first.
- The notices release waits for a build that has both #4094 and #4157.
- #4147: READ-OK from Second, Reviewer-two and Reviewer-three; ARM64 green; NEXT TRAIN posted.

### #3945, second c0a4 run from the Mac

Ready for lamp-voice: yes (#4170, docs only)

- Run on lamp-c0a4, 16:08–16:13Z: a local ARM64 build of lamp-voice `08fd8f9e2b` as a /run drop-in with `LAMP_NAME_SAID_TO_A_PERSON=on`. Six synthetic voices played from the Mac, three "Ma'am/Sir" lines each, plus one control.
- 1 of 18 heard at 0.92 and answered; the hold didn't fire. Three more were answered as a conversation going on. The control was answered.
- Restored after, lock released, Twelve told.
- A real person at a distance is still needed (Not verified on #4170).
- #3743: asked the first builder about the production render and its listen.

### #4226, the opening "Lamp" missed after desk talk (8389)

Ready for lamp-voice: yes (#4238, journal only, Part of #4226)

- A/B on lamp-c0a4, 21:26–21:53Z: 8327's and 8389's own lamp-voice, A B A B, five trials each. A synthetic voice played from the Mac said desk talk, then "Lamp, I want to cook something new this weekend." 1.5 s later.
- Let out: 8327 7 of 10, 8389 6 of 10. Not a regression.
- 5 of the 7 misses were the name heard at 0.94–0.99 and dropped by `NAME_AT_THE_START_US` (1.3 s), at 1686–1822 ms into the line.
- #4238 journals where every weighed hearing began, so the bound can be set from both sides.
- Gate: voice lib 4049/0, the other targets 275/0, 1/0, 4/0, 6/0; clippy and fmt clean.
- Restored after, lock released, Twelve told. c0a4's chat provider answers HTTP 402.
- Next: (b) the spread with this journal, (c) the bound or a first-word line start.

- #4226 handed over to Twelve on 29 Oct at 07:1x (the first builder's move). Step (c), the carried-sound start fix, landed as Twelve's #4258.

### #3945, which second sign let a named line out

Ready for lamp-voice: yes (#4295, journal only, Part of #3945)

- Mac-played run on lamp-c0a4, 02:49–02:55Z, the switch on through an env drop-in, each line at clip gain 1.0 and 0.35. None of the 18 "Ma'am/Sir" lines was answered. The one hearing at the line's start was held by the switch.
- #4295 journals the second signs that let a hearing count (`Attention::second_signs`), never the words.
- c0a4 check, 03:24–03:27Z: a "Ma'am" line soon after a call counted by "a conversation going on". One counted by "its words calling it, its hearing at the line's start" (probably a stand-in first word).
- Gate: voice lib 4062/0, the other targets 1/0, 275/0, 4/0, 6/0; clippy and fmt clean.
- Restored after, lock released, Twelve told.

### #3945 (b), a "Ma'am" start hearing in a cut line

Ready for lamp-voice: yes (branch `thirteen/said-to-a-person-cut-3945`, Part of #3945)

- With `LAMP_NAME_SAID_TO_A_PERSON=on`, a start hearing on a set-off "Ma'am" in a line cut short holds for the line after it. The fix is in `attention.rs` (`cut_said_to_a_person`), with the Reviewer-lead's P3 line in `second-voice.md`.
- Two Mac-played runs on lamp-c0a4, 14:39–14:47Z, at Mac 60% and 70%. Run 1: all eight cut lines held, the control answered. No hearing over the bar at a "Ma'am" line's start, so the new path was proven by unit tests only.
- Run 2 began 39 s after an answered call. Lines inside that conversation were answered, and a held "Sir, …" line made the cut words before it answered in its place. This is outside the change; noted on #3945.
- Gate: voice lib 4138/0 (15 ignored), the other targets 1/0, 275/0, 4/0, 6/0; workspace build, clippy and fmt clean.
- Restored after, lock released, Twelve told.
- Next: the declined line (a decline says nothing and carries no conversation).

### #3945, the declined line (#4417)

Ready for lamp-voice: yes (#4417, behind `LAMP_DECLINE_SILENT`, off; Part of #3945)

- A line the model declines says nothing, and its let-out is undone so it carries no conversation (`Did::Declined`). Round 1's words wait for its end on spoken lines told `DECLINE_NOTE`/`CONVERSATION_ALONE`; named lines don't wait.
- #4406 merged in train 375; its two wording P3s ride in #4417.
- c0a4, 17:20–17:40Z, Mac-played, the switch off and on: two declines silent with "no conversation going on from it". Words before a call, and a decline carrying a line, weren't reached on the lamp; the unit tests cover them. Restored after, lock released, Twelve told (its #4175 drop-in saved, with the local manager's yes).
- Gate on 2c28a0f974: voice lib 4144/1, the one being the timed `CarryDesk` test, 3 of 3 alone; before the rebase 4142/0 and 1, 275, 4, 6.
- #4385: HOLD then READ-OK on 9ff9b3929b.

### #4417's P3s, the decline's carry window (#4429)

Ready for lamp-voice: yes (#4429, behind `LAMP_DECLINE_SILENT`, off; Part of #3945)

- #4417 merged in train 381.
- The live carry window's `held`/`remember` writes after a let-out now reach the decline's snapshot (Sixth's and Reviewer-three's P3s). The new test fails without the `colleague_held` mirror.
- Gate: voice lib 4150/0, the other targets 1, 275, 4, 6; clippy, fmt, workspace build clean.

### #4429's P3s, a new caller and the driven copies (#4435)

Ready for lamp-voice: yes (#4435, behind `LAMP_DECLINE_SILENT`, off; Part of #3945)

- #4429 merged in train 384.
- A decline's saved carry window takes the live window's copies only while the live window is still that conversation's (same `first_request`), so a new caller's held line no longer stretches it (Reviewer-three's P3).
- #4429's hand-written test is replaced by three tests driven through judge. Each fails with its copy taken out (checked in a throwaway worktree).
- Gate on 4369df29aa: voice lib 4152/0 (15 ignored), the other targets 1, 275, 4, 6; clippy, fmt, workspace build clean.

### #4435's P3, the request a window was begun for (#4444)

Ready for lamp-voice: yes (#4444, behind `LAMP_DECLINE_SILENT`, off; Part of #3945)

- #4435 merged in train 387.
- The decline's saved carry window and the live one are matched by the request each was begun for, which an answer leaves, so an answer to an earlier chained line no longer splits them within one conversation (the Reviewer-lead's P3).
- One new test, which fails under the old comparison.
- Gate on f56d04bae0: voice lib 4155/2 and it 274/1, all three known timed process tests (#1569); the motor ones fail on lamp-voice itself here too. Others 1, 4, 6; clippy, fmt, workspace build clean.

### #3945, the decline's hold narrowed (#4458)

Ready for lamp-voice: yes (#4458, behind `LAMP_DECLINE_SILENT`, off; Part of #3945)

- #4444 merged in train 390.
- The first builder's call: the switch stays off, and round 1's words wait only in a busy room or with a second voice or face. In a quiet room with one person they stream as with it off.
- Two tests, each failing under its mutation.
- Gate on 86a2520d88: voice lib 4159/1, the one being a known timed motor test (#1569); the others 1, 275, 4, 6; clippy, fmt, workspace build clean.
- Next: "May, what would you do?" held as calling someone else, mid-conversation with one person (the first builder's 07:3x ask).

### #3945, "May, what would you do?" (#4460)

Ready for lamp-voice: yes (#4460, `LAMP_SPOKEN_TO`, on in product; Part of #3945)

- The first builder's 07:3x ask: a set-off name that is also a common word, at a line's start in a conversation with one person, in a room not busy, isn't taken as calling someone else.
- One test with the line as heard; four mutations each fail it in a different place.
- Gate on e05bd780f0: voice lib 4158/1, the one being a known timed motor test (#1569); the others 1, 275, 4, 6; clippy, fmt, workspace build clean.
- c0a4 run of the line as heard to follow.
