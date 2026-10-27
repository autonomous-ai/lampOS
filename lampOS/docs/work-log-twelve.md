# Twelve's work log

Twelve works on `lamp-twelve`, with one `twelve/<task>` branch per task. The
running list belongs to the first builder (the Queue issue, #705). Twelve's
person named lamp-cb94 for it; there is no owner file for that lamp yet, so
no result below is a result on a lamp.

## Proposed tasks

| Id | Proposal | Answer |
|----|----------|--------|

## Found, not mine

- 2026-10-16, `reminders::transport::tests::in_process_test_helper_transfers_and_releases_the_real_claim`
  fails at `reminders/transport.rs:588` (the claim is still locked after both
  ends drop) in 2 of 20 runs of `cargo test -p lamp-voice --lib reminder` on
  base `bc68dec87`, on an idle Intel Mac. Not mended.

## 2026-10-16 — joined, and #920 taken

Ready for lamp-voice: no

- Role file `lampOS/builders/twelve.md` made from the template, on
  `lamp-twelve` from `origin/lamp-voice` at `61c7cada7`. No `builder:twelve`
  label exists; twelve does not make one (the owner's yes is needed).
- Twelve's person asked it to take #920 (unlabelled, untaken): the
  lamp-actions bus test reads back a state with a 20 ms life, which a loaded
  runner outlives. Taken with `status:taken` before the first builder has
  taken this role file onto `lamp-voice`, at the person's request; the work
  is on `twelve/920-light-stale-window`.
- lamp-cb94: named by the person, not touched. #920 needs no lamp.

## 2026-10-16 — #920, the light's bus test window

Ready for lamp-voice: yes (#981)

- Kind: off unless set. Test-only, inside `#[cfg(test)]` of `lamp-actions`
  (`light::tests::through_the_bus_a_request_is_kept_and_a_stale_state_is_no_light`):
  the ring's expiring state lives 500 ms, not 20 ms, and the stale read waits
  600 ms. No code a lamp runs changes. Commit `103625ddf` on
  `twelve/920-light-stale-window`.
- Gate on macOS x86_64 (Intel i9), from `lampOS/`: `cargo fmt --all --check`
  clean; clippy `-D warnings` clean for `lamp-actions` and `lamp-voice`;
  `cargo test -p lamp-actions` 34 passed, 0 failed; `cargo build --workspace`
  ok; `cargo test -p lamp-voice` 1234 passed, 40 failed, 8 ignored: the timed
  process tests of #739/#893 on a loaded host (22 in
  `coordinator::motor::tests`), one of them passes run alone; not run again.
- ARM64 run 38847348555, green on `103625ddf`.
- Not shown: the old test never failed here (20/20 with every core busy,
  30/30 with 200 busy processes), so the failure was seen only on the ARM64
  runner. No lamp: none was needed.

## 2026-10-16 — the `builder:twelve` label

Ready for lamp-voice: no

- At its person's request ("đánh dấu builder:twelve luôn đi"), twelve made the
  label `builder:twelve` and put it on #920, and told the first builder on
  #705 (comment 6207819404), which may correct or remove it.

## 2026-10-16 — #920 released

Ready for lamp-voice: no

- Ten, which opened #920, closed it at 11:2x (its person: problems go to the
  first builder first, not into issues). At twelve's person's request twelve
  released it: `status:taken` and `builder:twelve` removed, #981 closed, and
  the branch `twelve/920-light-stale-window` deleted at its person's request
  (the fix was commit `103625ddf`, one test in `crates/actions/src/light.rs`:
  a 500 ms life and a 600 ms wait in place of 20 ms and 60 ms).

## 2026-10-16 — #935 taken

Ready for lamp-voice: no

- The first builder gave twelve #935 on #705 at 11:22 (moved from the
  eighth): the coordinator reminder round trip's `AckConfirmed` is missing
  after one Ack request. Find the cause and make the test deterministic from
  it, as the seventh did for #893 (stepped clock, no retries, no longer
  timeouts). One pull request into `lamp-voice`. Taken at 11:2x; work on
  `twelve/935-reminder-ack` from `origin/lamp-voice`.

## 2026-10-16 — #935, the reminder Ack on a busy disk

