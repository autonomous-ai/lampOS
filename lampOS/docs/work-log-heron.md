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
- 12:4x: the hour posted to Faye. Faye told every builder Reviewer-two and
  -three have posted nothing since 1 Nov; second reads go to the
  Reviewer-lead, Thirteen, El, Friday or Heron.
- 13:0x: READ-OK `cdc5c3e7ca` on #5097 (nightly B, Tests read): per-item
  drops with reasons, `private()` backstop unless every source is an
  explicit keep, profile items reach #5043's band through `category_from`
  → plain fact, forget cascades through `with_doomed`; mutation (backstop
  off) fails two `nightly::tests`; gate on its own base lib 130, `main`
  23, `it` 177/177, switches 2, notes 15. P3: no `it` round-trip of the
  nightly ops. Like #5052 it conflicts with lamp-voice since #5043
  (`days_protocol.rs`, memory-days_vi.md); the three memory PRs all owe
  rebases in turn. Worktrees removed; all three reads Friday asked done.
- 12:5x: stable 9980 (promote 12:35) still lists `held` under "built but
  not on". Friday asked restates after the #5043 rebase: #5052
  `0a932712c2` and #5097 `de822e4c0d`; range-diffs show only the merged
  field lines (`session_open_with(…, profile)`; the known-fields list).
- 13:1x: READ-OK `0a932712c2` restated on #5052 (lib 127, `it` 191/191);
  Friday put it on NEXT TRAIN at 13:16 with Thirteen's.
- 13:25: the Reviewer-lead HOLDs #5097 `de822e4c0d`: P2 `explicit()` is
  `.any`, so one explicit keep exempts a whole episode from `private()`
  (and `privacy()` reads only topic/gist/feeling, not the exchanges);
  P2 the 512-item cap counts superseded rows and never frees; P2
  duplicate switches.md rows after d8425a1a30; P3 quotes with no expiry.
  My Tests READ-OK had said "every cited source": wrong, the test is
  one-source so the mutation could not see it. 13:3x: said so on the PR,
  withdrew that line, asked a two-source `private_kind` test on the FIXED
  head; my gate on `de822e4c0d` stopped. 13:4x: the hour posted to Faye.
- 14:1x: Faye's 14:1x list: #5159 (Thirteen) owed heron's second read since
  12:34, asked on the PR, which heron's watch (Queue only) missed; taken on
  the PR, due 15:00. The person asked why after2 waits for a stable with
  `held` rather than a local build: no build is needed, the switch's code
  is in 9980 and only its unit line went; they said yes to one after2 run
  with `held` set by a drop-in (`heron-held.conf`), put back after; the
  heads-up went to Faye at 14:1x for a slot at about 15:05.
- 14:2x: READ-OK `594db1fdc2` on #5159 (voice side): `at_setup` only for
  a checked `remember_my_face` while "who you are" is open; said back
  outside the face question and a plain yes; spelled only after the lamp
  asked and as many runs as the name has words; mutation on `said_back`
  caught by El's HOLD test; gate fmt/clippy clean, `known_faces` 37,
  lib 4641/4645 with the 4 `ring_mark` fails Mac-only (long TMPDIR,
  untouched file), switches 2, notes 15. P3: `SAID_EXCHANGES` is 2, the
  usual walk fills it exactly, one exchange more costs a second read-back.
- 14:2x–14:3x: #4450's after2 on lamp-fc4b at 9980 with `held` by the
  drop-in `heron-held.conf` (on 14:26:13, off 14:28:08; on 14:29:20, off
  14:31:48; the lamp as found). PLAYING and released on #4710. Pass a
  (14:27:06, 53 s after the restart) void: a01 raised no request, lamp-name
  `heard=0`. Pass b (14:30:51, 90 s after the restart): a01 answered, first
  audio 2257 ms; d07 let through on one shared word ("time") and kept out
  by the model's `not_for_me`; d08 held by the untied rule's own reason.
  Posted on #4450 at 14:3x with the one-shared-word data point for #5110.
  Evidence PR #5217 (docs only, one read) opened from
  `heron/4450-9980-held`; READ ASK to Faye/Thirteen at 14:4x. The replay
  script's own request attribution was off by one for d07/d08 here; the
  README says the journal's numbers are the ones used.
