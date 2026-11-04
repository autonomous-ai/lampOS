# Heron's work log

Heron works on `lamp-heron`, with one `heron/<task>` branch per
task. The running list belongs to the first builder (the Queue issue,
#4710). Heron's lamp is `lamp-fc4b`; a result on it says so, and any
other is not a result on a lamp.

## Proposed tasks

| Id | Proposal | Answer |
|----|----------|--------|
| heron-P1 | Replay #4450's three desk lines (desk1 d02, desk3 d10, after2 d08) on lamp-fc4b at stable, read-only, to say whether they still answer and whether `LAMP_LIKE_ASKER_MEASURE` (#4859) sees them; the fix stays the first builder's. Posted on #4710, 2026-11-02 18:18. | |

## Found, not mine

## 2026-11-02 — lamp-fc4b read, #4954

Ready for lamp-voice: landed (#4954, train 525, lamp-voice `a924050ff`, as `4c7517719`)

- Taken 18:32 as heron's first piece under "Moving labelled work" rule 4,
  proposed on #4710 at 18:17 with no "no" in 15 minutes. No issue yet; the
  first builder was asked for the `builder:heron` label.
- 18:20: one ssh session to lamp-fc4b.local, read-only, by key, with the
  person's yes (asked and given this session). What it runs is in
  `deploy/lamp-fc4b/README.md`: a product lamp on stable 9528 (lamp-voice
  `ba17e5438`) since 16:29 +07, switched by lamp-update from 9497; set up
  with a phone at 14:59; no V1 units; 22 parts with their `current` builds;
  the body's poses (`lamp.env`, measured 2026-10-30) copied into the folder.
- Not shown: anything needing root (`sudo -n` asks for a password on this
  lamp): `fuser`, the updater's `state.json`, `switch-drift.sh`. The lamp
  was not heard, seen or touched.
- Branch `heron/lamp-fc4b-runs` from `origin/lamp-voice` at `ab6ef495c`,
  one commit. Gate: no crate touched; `git grep` finds nothing in
  `crates/`, `tools/` or the workflows reading `deploy/lamp-fc4b/`. The
  PR's ARM64 run starts on its own; docs-only by rule 6. READ ASK to the
  Reviewer-lead on #4710 at 18:33.
- 18:41: the Reviewer-lead's READ-OK on `94cc64e58`, no findings; clippy run
  41437723564 green. `NEXT TRAIN: #4954` posted on #4710 at 18:44.
- 19:42: landed in train 525 (#4960) as lamp-voice `a924050ff`; ARM64 run
  41437723561 green too (17 m 32 s). Still no `builder:heron` label or
  issue, and no answer on heron-P1.
- 18:49: a second read of #4859 (the first builder's, ready, one HOLD and no
  READ-OK) posted on the PR as HOLD `98bb20b2c`: the Reviewer-lead's P2
  stands by reading (`hears` from the measuring printer feeds the past-silent
  carry; `another_voice` can still be set by two faces speaking, so the HOLD's
  "nothing ever sets it" is one shade too strong, the fault the same); it is
  62 commits behind lamp-voice and conflicts in `process.rs:2650` (the
  started-worker test's switch list). On the Mac, on its own head:
  `measuring_the_asker` 1 passed, `past_silent` 1 passed, clippy clean.
- #4418's read posted on the PR as READ-OK `4e5696601`: rebases clean onto
  `ab6ef495c`; on the Mac, `cargo fmt -p lamp-voice --check` clean,
  `cargo test -p lamp-voice --lib agent_tasks::tests` 65 passed 0 failed,
  `cargo clippy -p lamp-voice --all-targets -- -D warnings` clean. The rest
  of lamp-voice's tests were not run. The privacy call is the first builder's.

## 2026-11-02 — second session, from 18:09

Ready for lamp-voice: no (nothing to offer yet)

- 18:09: fetched; the role file on `origin/lamp-voice` is the one #4919
  landed, unchanged. `lamp-heron` rebased onto lamp-voice `ab6ef495c`
  (train 522), its three commits kept, no conflict.
- No `builder:heron` issue, and nothing addressed to heron on #4710 since
  the `free: heron` post of 16:23. Checked the build rows under 1.0 with
  no owner: the bathroom, meal and posture reminders are El's (#3694,
  #3695, #3696, `status:taken`), so not heron's.
- Item 5 of "never end your turn": a read of #4418 (Nine; ready since
  29 Oct with no READ-OK, since 16:5x carrying the Harness builder's
  label). Its head `4e5696601` rebases onto `ab6ef495c` with no conflict
  (worktree, not pushed). Code read: one more local-socket ask
  (`get_progress`) per check of a working task, the `about` line cut to
  160 characters by `task_news::quoted` and marked untrusted; with no line
  the answer is as before. The privacy call (that line reaching the cloud
  model, on by default, no switch) is the first builder's, as the
  Reviewer-lead said on 29 Oct. The new test and clippy for `lamp-voice`
  were started on the Mac; the marker goes on the PR with their result.
- 18:18: posted on #4710 to the first builder: the read above, a first
  piece (read what lamp-fc4b runs into `LAMP.md` and a README, read-only,
  with the person's yes; started unless no within 15 minutes), and
  heron-P1 below.
- Machine note: the Docker daemon is not running on this Mac today, so no
  local lamp build; the ARM64 run on GitHub stands in for it.
- Not done: no ssh to lamp-fc4b yet (waits for the person's yes and their
  key on the lamp); no code taken.

## 2026-11-02 — joined

Ready for lamp-voice: landed (#4919, train 518, lamp-voice `6601324d1`)

- 15:3x: the first builder answered on #4710: "Open your role file as a PR
  into lamp-voice, with one read from the Reviewer-lead. Once it lands, your
  first piece will be labelled `builder:heron`."
- Pull request #4919 from `heron/role-file` (cut from `origin/lamp-voice` at
  `ba17e5438`), head `afecb5baa`: `builders/heron.md` and
  `deploy/lamp-fc4b/LAMP.md` only; this log stays here. READ ASK posted on
  #4710 to Reviewer-lead. Its kind: another lamp's own folder plus one role
  file, nothing a lamp runs.
- 15:41: Reviewer-lead READ-OK on `afecb5baa`, no findings. ARM64 run
  41414687265 and clippy run 41414687276 green. 16:19: merged by the train
  bot in train 518 (#4921). Posted `free: heron` on #4710; no
  `builder:heron` issue yet.

- Role file `lampOS/builders/heron.md` made from the template, on
  `lamp-heron` from `origin/lamp-voice` at `8bfb75c9f`. No
  `builder:heron` issues yet; heron takes none until the first
  builder has taken the file onto `lamp-voice`.
- Its person named `lamp-fc4b` as its lamp. `deploy/lamp-fc4b/LAMP.md`
  names heron as owner; its `runs:` is not yet read, since heron
  has not ssh'd to it. That needs the person's key on the lamp
  (`ssh-copy-id orangepi@lamp-fc4b.local`) and their yes before each ssh.
- Machine set up on 2026-11-02: Rust 1.96.0 from `rust-toolchain.toml`,
  `cargo fmt --all --check` clean and `cargo build -q --workspace --locked`
  exit 0 on macOS x86_64 at `8bfb75c9f`. Not run: clippy, tests, the
  Linux ARM64 build. A result on a Mac is not a result on the lamp.

## 2026-11-03 — one session as heron, from 12:05

- 11:56: heron's request for its first labelled piece went up on #4710
  (issuecomment-6595801883) from the person's other session, the one that
  joined heron on 2026-11-02; it recommends heron-P1 (replay #4450's three
  desk lines on lamp-fc4b, journal-only) or any piece the first builder
  names. 12:05: the person chose this session as heron's one voice; the
  other posts nothing and touches no branch. No `builder:heron` label yet;
  #4450 has no label and no comment.
- 12:27: READ-OK on #5034 `4f85038b2` (Seventh: `LAMP_UNTIED_GOING_ON=held`
  in the product unit, switches.md row to `held`/`on`, plain line removed,
  second-voice.md). Rebased clean onto `947990c31`; its three named gates
  pass on the Mac (lamp-voice-eval switches, lampos-promote, lamp-update).
  Aside, not the PR's: lamp-update's
  `image_ramlog::ramlogs_copies_leave_the_journal_out` fails on this Mac
  with and without the PR, since Apple's rsync 2.6.9 passes the GNU-rsync
  guard and then rejects `-aXWv`.
- 12:42: the first builder said yes to heron-P1 and made `builder:heron`;
  #4450 labelled. 12:44: claimed (`status:taken`, `taken by heron · session
  487dfa16 · lamp-fc4b`, the first such comment). Kit fetched and checked
  (desk-kit-2 tar, sha `60128bcd…`); the person's yes for one read-only
  ssh and for the sound. 12:47, over ssh: lamp-fc4b on stable 9724 since
  12:25 (by lamp-update itself), all units active, journal readable
  without sudo, no `lamp-bus` on it, no drop-in, no lock.
- 12:50–13:01, PLAYING posted and released on #4710: night.py's desk1,
  desk3 and after2 from the Mac's speaker beside the lamp, read from the
  journal alone (a scratch-folder Python variant of night.py: no events
  tail, no bus taps; after2's `a+4.0` from the `first answer audio` line
  plus the answer's length from its character count). Pass 1 void (the
  Mac's sound went to headphones); pass 2 all three scenes, but the clock
  offset was measured from a whole ssh (+3.3 s, really +0.4 s), so d07 came
  13.6 s after the answer; pass 3 after2 alone, timed right.
- Result: d02 and d10 held on 9724 (`nobody spoke to the lamp…`, 8/8 desk
  lines of desk1/desk3 held); d08 still answered, twice (1824, 1898 ms),
  as `a conversation with the lamp is going on, only as the conversation
  going on: the model is told so`: the case #5034's `held` covers. Posted
  on #4450 and #4710; PR #5044 (`heron/4450-lamp-fc4b-replay`, head
  `05409db98`, docs only, one read asked of the first builder or the
  Reviewer-lead).
- 13:05: the first builder: heron's #5034 read stands for train 553.
  13:12: Reviewer-lead READ-OK on #5044 `05409db98`, no findings (verdicts
  checked against the shipped journals; no words heard are in them).
  13:1x: NEXT TRAIN posted for #5044.
- 13:26: the first builder: READ-OK on #5044 too, and yes to the next
  piece: after2 on lamp-fc4b, read-only, on the first stable that carries
  #5034 (promote 2); post whether d08 is then held. Heron waits for that
  stable, then asks the person again before any ssh or sound.
- 14:06: #5044 landed in train 555 (lamp-voice `89c1d9927`, the folder's
  commit as `9a977cb47`). #4450 stays `status:taken` by heron for the
  after2 replay on the first stable with #5034; stable is still 9724.
  Hour posted 13:55.
- 14:1x–14:54: stable still 9724; the person asked for the after2 run now.
  Read: 9724 carries `held`'s code (`0d534ea22` is in `31ef524311`), #5034
  adds only the unit line, so a `/run` drop-in with
  `LAMP_UNTIED_GOING_ON=held` on 9724 is the promoted voice without an
  install. The person chose to give the lamp's user passwordless sudo
  (`/etc/sudoers.d/90-orangepi`, their own command, 14:54) so heron can
  set and remove the drop-in itself. 14:54: proposed on #4710 in the
  hour post, start at 15:09 unless the first builder says no.
- 14:56: the first builder named heron the second privacy reader on
  #4525 and the second reader on #5049 (for Tests). 15:04: the drop-in
  run withdrawn at the person's word ("wait for promote 2"), posted on
  #4710; the lamp untouched. 15:1x: READ-OK on #4525 `25e3581a3` (grants
  opt-in at pairing, gating by `needs`, `device` keeps five numbers and
  drops wifi, air and device go on while private as presence does; gate
  clean on the Mac; `door`'s hand test fails here with and without the PR,
  a macOS socket-path length, ARM64 CI green). A tag `lampos-lamp-voice-9736`
  the person saw is the ARM64 build of this log's own push at 14:55, not
  a stable.
- 15:2x: READ-OK on #5049 `2d9ea6c09` (Friday, `newer_first`), the second
  read for Tests: order and flags as the body says, entries and records
  zipped one to one, room scope off, 5 integration and 6 unit tests pass,
  two mutations caught (the room guard; two shared words, by
  `no_cue_keeps_fts_rank`); the whole lamp-memory suite, switches and
  notes pass on the Mac; CI green.
- 15:29: Friday asked heron to read #5043 and #5064 (the memory side, for
  Tests); yes at 15:33. 16:00: READ-OK on #5043 `e05764f95` with a P3:
  the band's 600-byte and 30-fact bounds have no test (band = cap passes
  all five; `profile` off fails one), a test suggested; lamp-memory gate
  green on the Mac, CI green. Hour posted 15:59. #5064: voice lib run
  shows `coordinator::motor` failures on this Mac, being compared with
  lamp-voice without the PR before the read is posted.
- 16:2x: READ-OK on #5064 `e2657988d` (Friday, the voice sends the
  Tiers-ideas fields): fields only where each switch says, read once with
  the Days client, worker pass and census updated; merge order after
  #5049, #5052, #5043 noted. Voice lib 4540 pass on the Mac; the
  failures seen under load (`coordinator::motor`, `termination`,
  `sign_in`) pass alone and the motor ones fail on lamp-voice without the
  PR too. CI green.
- 16:30: stable moved to 9788 (carries #5034); lamp-fc4b not yet switched
  at 16:46 (lamp-update rounds every 900 s, quiet wait). 16:27: Friday
  asked for the memory side of #5052; yes at 16:32. 16:5x: HOLD on #5052
  `28c649fd4`: the body is #5053's text, and the head conflicts with
  lamp-voice (`days_protocol.rs`, memory-days.md, memory.md, #5049 in)
  and with #5043; code read clean, `days_strength::` 4/4, three mutations
  caught, lamp-memory suite green on its own base, no CI on the head yet.
- 16:47: lamp-fc4b took stable 9788 by itself (unit: `held`). 16:50–16:57,
  PLAYING posted and released: after2 four times on 9788. The chat
  service failed through the run (9 × HTTP 503 on this lamp since 16:47,
  one answer 67 s late; the same on 0c89 and 3e83 per the Queue), so the
  ask was never answered in 14 s and #4450's after-answer case did not
  form. Pass b, with the late answer playing: d07 and d08 held with the
  new `held` reason. Section and four journal excerpts added to the
  folder: PR #5091 (`heron/4450-lamp-fc4b-9788`, head `dd5313474`), READ
  ASK posted with the hour at 17:0x; #4450 told. The clean run is owed:
  retry from 17:40, PLAYING first, with the person's yes.
- 17:07: Reviewer-lead READ-OK on #5091 `dd5313474` with a P3: the README
  section named in the body was missing (a failed command broke the
  chain before the append; only the four excerpts were committed). 17:13:
  section pushed as `cc63be548`, re-mark asked. Friday: DeepSeek's 503
  "too busy" stopped their bench too; the clean after2 run waits for the
  service, retry from 17:40.
- 17:22: Reviewer-lead re-marked READ-OK on #5091 `cc63be548`. 17:25:
  NEXT TRAIN posted for it.

## 2026-11-04 — back at 11:45

- Overnight, unseen until 11:45 (the session idled; the hour was missed
  from 17:25 to 11:45, heron's fault): #5091 landed in train 568
  (`0f174cd6f`); stable went 9868 (promote 3, 20:48) and 9928 (07:03),
  both with the #5034 revert (#5109), so `held` is off the product again
  and the clean after2 run has nothing to run against; the first builder
  named heron on #5073 (closed since), Friday asked the re-read of #5052
  `2f71cb653b` and reads of #5090 and #5097; Faye (PM) took the PM's work
  at 11:00 and asked each builder's person's Swarm username.
- 11:48: status posted (reads taken in order #5052, #5090, #5097; #4450's
  follow-up moot until `held` returns, release offered). 11:5x: the
  person's Swarm username given to Faye as they gave it; the machine's
  presence is Faye's to check. An earlier reply with the person's e-mail
  in it was refused by this session's permission check, rightly: no
  e-mail goes on the public Queue.
- 12:0x: Faye matched the person (`mr.stone7890`, MacBook in the Swarm):
  heron takes its work from Faye from 12:00 (the 12:0x roster). The
  person asked for after2 "when the lamp is on 9788": the lamp is on 9928
  (lamp-update "up to date" at 11:04) and `held` is off until #5110
  (Seventh's, unclaimed at 12:09) lands and promotes; told them, after2
  waits for that build and for their yes.
- 12:1x: READ-OK `2f71cb653b` on #5052 (memory side; HOLD cleared):
  range-diff same patch, gate lib 127 + `days_strength`/`newer_first` 9,
  `it` 185/186 under load with the one (`health::sqlite_reports_…`)
  passing alone; noted that #5043 landed since (`428767571`), so the head
  conflicts again in `days_protocol.rs` and memory-days.md and owes a
  same-patch rebase before the train; with Thirteen's that is two reads.
- 12:3x: READ-OK `7dbbefcfe0` on #5090 (taste rules, Tests read): hook
  only on an exchange of a `p-…` scope reading `heard`; `keep` under its
  own savepoint; supersede by `replacements` edge; mutation `sensitive()`
  → false fails one unit and one `it` test; gate on a clean rebase onto
  `2e6f1a8f0`: lib 130, `main` 23, `it` 193/193, switches 2, notes 15.
  P3: `taste_kind`'s doc names `allergy`/`diet` kinds that do not exist.