Ready for lamp-voice: yes (#1009)

- Kind: off unless set. Test-only: `request_deadline()` in
  `reminders/consumer.rs` is `Instant::now() + REQUEST_BUDGET` as before
  outside a `cfg(test)` scenario scope. Commit `0485ae63f` on
  `twelve/935-reminder-ack`.
- Cause, the fixture: one tick's 250 ms socket budget also spans two
  `fullfsync` commits (Ack plan, Schedule's Ack). Idle Intel Mac ~160 ms of
  250; busy disk 318–340 ms for the plan alone. Production recovers next tick.
- Fix: the two real-Store tests (`consumer/tests.rs:644`, the four
  `reminder_roundtrip` tests) run on the stepped scenario clock. Every
  assertion kept, no retries. Doc: `docs/voice-test-fixtures.md`.
- Shown, Intel Mac, four `F_FULLFSYNC` writers: base 0/10, change 10/10.
  Evidence `docs/verification/935-reminder-ack-mac-20261016/`.
- Gate: fmt clean; clippy `-p lamp-voice` clean; `cargo test -p lamp-voice`
  1,274 passed, 33 failed (the #739/#893 timed tests and the claim test
  above, none #935's), 8 ignored; workspace build ok. ARM64 run 38852786535
  green.
- Not shown: the line-338 failure itself (its boundary, line 336, was). No
  lamp.

## 2026-10-16 — the reply buffer capacity test taken

Ready for lamp-voice: no

- The first builder, on #705 at 12:16, gave twelve its list: (1) #1009 until
  it merges, (2) `provider_worker::tests::twenty_second_answer_through_a_five_second_reply_buffer_follows_capacity`
  (ReadTimeout, then ConnectionReset at its unwrap, red in the reviewer's
  #985 gate at 11:42), deterministic from its cause, one pull request,
  (3) then ask on #705 with what was found in the day's other red voice
  tests. (2) has no issue; taken at 12:2x on `twelve/reply-buffer-capacity`.

## 2026-10-16 — the reply buffer capacity test

Ready for lamp-voice: yes (#1035)

- Kind: off unless set. Test-only: `provider_worker.rs` reads
  `scenario_clock::monotonic_us` under `cfg(test)`, `lamp_ipc` otherwise.
  Commit `9f5cb82b3` on `twelve/reply-buffer-capacity`.
- Cause, the fixture: the rig runs coordinator, worker and scripted service
  on one thread; decoding the largest messages held a coordinator tick back
  101–144 ms, past the rig's 100 ms leases, so the turn was lost, the worker
  retired the answer it was receiving, then BarrierTimeout (or ReadTimeout)
  and ConnectionReset at the heartbeat. Gaps of 60–99 ms in every run.
- Fix: `speaking_speed` enters a scenario scope, one 2 ms step per tick;
  playback, the hold and the service pacing stay on the host clock. Doc:
  `docs/voice-test-fixtures.md`.
- Shown, Intel Mac, 24 CPU-bound processes: base 0/10 (all
  BarrierTimeout), change 10/10. Alone, idle: base 2/8 failed. Evidence
  `docs/verification/reply-buffer-capacity-mac-20261016/`.
- Gate: fmt clean; clippy `-p lamp-voice` clean; `cargo test -p lamp-voice`
  1,292 passed, 40 failed (motor, ring startup, optional ring, one duplex,
  one `reminder_roundtrip` that #1009 mends), 10 ignored; workspace build
  ok. ARM64 run 38858870677 green, started by hand (the pull request event
  started none), so on the branch, not the merge.
- Not shown: the reviewer's exact ReadTimeout run; no lamp.

## 2026-10-16 — #1009 merged, #1035 rebased

Ready for lamp-voice: yes (#1035)

- Peer reviews: Second on #1009 and Sixth on #1035, no finding. Both
  rebased at the first builder's request (14:2x) and pushed; their ARM64
  runs 38869688115 and 38870049874 green.
- #1009 merged by the first builder at 15:06 (`fc8ed84a4`). #935 left for
  the merger to close.
- #1035 then conflicted in `docs/voice-test-fixtures.md` (both added a
  section at the end); rebased as `c2d60f14c`, both sections kept. fmt and
  clippy clean; the capacity test and the reminder round-trip tests pass.

## 2026-10-16 — peer review of #993

Ready for lamp-voice: no

- The first builder's 14:26 gave twelve the peer review of Third's #993
  (light requests that lapse, #940). Reviewed head `1105f6c2b`: no P1 or P2,
  both reviewer P2s fixed. One P3: an asker refused for want of a place is
  later shown over the person's light, where the same asker alone stays
  given way (probe, not committed: alone not shown, ring full shown red).
  fmt and clippy clean, ring tests 21/0, msgs 41/0. Reported on #993 and
  #705; list empty, next task asked of the first builder.

## 2026-10-16 — #1035 merged

Ready for lamp-voice: no

- The first builder merged #1035 at 15:25 (ARM64 38871955140 green on
  `c2d60f14c`, Sixth's re-look clean). Twelve's list is empty; the next
  task was asked for on #705 after the #993 review.

## 2026-10-16 — due reminders never said on lamp-4ace, taken

Ready for lamp-voice: no

- The first builder's 15:30 list for twelve: (1) due reminders are never
  said on lamp-4ace (ids 12 and 26 still `pending`, no consumer line at
  15:00): find where a due item stops, journal every due item the consumer
  gets and what it did, fix with a test; (2) what a reminder overdue by hours
  does, to decide with the first builder; (3) the day's red voice tests.
  (1) taken at 15:3x on `twelve/reminder-due`.
- 16:1x: the journal is draft #1081 (`100ae8212`, ARM64 38875754147):
  one line per change in what the consumer does with each due item, and
  one when the due notice cannot be read. Strongest candidate so far, from
  the code: a reminder turn cancelled or settled without proof (`Receipt`
  dropped, `Abandon`) leaves the ledger entry unresolved, `known()` true,
  so it is never offered or acknowledged again; also a reminder said with a
  playback gap is never `completed()`. Asked the first builder for the
  lamp's ledger and trace for ids 12 and 26.

## 2026-10-16 — due reminders: #1081 ready

Ready for lamp-voice: yes (#1081)

- Kind: changes every lamp's default where `LAMP_SCHEDULE_SPEAK=on`.
  Head `56a1ba939` on `twelve/reminder-due`, three commits: the consumer's
  journal (once per change); attempts an earlier voice prepared and never
  completed are retired with `PreviousProcessExited` once the next voice
  holds the claim, and offered again; late under three minutes is said as on
  time, later the delay is named once in rounded words.
- Cause, from the first builder's lamp-4ace readings at 15:37: ids 12 and 26
  were prepared by earlier voices and nothing after; the next consumer saw
  them as `known()` and never offered them again.
- Tests: the first builder's prepared → restart → offered again → said once
  → acknowledged; the old never-re-offer test now holds only within the
  voice that made the attempt; the journal; each lateness case.
- Gate: fmt, clippy `-p lamp-voice` clean; workspace build ok;
  `cargo test -p lamp-voice` 1,349 passed, 33 failed (timed motor, ring and
  two coordinator tests on this Mac, none reminders), 10 ignored. ARM64
  38877109897 green.
- Not shown: on a lamp. List item 2 (an hours-overdue reminder) is open, to
  decide with the first builder.

## 2026-10-16 — #1095 reviewed, overdue reminders as #1103

Ready for lamp-voice: no

- Peer review of Thirteen's #1095 on `af351c72a`: no P1 or P2; one P3
  (`short-turns` and `stt-check` are voice commands not on the new list, so
  the guard still refuses a cleanup while one runs).
- The first builder's item 2 is draft #1103 (`ab1deded3`, stacked on #1081):
  a reminder due an hour or more ago waits for someone at the lamp and is
  said once, gently, as asked for earlier, then acknowledged. Limit: a newer
  timer waits behind it until someone comes.

## 2026-10-16 — paused by its person

Ready for lamp-voice: yes (#1081)

- #1081 ready, ARM64 38881485530 green on `f7d82fa6c` (the stderr fix: the
  consumer's lines were thrown away on lamp-4ace in stack27). #1103 draft,
  stacked on #1081, to rebase after #1081 merges. #1095 peer review done, no
  finding on `9a08bf282`.
- The person paused twelve at 16:5x: no loop, no new task until they ask.

## 2026-10-16 — evening: #1081 merged, #1103 ready, lamp-cb94 on V2, reviews

Ready for lamp-voice: yes (#1103)

- #1081 merged by the reviewer at 21:23 (`a19468dd6`, after its P2 and P3s).
- #1103 rebased onto `lamp-voice` (force-with-lease on its own branch, for
  the conflict), one commit `5d0479854`; the overdue gate runs before the
  say service's `for_skill`. Reminder tests 87/0; ARM64 38923473743 green.
  Kind: changes every lamp's default where reminders are spoken.
- lamp-cb94 (owner file #1127, merged): V1 stopped and kept
  (`~/twelve-backup-20261016/`), build 1314 installed by `lamp-update
  install` from a test-signed local channel (key `299144c6f1c0112b`),
  turn-end model and ONNX Runtime placed. The voice cannot speak: the proxy
  refuses the lamp's device key with `quota_exceeded`. `lamp-air` stopped
  and disabled there: no air sensor, it flooded the kernel log.
- Peer reviews: #1126 (Fourth) no P1/P2, one P3.
- Not shown on a lamp: #1103's wait for someone, and a timer across a
  restart said once; both wait on a key with quota for lamp-cb94.

## 2026-10-17 — #1103 merged; reviews; lamp-cb94 checks

Ready for lamp-voice: no (#1193 opened, ARM64 starting)

- #1103 merged by the first builder at 02:42 (`698dd1789`, the reviewer's P2
  fixed: an unknown lateness waits for someone too). Its last P3s and Nine's
  lowered P2 are #1193 (a journal line's words and `schedule.md`).
- Peer reviews, each no finding: #1161 (merged), #1131 (merged), #1186,
  #1175 (the reviewer's P2 fixed).
- lamp-cb94: `lamp-air` with #1163 checked twice (16 min on the PR build,
  31 min on `lamp-voice` `6557d70aa`): one probe every 10 min, 4 and 6
  kernel lines against 598 per 2 min before; it stays enabled there. Eyes
  run with this lamp's camera port. The voice still cannot speak
  (`quota_exceeded`).
- 03:2x: #1193 merged by the reviewer (ARM64 green on its second attempt, the
  first red only on the `lamp-bus` logging flake). List empty; asked the
  first builder for the next.

## 2026-10-18 — #1471 merged; Fast answers taken

- #1465 fault 1 as #1471: a held action asked for again ends its turn
  once, and the model is told it will not be done. Built on Tests'
  failing test, then folded into one commit for the reviewer's P2 (one
  `--force-with-lease` on twelve/held-call-bound, said on #705). Two held
  calls made together don't end the turn. ARM64 green at `195426057`.
  The first builder's lamp check on lamp-4ace: the most asks of one
  request was 1 (72 at 10:57). Merged as `d3e594bfc`. No spoken line:
  the first builder chose quiet.
- Took the Fast answers row (12:2x). Read the voice session events'
  format on lamp-cb94, read-only under the lock. Posted a reply-time
  script on #705 for the first builder to run over lamp-4ace's batteries.
  lamp-cb94 itself: ordinary turns median 1.00 s, p90 1.08 s.
- Fast answers, closed as the model's own time: #1520 (`LAMP_SHORT_DONE`,
  never short after an "already" answer) merged; read-only scripts posted
  on #705 (`reply_times_twelve.py` v1–v3, `prompt_times_twelve.py`,
  `slow_turns_twelve.py` v1–v2, `short_asks_twelve.py` v1–v2,
  `search_asks_twelve.py`, `connect_check_twelve.py`, `soak-check.sh` v2),
  each run by the first builder on lamp-4ace and tried on lamp-cb94 under
  its lock. The doc row is #1596 (merged).
- lamp-cb94: the reminder never said on 17 Oct was a physical mute
  (Thirteen); my "stuck" read was wrong and corrected on #705. The soak rule
  now: a pass needs a turn (`soak-check.sh`).
- #1595 → #1603 (merged): a `pending_only` reminders list says how many are
  set and not yet due. Its P3s are #1633 (carried by Tests).
- #1562 carried forward as #1616 (merged): the set-aside store keeps its
  identity so the delivery ledger follows.

## 2026-10-19 — #1716; #1679 next

- #1716: Schedule's health warns while a `newer-*` store is there, and an
  empty store beside one keeps its identity after a crash (the reviewer's
  #1616 P3). Offline overnight; back at 10:10 +07.
- Next: #1679 (light scenes, pointing, light shows), then #1693 (memory).
- #1693 (1) as #1723: `forget {source_id}` and `forget_everything`, behind
  `LAMP_MEMORY_FORGET_ALL`. Two copies of this session ran in one worktree
  for a while this morning; the duplicate stopped, its tests were kept, and
  its withdrawn design comment is marked on #1693.
- twelve-P1, #1720 (proposal): the Queue #705 is at GitHub's 2,500-comment
  limit, and nobody can post there; reports go on PRs until a new queue.
- #1723 merged: "forget everything" for a known person only, after the
  reviewer's P2 (it used to empty the room's memory with nobody known).
- #1733 merged: #1693 (3), one open thread a day asked about once, behind
  `LAMP_MEMORY_THREADS`.
- #1730 merged (#1679, built by the first builder's agent): rebased as one
  commit. From my peer read: `toward`'s sign pinned by a test, a pose held
  again let go by a later scene, and a kept light that can't be mixed with
  one given back.
- #1693 (4): the scenes and the person's checklist (5, 7) are on #1693, with
  `memory_counts_twelve.py`. The first builder runs 1, 2, 4 and 6 tonight
  and 3 at 07:00. (2) waits on #1712.
- #820 taken (the first builder, 13:44). The audit is on #820: the voice
  kept its own `/etc/localtime` reader, which past the changes a zone file
  lists gave no time and counted days in UTC, and its Days client read the
  zone once. Schedule reads `zone.env` once. lamp-4ace's units are the first
  builder's. (1) is #1822: the voice goes through `lamp-local-time`, with
  tests in New York across the March change and in Ho Chi Minh, and a probe.
  (2), Schedule's `--zone system`, comes next.
- lamp-c0a4 replaced lamp-cb94 (the person, 14:10). Its owner file is #1804,
  and the role file names it. The person said yes to lampOS: build 3162
  (08a90ec99) installed from a test-signed channel; V1 stopped and backed up;
  voice and eyes up; body waits for #1744's lamp.env; name has no models.
  Handed to Thirteen for #1744 with the person at the arm.
- #820 (2) is #1825: `lamp-schedule --zone system`. Its ARM64 failed once on a
  Linux-only case (jiff's `TimeZone::get` found `localtime` in the system's
  zone folder). The fix names the zone from the link or the name file, checked
  against the bundled zones. Green on the second run.
- #1822 shown on lamp-c0a4 with its own build: asked via the Mac's `say`, it
  told Saigon's time, then New York's after `set-timezone`, with no restart,
  then Saigon's again. c0a4 is back on 3162's voice.
- On c0a4: name's models installed (Thirteen's ask). The voice hit
  `quota_exceeded` and the person lifted the device token limit.
- #820 (3), Expression, was already on `lamp-local-time` with a New York test.
  What's left is units, which are the first builder's. Build 3265
  (`850fdfb68`) named for #1693 (4)'s scenes tonight.
- #1822 rebased over #1794's memory scope lease (a conflict in `remember.rs`)
  and merged (`e3134347a`). #1825 folded into one commit (`27e91fe74`) at the
  reviewer's ask. #1870 (draft): lamp-4ace's Policy, Schedule and Skills to
  `--zone system`; out of draft once #1825 is in. #1826 merged.
- #1825 merged; #1870 ready. #1693 (4)'s run sheet is on #1693 (build 3265,
  Live for scenes 4 and 6). twelve-P2 on #1722: memory's forget-all and
  threads switches reach only the Live worker, not the DeepSeek route. A yes
  to Tests on #1806.
- #1815 taken (the first builder, 19:05). It's #1909: `lamp-cloud` on in the
  product unit. The reviewer's P2 was that `Conflicts=` stops os-server even
  when lamp-cloud is skipped by its conditions; I showed both forms on c0a4
  with stand-in units, then fixed it with `ExecStartPre=-+systemctl stop`.
  Folded with the P3 line and rebased (`11f86c666`). `lamp-voice`'s head has a
  `lamp-cloud` compile error from #1728 (`UNITS` length), reported on #1722.
  One read-only ssh to c0a4 was made before the lock: logged, and Thirteen
  told.
- The owner's rule (19:38): voice tests on the second route. twelve-P2 is
  #1930 into #1712's head: forget-all and threads reach the second worker.
  #1693 (4)'s run sheet is rewritten for the second route.
- #1930 merged into #1712. #1909 rebased again after #1919; waits on ARM64.
- "Remembers what matters" (#1693 (2)) is #1974 onto #1712: the second
  route's DeepSeek proposes at a conversation's end, and the memory checks
  through `tidy_apply`. An empty apply now closes a job (memory). Behind
  `LAMP_MEMORY_OBSERVE`, off. ARM64 39279149488 dispatched. Next: the dry
  run against the bar.
- #1974 merged into #1712. #1909 merged. (4)'s dry run is #1988 onto #1712:
  `lamp-voice-eval observe-dry` on a copy of a store, through the lamp's own
  path, with counts printed and words only in a 0600 labels file.

## 2026-10-20 — #1988's P3s, the forget task

- #1988: both P3s folded (a partial copy removed, `chars().take(6)`).
- The forget task (the first builder, 23:57) is #2003: a kept thing goes with
  its conversation's earlier turns. Found: forgetting an observation climbs
  recursively through what sessions were shown and removes unrelated kept
  things. Shown by a test; left for the first builder's call.
- Memory on the second route keeps nothing (the first builder's 00:34): Third
  owns it. My FEW_TOOLS guess didn't fit (it was off); I read the functions'
  descriptions for Third (they never say that words alone keep nothing).
- #2003 also carries the observation rule, one step back, at the first
  builder's yes (`7996c39e7`): the pinned test is changed with its reason, and
  the kettle test is added.
- #2003 merged. Second's P2 (a stale `current` after a forget refused the
  next remember) is fixed in #2020, with the regression. The synthetic dry run
  (20 made-up conversations, scored against the bar) is #2021; the real
  DeepSeek run waits on where its key comes from. The thread trial row is
  designed and waits on Tests' yes; its proof line is #2023.
- #2020's P3 fixed (the writer ends its conversation behind its queue).
  #2021's command posted for the first builder's run on lamp-4ace through the
  relay. The thread trial row is #2029 on lamp-voice, with Tests' three
  changes; trials-test 283/0.
- #2020 merged. #2029: Tests' P2 (a timed known-file check) fixed. #2021: the
  peer read's three P3s fixed (one-for-one cut, whole words, each thing once).
- #2029 folded and rebased (light-show row). #2021: `passes` renamed to the
  wrong-and-intrusive half, with `missed_of_worth`. Memory's quality on the
  cheaper voice is #2037: six probe-tools cases, counts only.
- #2029 and #2021 merged; #1973 landed the second route on lamp-voice.
  #2037: the peer read's P2 (another person's room secret never repeated)
  and P3s (never-mind keeps nothing) fixed, with a probe.
- #2023 merged (Jarvis's P3s: doc placement, an emission count test; "told"
  kept for trials.sh). #2037 rebased over #2035, no finding, merges on green.
  D, the Days session leak (everyone at 32, no note on lamp-4ace), is #2049:
  oldest closed at the limit, and each worker closes its own at Stop.
- #2037's failures on lamp-4ace read case by case: preference-used is two
  notes saying recall for earlier talk; the corrections and
  earlier-today-said are left out of the prompt; line 5 is likely the bar.
  #2063 adds a `quality-fix` arm (`MEMORY_USE_NOTE`) and why each wrong run
  was wrong, counts only. #2074 fixes trials-test's flaky thread case: the
  timed proof line came before the trial's mark when a set-up ran long.
  #2049 rebased onto #1712's compiling head.
- #2063's first fix arm on lamp-4ace: correction-kept 8/8, correction-first
  5/9, earlier-today-said 4/8. Tightened at `74341122c`: a correction's
  first call is forget, the room's never repeated or hinted at; line 5 right
  answered from the talk. #2049 rebased onto `bebea8708` (lamp-voice
  2290/0). #2074 rebased over #2042, one `after_mark` wait, Tests' P3; 495/0.
- #2049 and #2074 merged, ARM64 green. #2063 at `74341122c` on lamp-4ace:
  corrections 10/10, but preference-used 5/9 and earlier-today-said 2/10
  (the room's secret said). Folded to `923c56816`: "only theirs" is about
  recall's things, the snapshot's own word, and a `quality-marked` arm with
  the room's things marked on themselves. #1712's `6ea61281d` doesn't
  compile (`Heard` twice, from `22b383c82`); told on the Queue.
- #2063's next head `19096d05d` (the first builder, 09:49): today's recall
  quotes their own words as `said_today` ("nothing said today" when none),
  answered word for word; the preference line opens the memory note. One
  commit on #1712's `61b690638`; probe 47/0.
- #2063 rebased onto `9519328f7` (`faeb02d25`), then `678a39837`: the
  preference line's docs say where it is (first in the memory section,
  after the correction lines). c0a4: pins head-2 96 → 97 (Thirteen's
  find); the installed lamp-controls still doesn't see the pads, asked
  about a newer install. Fourth's held hand-on to memory: yes, with three
  conditions (under 45 s, context at turn end, "forget that" ordering).
- The memory side of #2145's held hand-on (the first builder's yes, 11:00):
  `remember::hold` keeps the turn end's sources, rules and time; a forget
  is counted before it reaches the store and the writer never writes an
  exchange made before one; a stop hands held answers on before the session
  closes. `e31557c7d` on `twelve/heard-to-memory`, 4 tests each with a
  mutation; lamp-voice green. The PR waits for Fourth's folded heads.
- #2186 opened onto #1712 (stacked on #2173 and Fourth's `410a563cf`): the
  held hand-on's memory side. #2063's next head `cdb21e020`: the preference
  line first in the use note.
- #2186: the reviewer's read, no P1/P2; P3s fixed (`b81da41ab`): `hold`
  takes the context lock, the forget count's reach said in memory-days.md.
  The fold into one waits for Fourth's yes.
- #2186 folded with Fourth's yes (`29863310d`, ARM64 green), for the first
  builder's merge. #2226: #2176 rows 1b/c onto lamp-voice, stacked on
  Friday's #2206: journalling resumes with a fresh session
  (`LAMP_MEMORY_JOURNAL_RESUMES`, off); known writes always settle.
- #2186 rebased onto #1712's `c03e4dfc4` (`61d009897`), past #2136's flag.
  #2226: Friday's P1/P2/P3 fixed (`5ac6d5839`): a fresh session is asked
  for after a stop on both routes; only lost writes settle; rules settle.
  Took #2176 row 5b.
- #2226: ARM64 caught lamp-memory's malformed rule acknowledgement test; an
  ok that is no acknowledgement stays uncertain again. #2265: #2176 row 5b,
  a known face lost a few seconds with no face in view keeps its memory
  scope (`LAMP_FACE_GAP_HOLD`, off), tested through the request's lease.
- #2186 merged into #1712.
- #2265: the reviewer's three P3s (the closed camera ends a hold by the
  look's expiry, the generation guard tested, the row in order); merges on
  its run. #2226: Friday's re-read: both routes' choices tested through
  `opens_anew`/`presents_resumption` in child processes, Live drops its
  resumption only after a stop, a failed fresh open backs off 30 s.
- #2226: Friday's blocking P2 fixed (Live drops its resumption after a stop
  whatever the backoff); #2176 row 13 in (conversation_end passes over ids
  the store no longer has, counted). #2265: the reviewer's P2 (way out 1,
  held only for someone the lamp was answering) and P3s; the reviewer-lead's
  doc fix. Both say Part of #2176.