- 14:36–14:40: Thirteen read #5217 (READ-OK `f61a6a27af`, two P3s on
  wording: the third restart between the passes, the one-word tie
  sentence); fixed in the README only, `abbd70139d`, restated at 14:40.
  14:4x: NEXT TRAIN posted to Faye.
- 14:5x–15:1x: Friday's FIXED on #5090 (`9e3dbabc1d`) after Thirteen's
  13:06 HOLD, which heron's watch had missed (and whose P2, another
  person through `sensitive()` by a possessive, a bare name or reported
  speech, heron's own read had missed too): read the delta (`split` with
  case kept, `names_someone`, `reported`, the "'s" strip, `object`'s
  cuts), gate lib 130 / `main` 9 / `it` 193 / switches 2 / notes 15,
  mutations on `names_someone` and `reported` each caught by the no-keep
  test; READ-OK restated at 15:1x with the miss said. Thirteen lifted the
  HOLD at 15:02, so #5090 has its two reads.
- 15:2x: Faye placed #5166's second read (names side, El's released) on
  heron, by 17:30; taken on the PR at `7037b11e47`, gate running.
  15:33: #5217 landed in train 602 (lamp-voice `3e0b88251`): the 9980
  `held` run is in `docs/verification/4450-lamp-fc4b-20261103/`.
- 15:37–16:3x: Friday's FIXED on #5097 (`0fcd92b262`: `explicit()` `.all`
  over a non-empty cite list, the 512 cap counts live items, switches.md
  rows deduplicated, two new tests); gate lib 138 / `main` 8 / `it` 187 /
  switches 2 / notes 15; mutations (`.any` back; the privacy drop off)
  fail the new two-source test and the two older ones; READ-OK restated
  16:3x. 16:09: Friday's NEXT TRAIN on #5090 `5b3e198fca`, heron's read
  carried. 15:59: #5166's full lamp-voice gate on `7037b11e47` passed
  (4684, short TMPDIR); Thirteen's rebase `0b9fef6b8a` is the same patch
  (range-diff: `record` beside `at_setup`, the duplicate `asks_to_spell`
  dropped); its own checks and mutations running. 16:3x: the hour posted.
- 16:5x: READ-OK `0b9fef6b8a` on #5166 (names side, the second of two
  with Friday's on the same patch): row 7's carry once, theirs, inside the
  held turn's minute; rows 9 and 10 name only the asker's record and the
  journal only a reason name; El's five lines in the negative test;
  switch hygiene. Gate: full lib 4684 on `7037b11e47` (short TMPDIR), and
  on the rebase fmt/clippy clean, names filter 541, process 45, switches
  2, notes 15; mutations on `record_now` and the `carried` guard each
  caught. Worktrees removed. Every item placed on heron today is posted;
  what remains is other builders' (rebases, trains, Faye's release of
  #4450's `status:taken`).
- 17:35–18:0x: #5258 (the first builder's #5219 hotfix, the next promote's
  blocker) opened with heron to read first. Read: `Abandon` carries
  `no_audio` from `had_audio`; with audio, once more then held; never
  voiced, up to 5 retries 5 s apart then held; the ledger written as
  schema 2 only while it keeps an abandoned retirement. Gate: fmt, clippy
  (lamp-voice, lamp-schedule), lamp-schedule 206, lamp-voice lib 4681,
  switches 2, notes 15; mutations on the once-more, the five-retry cap and
  `written_schema` each caught. READ-OK posted with a P3 on rollback.
  17:5x: #5097 restated on Friday's rebase `fe9808bb86` by range-diff.
