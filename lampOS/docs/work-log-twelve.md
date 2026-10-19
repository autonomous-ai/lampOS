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