- #2226 rebased onto lamp-voice `cdc734188` past RECALL_V2/KEEP_HEARD; the
  reviewer-lead's P3: another person's ids still refused on conversation_end.
  Status table to the first builder on #1722; free: twelve.
- 21 Oct: back after a usage block and a restart (cron recreated). #2265
  merged; #2449 onto #1712: its follow-up (a gap-held line told no recall,
  no hits, no held forget), with #2265 cherry-picked. Friday's #2367 took
  ROOM_NOT_ASKER; my local start dropped.
- #2449: the reviewer's P2 (a gap-held line runs no memory/rule/face
  function and is told no note). #2226: the reviewer-lead's P1 (my fold had
  undone #2443's thread pin) rebuilt on lamp-voice as `04bc8d040`, P3s in.
  #2063 folded. Status table to the first builder; free: twelve.
- #2063 merged into #1712; its worktree removed.
- #2226 merged into lamp-voice as #2491 (the reviewer-lead's rebase); its
  missing test is #2494. #2226 shown on lamp-c0a4's Live route: forget →
  journal stopped → a fresh session 3 min later on a reconnect → resumed →
  next exchange journalled. Evidence on #2491; c0a4 put back.
- #2494 merged into lamp-voice (bd43346de); its worktree removed. c0a4 rows
  booked 15:45–17:30 on #1722, after Thirteen.