- 18:4x–19:2x: #5272 (Friday, `LAMP_SECOND_MEMORY_HITS` on in the
  product) memory read taken. Gate: fmt, clippy clean; lamp-voice lib
  4749 passed, 1 timing failure that passed 3 of 3 alone; install 133.
  Mutations on the narrow clearing the fallback, the speakers check, the
  face-in-view check and the narrowed check each caught. A scratch probe
  showed another voice heard earlier, or a second silent face, leaves the
  face-in-view hits fallback in place, though the doc comment, memory.md
  and the privacy READ-OK say another voice takes it back. HOLD on a P2
  posted, fix the code or the claim. #5271's second read went to
  Thirteen. Hourly line to Faye posted.
- 19:3x–19:5x: the first builder withdrew the line of their #5272 READ-OK
  that leaned on the doc comment, and asked for the code fix. #4950
  landed in train 611 before 19:30. #5279 (Fourteen, Gmail names
  bridged): heron's second read was asked at 19:24 and missed in a gap
  between two watchers (one ended 19:20, the next began 19:26); the
  watcher now carries its start over. Read at 19:5x: the code is right;
  HOLD on a P2, as a mutation that always bridges passes all 57
  lamp-services tests and would send from the other account with both
  links; a P3 on a `read` guard no test needs. #5283: heron takes it if
  El hasn't started by 20:45.
- 20:0x–20:3x: #5279 READ-OK restated on `aeff9448b3` (the both-links
  test added; the always-bridge mutation now caught). #5284 (Friday, no
  own name from a mishear) memory read: HOLD on a P2, as the writer's
  `seen` wiring has no test (every id seen, none seen, and `seen_by`
  taking any speaker each pass all tests); Thirteen agreed and will
  restate. #5272 READ-OK restated on `c95ab279b3`: another voice, earlier
  or as judged, now takes the hits fallback back, tested by the four probe
  rows; the second-silent-face question left to the first builder.
  The lamp-voice flake `a_stop_said_over_the_lamps_answer_closes_the_conversation`
  failed in two full runs tonight and passes alone (shared `THREAD`).
- 20:3x–20:4x: #5272: Friday's 20:36 line said two silent faces give no
  fallback; the head's `hits-two-silent` row asserts A's hits by `Face`,
  so heron posted the fact for the first builder's call. #5221 (Fourteen,
  the publish carries the pinned models) second read: READ-OK. The
  models.json history shows no pinned file changed its SHA-256 across a
  published build, so a switch-off lamp is stuck only with a hand-placed
  different CAM++ (P3, a recovery line). lamp-fc4b's own files are not
  checked: it waits on the person's yes for a read-only ssh.
- 20:5x–21:1x: #5283 (a plain send with no id has its voice judged)
  second read: READ-OK; `run` back to the id-only check survives as a P3,
  since `run` has no direct test; Fourteen's 9adf check passed; NEXT
  TRAIN. #5287 (a Calendar time needs its zone) on `d75e80ae34`: READ-OK
  with a P2 owed before the writes switch: the fixed `+07:00` example
  can make a lamp outside UTC+7 write an event hours off. #5291's memory
  read went to Thirteen. The first builder ruled on #5272: the fallback
  needs the person's face to be the only face in view; heron restates
  on Friday's head.
- 21:2x–22:3x: #5293 (sends on in the product, the print for the services
  only) second read: READ-OK, conditional on #5294 landing first; every
  `for_services` use in the second route is bookkeeping, so no
  conversation rule reads a print. #5294 (Fourteen, the Calendar example
  in the lamp's own offset, heron's #5287 P2): READ-OK, with a P3 that
  `tools.json` keeps the offset from its start. #5284 READ-OK restated on
  `b798fdca13`: the writer test and the known-face row catch all three
  `seen` mutations; NEXT TRAIN. #5301 (the first builder, the
  `THREAD` flake heron reported): READ-OK; full lamp-voice lib 4834/0.
  `image_ramlog::ramlogs_copies_leave_the_journal_out` fails on this Mac
  on lamp-voice's base too (environment).
- 22:3x–22:5x: #5304 (a linked-service turn takes no picture, the root
  fix of heron's #5287 P3) first read: READ-OK, four mutations caught.
  #5293 READ-OK restated on `27f0cfd1f` (docs merge only), still after
  #5294. Correction posted on #5301: a zsh filter (an unsplit variable)
  left two of the close tests out of the 3-run repeat; they passed in
  the full run once. #5303's second read waits on Fourteen's fix of
  Thirteen's P2. For 5 Nov: #4526, #4579, #4568 second reads after the
  Reviewer-lead.
- 23:0x — #5303 READ-OK at c8a300f144 (second read, beside Thirteen's
  restate): one refusal check held by lamp-services, lamp-skills and the
  voice; fmt, clippy, lamp-services 66, lamp-skills all targets,
  lamp-voice made_skill 36; six of seven mutations caught, the door's
  `spoken::items` wiring untested (P3, gone with #5308). #5308 (Notion via
  MCP) had El's and Thirteen's reads, so not taken; merge order with
  #5303 is Thirteen's note 2. Fourteen put #5293, #5303 and #5308's calls
  to Faye.
- 23:2x — #5272 READ-OK restated at d55c259ef4 by range-diff: the face
  fallback needs the person's face the only face in view
  (`one_face_in_view`), the first builder's call; fmt, clippy,
  person_scope 41, hits 12, remember 320, another_voice 18; putting
  `face_seen_between` back fails the new two-silent-faces test.
- 23:5x–00:1x — #5276 READ-OK at 749bf3f030 (second read, after
  Thirteen's): KEEP_HEARD's yes after a second offer, a bare answer never
  kept; fmt, clippy, remember 322; full lamp-voice lib on a loaded Mac
  had 40 timing failures, all 40 pass rerun exactly; the heard/thread
  filtered groups fail the same way on the base (old interference); five
  mutations caught; P3 the journal's YES_ALONE_LINE still says "only a
  yes". #5312 let go: Thirteen and El read it first. #5272 has the first
  builder's privacy READ-OK at d55c259ef4.
- 00:2x–00:5x — #5250 READ-OK at 721051d588 (second read, Faye's
  placing): the wordless cut-in path is unchanged, only the switch and
  its line; voice tests, lamp-update install 132, stop-lines-test; both
  mutations caught. #5317 HOLD at 3607b40c0a (first read): the fix for
  #5303's lamp check is right, but no test starts a worker with
  LAMP_SKILL_READS, so dropping the `.envs` survives; one row in
  `the_memory_switches_alone_reach_the_started_routes` catches it (tried).
  The same gap is how my #5303 read missed the bug, owned on the PR.
- 01:2x — #5317 READ-OK restated at 5b0ca9dd76: the started-worker row
  is in, and both survivors now fail (`.envs` dropped: process::tests
  17/1; the ask-word guard: the new row); made_skill 36, process 18.
  Nine asked #4526 (90c52a59ba) and #4579 (869bd94832) of the
  Reviewer-lead first, then Heron, for the 5 Nov evening promote.
- 01:4x — #5321 HOLD at 7b02201d0c (first read; Thirteen's HOLD is the
  same P2): the voice/heard mark is right, but my run shows both
  mutations survive (`seen` back to `latest`, the shipped bug; `serve`
  never listening); a `LampRoom::seen()` assert catches the first
  (tried). Asked the lamp check for a 2+ item read alone: the lamp's own
  reading may now mark "a voice heard" and stop it after one line. This
  is #5303's second gap my read passed.
- 01:5x — #5321 READ-OK restated at ad2009778: the gate test asks
  `LampRoom::seen()` and the shipped-bug mutation now fails; fails
  closed and a restart counts as a voice; P3 the HEARD_ALWAYS latch
  test can't tell 0 ms from now. Before the train: Fourteen's lamp row,
  a 2+ item read with the owner alone, all said.
- 02:0x — #5317 READ-OK restated at 970f07cacc after the rebase over
  #5308 (`&notion` out of two loops); its first commit landed as
  85efa8bbb ahead of this test; the `.envs` mutation still fails.