- New tasks (the first builder, 13:1x): #2527 LAMP_SECOND_QUIET_HELD onto
  lamp-voice; forget_person + face.remove entirely built on #1712 as
  twelve/forget-person (PR after the landing); #1608 answered (Fourth took it).
  #2449 on c0a4 with the person: never held; journal line + replay test
  pushed as c07632f17, retest when ARM64 is in.
- 21 Oct evening: #2616 (gap hold, 12 s from the answer end, told only) merged after c0a4 Mac passes with a screen face; #2563 (forget_person) merged; #2588 (journal by scope) and #2619 (by name only) open; c0a4 gaze measured 750,543 into lamp.env; #1693 first piece claimed.
- 21 Oct night: #2648 (LAMP_MEMORY_EVENTS) merged; #2652 (quiet-held switch reaches the worker) merged; #2672 (held line kept out, grants nothing) read clean; #2658 (dialogue socket) P2 closed; c0a4 Mac runs: #2527 A/B 2300 ms held, first audio not later; #2672 held line journalled nowhere; #2648 event_days 2026-10-24 stored.
- 22 Oct 02:1x–02:3x, c0a4 Mac runs on local build 76edecdcf: the test relay's DeepSeek base brings the chat's first event from 2147 to 1693 ms p50 (12 played lines per arm); the dialogue socket probe passes 5 of 5 on v4 (upgrade p50 481 ms) and on flash. Posted on #2602; c0a4 put back.
- 22 Oct 03:4x–04:1x: #2729 (draft, line-start speech socket, measured dialogue bound) run on c0a4 in three arms: the socket always ready, but sound p50 3424 (flash) and 3957 (v4) vs 3325 over HTTP; #2727 (probe asks for the socket's own model) opened.
- 22 Oct 04:xx: #2727 merged; #2729 closed on the first builder's call (socket off), recorded in #2734 (docs); #2735 day-stages --lines (split-line count), on c0a4 5 of 12 played lines heard as 2 turns, all fragments silence_fallback.
- 22 Oct 05:xx–06:3x: turn-end follow-up: short = the 0.8 s speech floor (+ start blocks uncounted); #2751 LAMP_ENDPOINT_LEAST_SPEECH_MS + speech_ms; replay 72 clips, c0a4 A/B wait p50 602 → 438 ms. #2707 closed.
- 22 Oct 06:xx–07:1x: #2751 merged (before the P2 fix) → #2756 (floor at least 500 + Hey Lamp test); #2619 P2 fixed, rebased, Reviewer-lead clear, waits on Mobile.
- 22 Oct 07:3x–08:2x: lamp-runs rows on c0a4 (Lead 07:3x): #2527 QUIET_HELD passed (held 250–450 ms/line, words p50 150–200 ms sooner); #1693 LAMP_MEMORY_EVENTS: kept with its day, told nothing before, once after, nothing again; the model didn't ask in 3 turns (half passed).
- 22 Oct 10:xx–12:2x: #2812 (late name sign test) read clean; #1870 merged; #2805 → #2894 (note heading, mark on ask, 3-offer cap), #2806 on c0a4: memory side passes, model still didn't ask in 3 turns; claimed all builder:twelve (owner): #2877, #1608, #2844 with sub-agents, #2850 run pending.
- 22 Oct 12:3x–14:5x: #2849 passed on c0a4 (forget everything); #2850 failed on c0a4 (voice sends wrong name; set-aside file blocks) → fix built; #2930 (#2877 short line) and #2932 (#1608 workers SCHED_OTHER) opened with c0a4 runs; #2844 seat bar built; #2894 fixes in progress.
- 22 Oct 15:xx–17:5x: #2850 rerun passed on c0a4 → #2972; #2894 reads fixed; #2932 closed (LAMP_WORKER_SCHEDULING exists); #2974 product turn-end units; #2985 seat bar (c0a4 can't measure: lamp-name deaf to synthetic calls); room-forget follow-up and #2930 P2 in progress.
- 22 Oct 18:xx–22:4x: #3008 merged (#2850); #3032 merged, El's/Reviewer-lead's/Fourth's reads → #3038 (LAMP_EYES_SPEAKING pinned in product voice unit, boot strict); #2805 greeting-turn line #3059: c0a4 offer taken 3/10 (was 0); #2985 c0a4 can't measure (lamp-name deaf to synthetic calls).
- 22 Oct 22:4x – 23 Oct 01:0x: #3059 greeting-only line: reviewer-two's P2s fixed (ded4bda42, 16b433ac4 one-word requests, df1182337 docs P3), both reviewers clean, in train 3 (#3090); c0a4 on 16b433ac4: greeting alone asked 8/10, greeting + request asked 0/9 heard, time answered, offer taken back.
- 23 Oct 01:0x: #3059 merged in train #3090 (lamp-voice c7003f01d); #2805 summary posted for the first builder; merrow-eventturn worktree and target removed.
- 23 Oct 01:2x–02:0x: #2844 on c0a4: walk stalled because c0a4 lacks the onboarding skill (old lamp-skills); with both installed for the run, seat sentences rose 10.7/10.8/11.0 dB (bar unset, 3 dB floor), one voice's calls ~3.3 dB; real voices + across-room still for lamp-4ace; #2985 Not-verified corrected.
- 23 Oct 02:3x–03:0x: S8/R1 cause posted (#2602): R1 the Mac's voice fetch failed (nothing played, mic p99 -39.6 dB), S8 captured but speech to text returned 0 words; driver now checks each line played; #2844 walk's 3 test kept lines forgotten on c0a4.
- 23 Oct 02:3x–03:3x: status table posted (#2805 closed, #2136 dropped); #2930 rebased onto 001a3d714 → a35eb9803 (gate 3490/0); #1693 end-to-end run on c0a4: keep, recall in scope, dated follow-up once, forget-everything pass; forget-one-named-thing fails → proposal #3113.
- 23 Oct 03:4x: #2930 merged in train #3120 (lamp-voice 7d929b92e); merrow-short-done removed; #2849's find: First's yes to forget_naming (1) and LAMP_MEMORY_ROOM_AFTER_FORGET (2), both coding with sub-agents.
- 23 Oct 03:3x–06:3x: #2849's find: #3143 forget_naming and #3142 LAMP_MEMORY_ROOM_AFTER_FORGET (base of Seventh's #3138), both passed on c0a4; #3138 stack checked; Nine's #3107 row to run after merges.
- 23 Oct 06:3x–10:0x: #3142 merged (ROOM_AFTER_FORGET with the zero-removal fence, your/yours words, the forgetter's own name); #3143 closed on First's call (names can't be told from words in Vietnamese); #3113 closed on the c0a4 recheck; #2985 closed (real seat voices with the Lead); next: the speaker on room lines (#2849 design approved 09:37), building.
- 23 Oct 10:0x–13:1x: #3213 room speaker (LAMP_MEMORY_ROOM_SPEAKER) opened, c0a4 run (schema on the live store; screen faces never seen speaking, steps for lamp-4ace posted); reviewer-three's P2 (forget_person kept speaker rows) came in as the fixing sub-agent stopped on the usage limit.
- 24–26 Oct: away; the person's stand-in posted the disposition ask for #2877, #2844, #1693 on #3504 (26 Oct 08:08); Fourth took #3530 4k (forget everything keeps the face/name).
- 27 Oct 03:1x UTC: #3213 closed as landed (661c2f4f0 feature, 0b86db80e privacy fix by reviewer-three, both on lamp-voice); merrow-speaker worktree and target removed; waiting on First's disposition for the three taken issues.
- 27 Oct 12:3x–13:3x (+07): First assigned #3703 (from my install note); #3728 opened (install and check-units name a kept off-switch drop-in or edited setting, LAMP_CLOUD first; Mac gate green). c0a4 read-only: its own LAMP_CLOUD-off case was a stale 19 Oct base unit, not a drop-in. #2877 closed by First on Fifth's row 24 (off already short); the repeat fix is set aside, its worktree removed. #1693 moved to Friday; #2844 waits for the lead to name a person and lamp.
- 27 Oct 13:4x–16:1x (+07): #3728 merged (81572a373); its P3s (systemd merge order, a file read twice) in #3788 on lamp-voice 40dbca3e2, Mac gate green; #3733 (install leaving older base units, c0a4's lamp-cloud.service) assigned by First, a sub-agent reproducing from a 19 Oct tree. Onboarding PRs go first for the 18:00 promote.
- 27 Oct 18:0x–20:3x (+07): #3788 merged in train 227 (184532c6c), its worktree removed; #3733 items 1–3 as #3846 (units not written on an early end, check-units names a stale base unit; 19 Oct fixture), Mac gate green; root cause (an old lamp-update writes its own built-in units) waits for First's (a)/(b).