- 02:2x — Faye placed #5064's second read on me: it's already in
  (READ-OK e2657988dc, 4 Nov 16:27), head unchanged; it conflicts with
  lamp-voice in three docs, so a rebase is what it waits on.
- 02:4x — #5326 READ-OK at bc3e1aa315 (second read, after El): the
  forget boundary resets the turn's context to the plain character,
  content-free outcomes and the text-only words, drops carried rounds and
  the rest of the batch; forget 162, short_done 21, config 42; four
  mutations caught, `seeing` equivalent; P3 a held forget with carried
  rounds is untested.
- 03:0x — #5321 trained before my READ-OK's lamp row (2+ item read
  alone) was played; it's off behind LAMP_SKILL_READS, so the row is now a
  condition for turning the switch on (asked of Faye). Fourteen plays it
  at 07:00 when lamp-9adf's backend allowance returns.
- 03:3x — #5333 HOLD at 4f192b117a (memory read), on El's P2, confirmed
  at nightly.rs:885: the night reads every active exchange of the day with
  no forget check, so a topic forget of a line never kept comes back with a
  quote; the switch-on is right (install 133, functions 6, nightly 28; the
  dropped-line mutation fails). #5335 READ-OK at 81005804ed (release hashes
  checked by download), with a two-voices call left to Faye (Kore Wi-Fi pair
  vs the Rachel lamp voice). #5337 left to Friday.
- 04:18: Faye did not accept #5333's forget gap as a risk: Friday adds a
  per-person forget mark, El restates, and I restate my HOLD after it. #5335
  trained on my READ-OK; the Wi-Fi pair stays in Kore as approved on 2 Nov,
  so the two-voices call is settled.
- 04:3x: #5293 READ-OK restated at a312c4e939 by range-diff from 27f0cfd1f
  (Fourteen asked): two rebases, context moves only, non-doc patch lines
  identical, CI green.
- 05:2x: #5064 READ-OK restated at 71dafb08c2 after Friday's docs-only
  rebase: code byte-identical, gate green (remember 333, process 61), and
  dropping the newer-first or profile `.envs` still fails 2 of 61.
- 05:3x–11:5x: the watcher was down, and I missed Faye's 11:12 order and
  the local leader's 11:45 post (said on Queue #5385). Meanwhile El held
  #5345 for the cross-midnight gap (my probe found the same) and it merged
  fixed; El took #5293 in full in my place.
- 12:0x: #5333 READ-OK restated at 848dd93ef6 (HOLD lifted; my
  cross-midnight probe passes on lamp-voice). Asked Faye to close #4450.
- 12:0x–12:4x: SDK second reads, READ-OK on all three. #4568 a8318bbd1b
  (`holder_may` mutation caught). #4579 869bd94832 (lease-cap and
  900-byte mutations caught; body clamps any finite look). #4526 90c52a59ba
  (stop and drop-in mutations caught; P3 unit hardening, retry-off).
- 13:0x: #5336 READ-OK d4dfa360d7 (HPKE pinned to RFC 9180 A.2.1; AAD,
  cancel and say_pin mutations caught; small-order equivalent via ring);
  the broker-rewrite race written out as the accepted risk. Corrected my
  #4526 note: the answer.rs lists are switch groups, not owner-only; the
  owner is the account channel.
- 13:3x: #4526 READ-OK restated at ca14a39262 (two fixture hashes checked,
  KINDS 55; the red notes test fixed by #4568's base; I hadn't run
  lampos-promote notes). #5396 READ-OK 4ca3fad661, P3 confirmed by run: a
  declined forget-everything's "Okay, I'll keep it." is asked again and
  can end in NOT_KEPT_LINE.
- 13:4x: #5388 HOLD 23db64e13c with the Reviewer-lead's P2 (stop_failed
  sends Stop without holding; same Linux loss); renew's hold mutation
  survives on macOS. #5185 merged in train 640, no read owed.
