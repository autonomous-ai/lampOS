# Blue builder's work log

Blue works on `lamp-blue`, with one `blue/<task>` branch per task. Its task
is a special one from its person, not from the Queue: the partner add-on
protocol, whose design is agreed with the first builder on the Queue issue
(#1722) before any code. Blue shares lamp-75c9 with eighth, nine and ten,
under the lamp lock.

## Proposed tasks

| Id | Proposal | Answer |
|----|----------|--------|
| blue-P1 | A partner add-on protocol: add-ons read allowed lamp topics, memory and context through Harness, and write one proposal topic; a judge on the lamp keeps or drops proposals before the voice acts; five questions to settle first (#1722) | yes, but: an add-on is a skill of kind `addon`; on-lamp half in 1.0, off by default; Harness half after 1.0; rules judge light, move, mood; Jev only a say; summaries only, no memory in phase 1 (first builder, 13:1x, #1722) |

## Found, not mine

## 2026-10-21 — joined

Ready for lamp-voice: no

- Role file `lampOS/builders/blue.md` made from the template, on
  `lamp-blue` from `origin/lamp-voice` at `ee1edd60a`. Lamp-75c9, shared,
  reached by key. No code yet; blue writes none until the first builder
  has answered blue-P1 and labelled issues `builder:blue`.
- Read for blue-P1: the bus (`docs/bus.md`, `crates/msgs`), the arbitration
  (`msgs/src/arbitration.rs`), the policy and Jev (`docs/policy.md`), the
  say service (`docs/speaking-first.md`), skills' permissions
  (`docs/skills.md`), `lamp-memory`, `lamp-tasks`, `docs/architecture.md`'s
  "Future task and memory boundaries", and Harness (`autonomous-ai/openharness`
  at `8c616abd`: the device protocol, Agent Messages, Store packages).

## 2026-10-21 — blue-P1 written down

Ready for lamp-voice: no

- `lampOS/docs/addons.md` on `blue/addons` (`d8099bae9`): the design blue-P1
  proposes, the first allowlist and the open questions, so the first
  builder can answer against one page. Docs only, not a pull request until
  blue-P1 is answered.
- On #1722 at 13:0x: no rows from `lamp-runs.md` are blue's; lamp-75c9
  stays with Ten and Eighth for theirs.

## 2026-10-21 — blue-P1 answered; docs PR #2499

Ready for lamp-voice: yes (#2499)

- The first builder answered blue-P1 at 13:1x (#1722, comment 6309437399):
  an add-on is a skill of a new kind, `addon`; one write topic
  `addon/proposal` written by `lamp-skills`; a summaries-only allowlist;
  rules judge light, move and mood, a new acting Jev question only for a
  say; on-lamp half in 1.0 off by default, Harness half after 1.0. Next:
  `docs/addons.md` as one docs-only PR, then the `addon/proposal` type.
- #2499 from `blue/addons` (`897d24691`), docs only. The kind: docs only;
  no ARM64 run needed (joining.md, "Moving labelled work" 6).
- Found, mine: adding `builder:blue` to #2499 through the API created the
  label, since it did not exist. Told the first builder on #1722; left to
  them.

## 2026-10-21 — #2499: the first builder's three changes

Ready for lamp-voice: yes (#2499)

- The first builder's read (13:4x, on #2499): yes, after three changes in
  the one commit. Folded as `275145269`: the person's light wins (a light
  proposal only over the lamp's idle light, #1362); a note is quoted,
  untrusted context, never an instruction; remote add-ons need their
  person's yes per subscribed summary, seen and revoked per add-on. Left
  for reviewer-two to merge.

## 2026-10-21 — the Harness half, autonomous-ai/openharness#1485

Ready for lamp-voice: no (not lampOS code)

- At my person's request: device add-ons in Harness's CLI, on branch
  `feat/device-addons` of the fork `daniel-auto/openharness` (`a9f653ca0`).
  `addon.publish` / `addon.decision` capabilities and the `addon.proposal`
  event on the existing device channel, bounded in RAM; `harness
  autonomous-device addon state|propose|proposal` over the loopback API.
- Shown on this laptop: typecheck; the new and touched specs; `test:core`
  2,910 passed with full coverage; the full suite's 25 failures are the
  same 25 as on `origin/main` here (environment); an isolated daemon from
  the branch answered every `addon` command. Not shown: a device; none
  implements the device side yet.
- Told the first builder on #1722.

## 2026-10-21 — the first add-on code, #2514

Ready for lamp-voice: yes (#2514), once its ARM64 run is green

- The first builder's next for blue (13:5x, #2499): `lamp_msgs::AddonProposal`
  and lamp-skills writing it for `kind = "addon"`, off unless
  `LAMP_ADDONS=on`. Claimed on #1722 at 14:0x; no issue for it yet.
- `blue/addon-proposal` at `5d4f76b36` (rebased on lamp-voice `9596fb457`).
  Off unless set: without the switch no add-on is loaded and the topic is
  never declared (`tests/it/addon.rs`).
- Gate on the laptop: fmt; clippy -D warnings for lamp-msgs, lamp-skills,
  lamp-voice-eval, lamp-cloud; their tests all ok (lamp-skills `it` 221,
  5 new); workspace build and `--no-run`. Not on a lamp: nothing reads
  `addon/proposal` yet.
- openharness#1485: the reviewer's 8 findings fixed in `a7af5bb85`, each
  answered; its CI waits for a maintainer's approval (a fork's PR).
- 14:3x, #2514: the reviewer-lead found no P1/P2 and one P3: the app's skills
  page showed an add-on working while `LAMP_ADDONS` was off. Folded in as
  `e9ca4f398`: lamp-cloud reads the switch from its own unit and lists it
  "add-ons are off on this lamp". Reviewer-two leaves the shared message
  type to the reviewer and the first builder. openharness#1485 is approved
  on re-review; it waits for a repo admin to approve its held CI run.
- 14:5x: openharness#1485 merged as `3f3922e52` (the Harness half, CLI only,
  unreleased). Its re-review's two doc notes are openharness#1488. Told the
  first builder on #1722.

## 2026-10-21 — #2514 merged

Ready for lamp-voice: merged (#2514, `7db3a3497`, by the first builder at 14:54)

- ARM64 run 39562880069 green (18m20s), clippy green; the reviewer-lead's P3
  closed on `e9ca4f398`. There was no issue to close.
- Asked the first builder on #1722 for the go and a `builder:blue` issue for
  the judge (rules for every act, an acting Jev question for a say, the mood
  nudge input and the say service's context line), off unless
  `LAMP_ADDONS=on`.

## 2026-10-21 — #2530 taken: the add-on judge

Ready for lamp-voice: no

- The first builder's go (14:5x, #1722): #2530, `builder:blue`. Part 1, one
  PR: the rules for every act, a light through the arbitration at `Skill`
  priority or below, and a bounded, decaying mood nudge on `lamp-mood`.
  Part 2, the say with an acting Jev question and the say service's context
  line, only after part 1 has run on lamp-75c9; the Jev question is posted
  on #2530 for the first builder's read before it is built. Each part gets a
  lamp-75c9 run and a `lamp-runs.md` row. All off unless `LAMP_ADDONS=on`.
- Claimed at 15:0x: `status:taken`, "taken by blue · session b602ea5d… ·
  lamp-75c9"; the first taken line is blue's.

## 2026-10-21 — #2530 part 1 up as #2553

Ready for lamp-voice: yes (#2553), once its ARM64 run is green

- `lamp-addons` (`crates/addons`): the rules, the situation folded from the
  bus, a kept light asked of the ring at `Idle` as `addon:<name>`, and each
  verdict on a new `addon/decision`; `lamp-addons.service`, off unless
  `LAMP_ADDONS=on`. One commit, rebased on lamp-voice `533b1dae0`.
- The two questions on #2530 (15:1x) had no answer by 15:4x; taken in their
  narrowest safe reading (joining.md): `lamp-mood` untouched (a kept mood is
  said, not carried out), `lamp_msgs::Decision` unchanged.
- Gate on the laptop: fmt, clippy -D warnings and tests for lamp-addons,
  lamp-msgs, lamp-update, lamp-cloud, lamp-voice-eval; workspace build and
  `--no-run`. Next: a lamp-75c9 run of the two examples on #2553's ARM64
  build, watched with the laptop's camera and mic.

## 2026-10-21 — #2553: the reviewer-lead's read, and a correction

- **Correction:** the first builder had answered both #2530 questions at
  15:1x (comment 6311216795); I missed it and said they were open. Said so
  on #2530 at 16:00. The answers:
  - a `crates/mood` nudge input is yes: ±0.1 inside the clamps, held until
    `until_us`, decaying over its last minute, the newest replacing the
    last; warmth positive only, energy both ways; a test that no nudge
    passes a clamp.
  - `policy/decisions` is no: decisions go on blue's own `addon/decision`.
  - One more never: nothing is kept while onboarding runs a step.
- **The reviewer-lead read `2b892bc26`:** one P2 and three P3s, all fixed
  and folded as `c1af23e28`.
  - **P2:** a product lamp dropped every proposal. A `lamp/pause` never said
    counted as paused, and so did a `voice/mute` never said; the product
    voice says none. Now:
    - a pause never said is not paused;
    - with no mute read 3 s after the judge starts, the rest skill alone
      tells sleep;
    - an e2e test publishes neither topic.
  - **P3s:**
    - a test that a proposal not written by `skills` is not judged;
    - `give_back` is deleted and the doc says the claim lapses by itself;
    - `LAMP_LIGHT_KEEP` is documented as not handled yet.
  - **Folded too:** the onboarding never (`onboarding/pending` holding an
    invitation).
- **Next:**
  - The mood nudge as part 1's second PR, after #2553: a typed
    `mood/nudge` written by `lamp-addons`, held in `lamp-mood`'s engine
    inside its clamps.
  - Then the lamp-75c9 run.

## 2026-10-21 — #2577: the mood nudge (part 1b); #2553 cleared

- **The reviewer-lead re-read #2553 on `c1af23e28`:** the P2 and two P3s
  are closed, the ARM64 run is green, and it merges on that head. Their
  one new P3 is fixed in #2577, so #2553's green head stays:
  - a restarted voice (a new publisher boot on `voice/state`) made the old
    `voice/mute` reading go stale and held the lamp asleep;
  - the judge now forgets the old mute and waits 3 s afresh.
- **#2577 (stacked on #2553):** the first builder's question-1 answer.
  - `MoodNudge` on `mood/nudge`, written only by `lamp-addons`.
  - `lamp-mood` adds it before its own clamps: energy ±0.1, warmth up only;
    held until `until_us`, fading over the last minute; the newest replaces
    the last; none taken asleep.
  - The judge drops a colder mood.
  - Gate: all green under umask 022, except a `lamp-schedule` test this
    branch doesn't touch. It fails one run in three on its own on this
    laptop.
- **Lamp run, ready:**
  - #2553's ARM64 artifact (run 39575120939) is downloaded, and
    `lamp-addons`, `lamp-skills` and `lamp-mood` match its SHA256SUMS.
  - Run copies of both examples load in `lamp-skills test`.
  - Waiting on lamp-75c9's lock (Eighth).

## 2026-10-21 17:1x — part 2's design answered; part 1's run sheet

- **The first builder on part 2 (#2530, 17:1x): the design is right.**
  Build it after part 1's lamp run.
  - **Q-A, yes:** `lamp-policy` writes `about: "addon"` on
    `policy/decisions`, with an optional `proposal`. Check the voice's
    reader filters on `about`, and test that the voice ignores an `addon`
    decision on Live and on the second route.
  - **Q-B, the typed grant path:**
    - a kept say is the skill's own note, and only with a declared say
      grant (as #2596's `say.from`);
    - the note is quoted and marked untrusted;
    - a kept `note` is a `try_context` line, never spoken;
    - all behind `LAMP_ADDON_SAY=on`, and shadow until `--act addon`.
- **The Queue is now #2602:** #1722 is full.
- **Part 1's run sheet:** posted on #2530 (6313491829), with six rows:
  - the light kept;
  - the mood kept;
  - the person's light wins;
  - each reachable never once;
  - the rate cap;
  - restore.

  Not tried on the lamp: privacy closed, asleep and onboarding (each
  with why).
- **lamp-75c9:** asked its holders on #2602 for a slot after 18:00. I
  cancelled my waiting acquire so as not to take the lamp before then.
- **#2553:** next in the reviewer-lead's merge queue (17:17).

## 2026-10-21 18:00–20:1x — #2553 merged; the first lamp-75c9 run

- **#2553 merged** into lamp-voice as `27130e95e` (the reviewer-lead),
  after two drift rebases (`docs/install.md`, then `install.rs`), each a
  range-diff `=`.
- **#2577:** retargeted to lamp-voice through the REST API (`gh pr edit`
  failed on Projects classic). Rebased as `815eb09ec`; both CI runs are
  green, and it is in the reviewer's queue.
- **The lamp-75c9 run, 18:05–18:13**, on #2553's head `513da9128` (ARM64
  run 39585921049):
  - root steps through the helper's `[hand-off]`;
  - a probe mood was kept; a probe light was dropped by the person's own
    light (orange, 1, set this boot, up since 19 Oct);
  - later proposals were dropped by a busy room;
  - **not reached:** the evening and music triggers (laptop screen locked,
    `ears/classes` off) and the rate and pause rows (session stopped).
- **My mistake:** my lock-renew loop held 75c9 to 20:01 after my session
  stopped at 18:13; the timer had restored the lamp at 18:31. Apologised
  on #2602, and noted in memory: renew loops get a hard end.
- **Asked the first builder on #2530:** should a person's light stay theirs
  for the whole boot (a), or 12 h or until the lamp sleeps (b)?
- **Tests' #2630** (faster judge tests): yes, and it passes merged with
  #2577. Said yes on #1806 to the `ASLEEP_FRESH_US` `serve` option for
  tests.

## 2026-10-21 20:1x–21:3x — #2577 merged; #2640; part 1's rows passed on lamp-75c9

- **The first builder's (b) on the person's light (#2530, 20:1x):** it
  stays their base. Once it rests (Mood asleep since, or 12 h), an add-on
  may lay a brief glow over it.
  - Built as **#2640**: 5 s at `Idle`, and the judge's own glow keeps the
    light theirs.
  - The reviewer-lead's P2 (a light set off rested too) and P3 (Mood's
    clock) are fixed as `3d10c65ca`. I caught my own precedence slip on the
    way.
- **#2577 merged** as `26a949ffa`, read clean by the reviewer-lead. #2640
  is retargeted and rebased.
- **lamp-75c9, 21:17–21:24,** on #2577's build:
  - the busy room, the person's light, a kept mood, the rate (51 s after a
    kept one), the pause and the rate freeing after 2 min, all as designed;
  - recorded on the bus;
  - root steps by the helper's `[hand-off]`;
  - lock released at 21:24, everything restored, and `/var/lib/blue-addons`
    removed.
- **#2662:** the `lamp-runs.md` rows. Part 1 is struck; #2640's glow row
  waits for its build.
- **Next:** the glow row on #2640's build, then part 2 (`say`).

## 2026-10-21 21:3x–22:5x — part 2 up as three stacked PRs

- **#2666 (2a), the judge's ask and answer.**
  - The Reviewer's P2: an answer kept a say past its deadlines. Fixed: the
    ask now keeps both the proposal's own lease and the 3 s wait, and an
    answer at or past the earlier keeps nothing (`Asking::past`), with
    boundary tests. Rebased onto #2640 (11 conflicts resolved).
- **#2667 (2b), `lamp-policy --gate --addons`.**
  - The moment question, about `addon`, shadow unless `--act addon`.
  - The Reviewer's P3 (a real two-ask coexistence test) is closed.
- **#2675 (2c), the voice's side.**
  - The first builder's three conditions, and (b): the Live route refuses
    `quoted`.
  - On the second route: no tools, calls refused at execution,
    `keep_out`, journal `said nothing`.
  - The grant is `say = "quoted"`, with `propose` naming `say`.
  - Test 5: the voice ignores `addon` decisions on all four readers.
- **#2671 (tests only):** the add-on light test keeps the judge's journal
  for a failure to show, after Second's Mac failure. My race theory was
  wrong (the old order slowed still passed), and I said so.
- **Claimed next:** a kept `note` as a fresh system note on the second
  route's next spoken turn. Its shape is posted on #2530 for a read; I build
  it at 23:20 without an answer.
- **lamp-75c9:** Eighth, then Ten, then blue (#2640's row), then Nine.

## 21 Oct, 23:0x: no kept note, and #2675's P1

- First builder (22:53, #2530): no kept `note` as free text on the person's next turn, since that turn has every function. Notes may return later as typed notes (a kind plus bounded values the voice renders). For now `note` is refused at load. Claim withdrawn on #2602.
- Reviewer-lead P1 on #2675: a quoted line was kept in the voice's thread and replayed as a system note on later turns. Fixed with `keep_written` (no `thread.written`, marked `count_only`), plus a test. Folded as `7d5686692`. The 900-byte P3 needs no change: the proposal's note is capped at 300.

## 21 Oct, 23:1x–23:2x: #2675 read clean; #2640's row passed

- **#2675:** the Reviewer-lead's P3s are fixed by `Thread::quoted`, a mark of its own, so an onboarding sentence keeps its mark and a person's words are kept. Folded as `7662cb711`. Re-read with no finding; it merges after #2667.
- **lamp-75c9, 23:20–23:27**, on `57920a920`, with root steps by the helper's hand-off:
  - the room was busy until 23:23:11;
  - at 23:24:02 and 23:26:02, a probe light was kept at `idle` for 5 s over the person's resting orange, with `light/state` orange throughout;
  - the rate dropped the rounds between;
  - the camera's evidence is weak (an overexposed band).
- The lamp is restored and released to Nine. #2682 strikes the lamp-runs row; the set-off case stays open.

## 21–22 Oct, 23:4x–01:1x: part 2 in the merge lane; its lamp row scripted

- **First builder (23:46):** #2666 was ready but had drifted. Rebased as `01bc12064` (range-diff `=`, workspace clippy clean, ARM64 and x86 CI green). #2667 (`743ece8df`) and #2675 (`83e46d7d8`) were restacked, each range-diff `=`.
- The Reviewer-lead set #2666 for 17:10Z, but it was still open at 01:0x. #2682 (the #2640 row struck) merged.
- **First builder (00:42):** the next step is part 2's 75c9 row script, so it runs as soon as #2675 is in. It is written: `blue-lamp2.sh` and the probe add-on `probe-say` (a quoted say after 20 s of the voice resting). The plan is posted on #2530. It runs on the CI build that holds all three PRs.

## 22 Oct, 01:2x: part 2a merged

- **#2666 (2a)** merged as `849c0829f`.
- **#2667 (2b):** retargeted onto lamp-voice through the REST API and rebased as `6d035c151` (`=`), then pushed so CI runs. #2675 is restacked as `caac7abcc` (`=`).
- **#2671:** read with no finding by the Reviewer-lead. It drifted on `tests/judge.rs` from 2a, so it is rebased as `b5d36beb4` (`=`). lamp-addons' tests and clippy pass.

## 22 Oct, 01:5x: #2675's lamp check; #2671 merged

- **First builder (01:50):** while 2b and 2c wait, write #2675's lamp check for lamp-4ace.
  - Folded into #2675 as `864b53429`, adding files only: `tools/played/addon-say.sh`, `tools/played/addon-say-test.sh`, a `lamp-runs.md` row, and the played README.
  - The script turns `LAMP_ADDON_SAY` on with a `/run` drop-in, removed on exit. It asks the say socket for one `{skill, quoted}` and compares the lamp's transcript for that turn with the kept text: kept in order, changed, added, refused.
  - The stand-in test passes (11 cases). Not run on a lamp yet.
- **#2671** merged.
- #2667 is third in the Reviewer-lead's lane.

## 22 Oct, 02:0x–02:3x: 2b merged; #2675's ARM64 failure was mine

- **#2667 (2b)** merged as `97ce33ce3`. #2675 is retargeted onto lamp-voice through the REST API (`7dd7c6b6d`, `=`).
- **ARM64 failed** in lamp-cloud's `an_add_on_is_said_off_while_add_ons_are_off_on_this_lamp`. Its test add-on proposed `note`, which this PR now refuses at load. I had run only the voice and skills crates.
  - Fixed: the add-on proposes a `mood`. Folded as `6eb4a9666`.
  - The whole workspace ran locally: 200 targets passed. lamp-audio's bounded-work timing test and lamp-records' `public_tree_pipeline…` failed under the workspace's load, and both pass alone.
  - Noted in memory: a lamp-skills load-rule change gets the whole workspace's tests.

## 22 Oct, 03:0x–03:1x: a peer read of #2686; #2675 rebased twice

- **First builder (03:06):** peer-read El's #2686, #2617's six Wi-Fi follow-ups, on `1460c6fd88`. Five of the six match the ask.
  - **P2, follow-up 6:** when the app didn't confirm and the old file can't be written, `hold` sends `REMOVE_NETWORK all` first. Any later refusal or timeout leaves wpa_supplicant with no network, and nothing reloads the live file. The lamp is off both networks until a reboot.
  - **Fix suggested:** send `RECONFIGURE` on error, wait between tries, and add a part-way-failure test.
  - **P3s:** the stop restore is one radio try in practice; a confirm with an id is refused when the set had none.
  - The first builder agreed (03:15) and asked El to fold it.
- **#2675:** lamp-voice moved twice and conflicted, each time on the played README's list of scripts. Rebased as `1a0d2e9cb`; the range-diff is that hunk only. Gates are clean and CI is running.

## 22 Oct, 03:4x–04:1x: #2686 re-read; part 2 merged

- **#2686:** El folded the P2 (`reload` after any `hold` error and when the old network never came back, tries 1 s apart, a part-way-failure test) and both P3s, as `3718dabf34`. My re-read found no new finding.
- **#2675 (2c)** merged as `8826f0bf2`, on the first builder's word after one more rebase (`cc33e1ea1`, `=`; #2649, #2686 and #2725 had touched the second route's code). **#2530 part 2 is in:** 2a `849c0829f`, 2b `97ce33ce3`, 2c `8826f0bf2`, all switches off.
- **Open, for the morning:**
  - `addon-say.sh` on 4ace (the first builder's);
  - the full path on 75c9 with `blue-lamp2.sh`, on lamp-voice's ARM64 build of `8826f0bf2`;
  - #2640's light-set-off case, which needs my person's yes.

## 22 Oct, 04:2x–04:4x: `move` up as #2738

- **First builder (04:25),** answering my shape on #2530:
  - only `nod`, `curious` and `stretching`;
  - in now, behind `LAMP_ADDONS`;
  - one per add-on in 30 min, 2 an hour, and never within 60 s of talk.
- **Built as #2738** (`4b6cb99df`):
  - `ADDON_GESTURES` in lamp-msgs;
  - three new drops in the judge;
  - a kept move goes as one idle gesture on the body's mood socket (`--body-socket`, test-only `--move-quiet-ms`);
  - unit tests, two e2e tests (a mutant fails one), and a skills load test;
  - docs, and a lamp-runs row on 4ace with someone at the arm.
- **Gate:**
  - fmt and workspace clippy are clean;
  - the add-on-type crates (cloud, update, skills, msgs, addons) pass;
  - one daily-journal lock race in lamp-skills under load passed alone three times;
  - the full workspace run was stopped for low memory, so CI is the full gate.
- The ARM64 build of `8826f0bf2` is staged for the morning's 75c9 row.

## 22 Oct, 04:5x–05:1x: `move` merged; the judge-test flake

- **#2738 (`move`)** was read clean by the first builder and merged as `669f7de53`.
- **First builder (04:50):** the judge's say test flaked on #2737's run (`judge.rs:438`, "asked"). There were two races in the test:
  - **the early start:** it waited only for the judge's first subscription;
  - **privacy said once:** 50 ms at most, so it could be passed over under load.
- **Fixed as #2741 (tests only):**
  - `listening()` waits for `policy/decisions`, the judge's last subscription;
  - `calm()` keeps privacy said every 20 ms behind a `#[must_use]` guard.
- **Shown:** with a temporary 1 s pause in the judge, the old test fails as on CI, and the fixed one passes.
- The first builder read it clean. It was rebased over #2738, and `moved()` now uses both fixes.

## 22 Oct, 05:3x–05:5x: #2741 merged; the made-skill check, #2745

- **#2741** (the judge-test flake) merged as `08e091245`. #2530's code is all in. Only lamp rows are left, for the morning.
- **First builder (05:34):** next is a played check for "An owner makes their own skill" (#2424). Claimed.
- **Built as #2745** (tools and docs only): `tools/played/make-skill.sh LABEL [yes|no|both]`.
  - It turns `LAMP_MAKE_SKILL` on with a `/run` drop-in, removed on exit along with any skill it put on.
  - It asks for a stand-up reminder skill and says yes to the hand-over.
  - It waits up to 8 min for the kept SHA-256 and for "Shall I put it on the lamp?".
  - **Yes arm:** nothing on before the yes; that SHA-256 put on, enabled, loaded and removed.
  - **No arm:** nothing installed.
- **Its stand-in test** passes, and a mutant without the SHA-256 comparison fails it.
- **Docs:** the lamp-runs row, replacing the stale "waits for the lead's ruling" line, and the README.
- **#2745's P2 (first builder, 05:43):** the yes would have started a real agent on a real machine at night.
  - Rewritten on `stand-in.sh`, as `calendar.sh` is, so nothing real starts. The fixture's `show` answers with a fixed `stand-up-nudge` skill (light only, a nudge; a made skill can't speak). `LAMP_MAKE_SKILL=on` rides on the stand-in's drop-in.
  - The steps follow `calendar.sh`: the lamp's confirm or an offer at once.
  - The test covers both orders and passes; the SHA-256 mutant fails it.
  - Folded as `7efb56173`.
- **#2745, the Reviewer-lead's P2 (06:2x):** in the yes arm, a skill put on before the yes was failed but left on the lamp.
  - Fixed: the early skill goes into `PUT_ON` in both arms.
  - The stand-in's folder is removed if an upload fails before setup.
  - New test cases `early` and `plan-fails`; the mutant fails `early`.
  - Folded as `8cf682a0d`. The first builder runs it on 4ace only after Eighth's store-of-its-own fix.

## 22 Oct, 06:5x–07:3x: #2745 merged; part 2's 75c9 row

- **#2745:**
  - rebased onto Eighth's stand-in store fix, at reviewer-two's base gate;
  - the Reviewer-lead's P2 closed;
  - merged as `20e394fac`.
- **The lead (07:3x):** run lamp-runs rows now, from the Mac.
- **lamp-75c9, 07:25–07:31**, on `8e5033e8e`, with `blue-lamp2.sh` and the `probe-say` add-on, root by hand-off:
  - four says were dropped by the judge for the room (`room/state` `unknown`, about −20 dBFS);
  - the room turned quiet at 07:30:05;
  - lamp-skills' 4-says-an-hour cap, which counts dropped proposals, stopped the next one;
  - the policy and voice end was not reached.
- Restored, and released to Nine.
- **Asked the first builder on #2530:** should lamp-skills count only kept says toward its cap?
- **07:3x, the first builder's answer:** yes. A dropped say costs nothing, and after a drop that add-on waits 5 minutes. Opened as #2768 (`blue/addon-say-cap`). The hold (`AFTER_A_DROP_US`, 300 s, that add-on only) was folded in at 08:00 (`2ecc994f2`), with a unit test and an e2e check; both fail with the hold taken out.
- **08:21, reviewer-two on #2768:** P1/P2 none at `2ecc994f2`, and asked for a rebase because `6bb802490` changed `main.rs`. Rebased at 08:3x to `c806ac0f8` on `efff7c4a5` (range-diff `=`). Locally, lamp-skills passes 140 + 22 + 271 and clippy is clean; fresh ARM64 is running.
- **08:2x, my person's decision:** everything related to mood measurement belongs to Sixth from now on. Blue takes no mood work and points any mood task to Sixth.
- **09:34–09:59, part 2's rerun on lamp-75c9** (#2768's build `61cd049bd`, via my helper's setup and restore). Five say proposals in 23 minutes:
  - the first was dropped by the room rule (`unknown`);
  - four reached the policy once the room was `quiet`, and all four came back `wait`: Jev 0.20, 0.21 and 0.19, and one rule wait after 1519 ms with no model answer;
  - 16 triggers were held by the 5-minute hold;
  - the voice's quoted turn was not reached.
  Put back and released to Nine at 10:01. Results are on #2602 (6331607814).
- **09:48, reviewer-two:** asked for a merge window at the current base for #2768 and #2764. I agreed to keep `fe174b748` until its ARM64 finishes.
- **10:0x, the backlog:** Blue has #2811, #2813, #2825 and #2842. Took #2813, then #2811.
- **10:1x, the first builder on #2813:** (b), keep it spoken-only. The lead's ruling on #1711 binds a made skill to a yes said out loud at the lamp, so #2813 becomes "typed in the app, put on by a spoken yes at the lamp". They also asked for the window check in lamp-skills' helper on its own: opened as #2874.
- **#2811:** `make-skill.sh` needs `sudo -n` on the lamp, which lamp-75c9 doesn't have. I asked whether it should run on lamp-4ace.
- **10:3x, the first builder on #2813's contract:** yes, as written. `skills.make` and `skills.made` behind `LAMP_APP_MAKE_SKILL`, put on only by a spoken yes at the lamp. `skills.made` covers only the app's own asks, and `not_kept` gives only a plain reason. Fourteen built #2814 (ecm-sds-mobile#38) against it.
- **10:53, reviewer-two on #2874:** P2. `waiting` filtered out installed and future offers before picking the newest, so an older offer came back once the newest was on, in the voice's `put_on` too. Fixed by taking the newest offer first, then refusing; added the real put-on sequence and a clock-rollback test; both fail on the old selection. Rebased to `f9203992a`.
- **11:0x, #2768:** P1/P2 none and ARM64 green at `14a44727e`. Rebased to `ef3206c23` for reviewer-two's current-base rule.
- **#2813, lamp-voice half:** committed on `blue/app-make-skill` (`app_make.rs`, `agent_tasks::hand_over`, docs). The lamp-cloud half (`skills.make` and `skills.made`) comes next.
- **11:2x, the first builder on #2811:** (a). They run `make-skill.sh … both` on lamp-4ace after 14:00 and post the PASS/FAIL lines; lamp-4ace's key stays with its holder.
- **#2874 and #2768:** both clean in review, rebased onto latest lamp-voice for merge (`6767514c2` and `1a121964b`). #2874 needed one test-only line for the base's new `asked` argument.
- **11:53, reviewer-two on #2887, three P2s, fixed:**
  - `app.json`'s writers now share one `flock` transaction; a 100-round overlap test fails without it;
  - the task-news reader settles the app's ask when work wasn't sent, failed or stopped (uncertain stays `making`); tested through the real reader;
  - the socket group defaults to `lampos`, and nothing starts if it's missing.
  #2887 is now stacked on #2874, so `asked` is the offer a spoken yes would put on now. `make-skill.sh` gained `MAKE_FROM=app` for #2813's lamp check, with stand-in arms.
- **Fourteen:** #2814's screen merged (ecm-sds-mobile#38, plus #39 fixing two wire names). #2813 waits only on its lamp check.
- **Note:** the screen-person script and media tool are under `/tmp/claude-1000/lamp-helper/` (my helper, 22 Oct).
- **12:4x–13:0x:** #2768 merged (`da41d3657`); #2874 carried as #2909 and merged. #2887's three P2 fixes pushed and restacked onto lamp-voice (`fe6cec965`); #2889 restacked (`c0af364e9`). Full lamp-voice had 3 failures in provider/coordinator tests outside this change; each passed 2/2 alone.
- **13:0x:** the first builder plays #2811 on lamp-4ace at 14:20–14:30. Mobile merged ecm-sds-mobile#39 and will run #2813 on lamp-0c89 once #2887 and #2889 are on a build.
- **13:1x–14:0x, #2887 and #2889 (#2813), more review:**
  - reviewer-two's staged control showed a fast-failing task stayed `making`; the settle now runs before the told/offered skip;
  - reviewer-two took the played app mode out of #2887 (inline Python, Bash 3.2 false PASS); it was rebuilt in #2889 as `lamp-cloud make-skill make|made`, run as `lampos`, with Bash-3.2-safe checks and negative controls;
  - El and reviewer-two: the app hand-over now finds the one Claude target when the target is the default (none or several refused), keeps a lost confirm as handed, and gives a malformed request id its own reason; seen ids are kept, the newest 100;
  - El's P1 on #2889: the app arm greets the lamp before waiting for the spoken ask (the news gate needs someone seen or heard).
  - Heads: #2887 `e98f71880`, #2889 `51b278f86`.
- **Lesson:** never rebuild a crate's test binary while its full run is going; it broke ~20 subprocess tests and voided that run.
- **17:49, #2889:** El's two later P3s are folded in.
  - The played greeting now comes only while no new ask has been said. The stand-in models the news gate, and a `FAKE_FACE` case checks that no greeting is played.
  - `skills.made` refuses a non-string or bad `request_id` before it asks lamp-voice.
  - Rebased onto lamp-voice `e9b0a46fb` and pushed as `07324d094`. The lamp-cloud gate (fmt, clippy, tests) is ok and the stand-in suite passed. Waiting on reviewer-two and El.
- **17:55, reviewer-two on #2889:** P1 none, P2 none at `07324d094`, and asked for a rebase onto the latest base. Rebased at 17:56 onto `fe49ed3c4`, now `d09034e1a` (range-diff `=`). lamp-cloud's tests and clippy pass.
- **17:59, reviewer-two on #2889:** P1/P2 none at `d09034e1a`; the base moved to `69f222833`. Rebased at 18:01 to `d4d0873e1` (`=`). lamp-cloud's tests and clippy pass.
- **18:0x, Fourteen:** the app's UUID v4 request ids pass. The app's field now counts UTF-8 bytes (ecm-sds-mobile#46), to match the 180-byte limit. No lamp-side change.
- **18:02, reviewer-two on #2889:** P1/P2 none at `d4d0873e1`; the base moved again. Rebased at 18:06 onto `bf38a1890`, now `fd92eb059` (`=`). lamp-cloud passes.
- **18:13, reviewer-two on #2889:** the full local gate passed at `fd92eb059` (Cloud 192/0, Voice-Eval 623/0; P1/P2 none). The base moved again: rebased at 18:15 onto `e2232b836`, now `f2af29ab6` (`=`).
- **18:37, reviewer-two on #2889:** everything was green at `f2af29ab6` (ARM64, Linux, the full local check and the fake suite), but the final guard caught the base moving to `f012c4725` (#2998). Rebased at 18:38 to `b4c7efdb1`; the only range-diff change is a base context row.
- **18:42–18:44, #2889:** reviewer-two passed `b4c7efdb1` (P1/P2 none). The base moved again to `5e51ac016` (skills and switches docs), so rebased to `eaf6b9557` (`=`). Asked the first builder on #2602 for a carry or a merge in a quiet window, after six rebases.
- **18:46, the first builder:** the Reviewer-lead carries #2889 in their serial queue; blue stops rebasing it. Its last head is `eaf6b9557` on `5e51ac016`.
- **19:37, the Reviewer-lead:** #2889 is carried as #3031, the same patch (range-diff `=`, head `14670b37d`). It is first in their queue and merges on clippy, which is green. #2813's lamp check follows the merge.
- **19:46, #3031 merged** (#2889 carried) as `1c9cbddc4`. Both halves of #2813 are now on lamp-voice. Fourteen says the app's screen (ecm-sds-mobile #38, #39, #46) now has its lamp side. #2813's done-when (the stand-in suite, ARM64 green) waits on ARM64 of `14670b37d`, which is still running. `f2af29ab6`'s was green.
- **19:59, #2813:** ARM64 is green on `14670b37d`, so the done-when is met. Posted on #2813 for the first builder to close it, and offered the real-lamp app run on lamp-75c9 as a separate row.
- **20:00, the first builder:** #2813 closed. Yes to the real-lamp app row on 75c9 tonight if Mobile can. **20:05:** read 75c9 under the lock: lamp-cloud is inactive (not paired) and it is on build 5900, before #3031. At 20:07 asked Mobile: 0c89 with drop-ins, or pair 75c9.
- **20:33, the first builder:** #2813's app row goes to Mobile on 0c89 (75c9 can't take it tonight). At 20:38 sent Mobile the build, the two drop-ins, the yes and no tries, and what to record.
- **21:03–21:2x, a piece nobody held (the new AGENTS.md rule):** proposed a developer's guide, then narrowed it at 21:08 because `skill-creator/` already has most of it. The first builder said yes. It is #3055: a section in `skill-creator/README.md`, every command run as written.
- **21:2x–22:02, #3055:** folded El's two P3s (the binary's path, `needs`). The Reviewer-lead read it with no finding, and it merged as `a4eb3d11a`.
- **22:05–22:3x, the first builder's code piece:** `turn_end_replay --unfinished-below/--unsure-ms`, using the voice's own parsers, now moved to `activity`. It is #3061, stacked on `first/join-one-person`. On 58 generated lines, 0.3/900 cut the split lines from 14 to 6 for +300 ms at the median. Asked for the real 4ace bundle.
- **22:3x–00:2x, #3061:** El's read found no P1 or P2. Their P3 is folded in as `ConfirmPolicy::with_dials`, used by both the voice and the replay, with a test; the replay output is byte-identical. At 00:2x moved it onto `first/join-one-person-on-2967` (`b77be89d7`), which the lamp-4ace stack carries, as `5e2621c56` (`=`). The gate passes, apart from one voice-eval flake reported to Tests.
- **23:4x, the open-issue audit:** #2530, #2811 and #2842 are all left, each waiting on a lamp or a person.
- **01:0x, my person on #2842:** no second computer joins the Harness account; only "set up later, then signed in" runs. Read 75c9 under the lock: `lamp-tasks` has `--set-up-later`, and the drop-ins match its unit. Posted the narrowed plan and a done-when change on #2842, for a slot after 4ace's r09, about 02:50.
- **01:03–01:06, the first builder:** yes to #2842's narrowed done-when ("with Harness set aside the lamp says hand-overs can be set up later, and with Harness signed in they work"). Slot: lamp-75c9 05:30–05:54 (the 01:06 table). Wrote `run.sh` (`fec1b8bf…`): one root step that sets Harness aside, puts it back on my signal, and undoes itself at the latest after 24 min. Tested against stand-ins.
- **01:1x, a fault:** my lock `try` on 75c9 was refused (Ten held it), but a `;` let an scp put three files in `/var/tmp/blue-2842`. Nothing was applied. Told Ten on #2602; every lamp command now follows `try &&`.
- **01:3x, #3061:** rebased onto the stack at `cc8aebf3f` (reviewer-three's clean read), now `b9c95f045` (`=`). The gate passes, with logs kept by head.
- **01:5x, the first builder's ask: read #3102's endpoint side** (`e293f8e2e`). No P1 and no P2, one P3: the join's gap is measured from the End decision, so the dials widen the pause `JOIN_WITHIN` allows (about 3.6 s, 4.2 s and 4.5 s). Recommended measuring from the last speech, or naming it in the doc.
- **02:06–02:1x, train 6 merged** (#3102, the voice stack) as lamp-voice `001a3d714`. As the first builder asked, rebased #3061 onto it and retargeted to `lamp-voice`; it is `aa4b66b8d` (`=`). The local gate passes with logs kept, and its own ARM64 and clippy are running for train 8.
- **02:32–02:3x, #3061:** reviewer-two's full read found no P1 or P2, with P3s on CLI-pair coverage and the body. Refreshed the body to the current base and gate, with its limits. It is in train 8 (#3112) with #3097, #3016, #3103 and #3109. Linux clippy is green and ARM64 is running.
- **02:40, the slots changed:** #2842 moves to lamp-75c9 04:58–05:22 (for Jarvis's bedtime QUIET from 05:25). `run.sh` now has a 20-minute hard end (`77fef34c…`), so lamp-tasks is back by 05:18. Train 8 (#3112) carries #3061, and reviewer-two and reviewer-three read the carry clean.
- **02:53, train 8 (#3112) merged** as lamp-voice `7b3e6aef8` with #3061: `turn_end_replay --unfinished-below/--unsure-ms`, using the voice's own parsers and `with_dials`. The first builder's 22:05 piece is done.
- **02:59, the first builder's next piece:** r08 on lamp-4ace (build F) heard 12 of 50 lines in more than one piece. Read each split's endpoint decision, say which dial would have kept it whole, and propose one setting for r10 (06:27). At 03:0x asked for the requests' End records, the voice's policy line and the 12 played clips.
- **03:1x, r08's splits answered:** 18 cuts in 12 lines: 7 short name pieces at the 600 ms window, 7 confident complete verdicts between sentences (pauses 1.7–2.1 s), 2 unfinished holds that ran out (pauses 2.7 and 5.1 s), and 2 desk lines. No dial setting within a few hundred ms keeps any whole. `UNSURE_MS=1200` saved nothing and added 600 ms to 3 line ends; the unfinished hold kept 2 lines. Proposed for r10: UNSURE off, UNFINISHED_BELOW 0.5 kept. Asked for the whole round's End counts and the let-go lines' reasons.
- **03:11, the first builder took the r10 proposal:** `LAMP_ENDPOINT_UNSURE_MS` off, with `UNFINISHED_BELOW=0.5` and `LEAST_SPEECH` 500 unchanged. r08 had 86 Ends: 42 complete, 20 incomplete, 13 uncertain, 10 window. 5 holds kept a line. The 4 let-go pieces had no voice check and no face, so r09 runs `LAMP_SAME_VOICE_MODEL`. **03:13, reviewer-two's post-merge gate on #3061:** 4,390/0/14.
- **03:36–03:4x, the first builder's piece, CAM++ on r09:** 23 cosine judgments matched to voices. Same voice scored 0.32–0.63 (12), another voice 0.02–0.26 (11), and `DIFFERENT_BELOW` 0.30 was right in all 23. The 9 pieces let go are `SAME_FROM` 0.70 being out of CAM++'s reach on mic2 pieces. Recommended no `SAME_FROM` change on r09 alone, and a labelled pair set of the kit's voices through CAM++ offline before setting it.
- **03:42–04:0x, #3123:** the `same_voice_pairs` example for CAM++ bars on labelled clips, cut and printed as the voice does. It was tried on 30 Piper clips with the pinned model. The first builder runs it on the kit's 50 clips and posts numbers.
- **03:55–04:0x, #3123 reads:** Fourteen and reviewer-two found no P1 or P2. Folded reviewer-two's three P3s: a full-range `best_bar`, numeric-only pair ids, and a 10 ms-aligned `--end-silence-ms`. Now `f8dba3386`. Fourteen's P3: run the kit at 300 and 600 ms.
- **04:02, the kit's 50 clean clips through #3123 (the first builder):** same voice median 0.51–0.53, another voice 0.09. `DIFFERENT_BELOW` 0.30 misses 3.7% / 5.9%. `SAME_FROM` 0.70 is reached by 0 of 18 one-line pieces. Next: the same table from mic2 after 10:00. At 04:1x proposed reading `lamp-bus record` audio in Rust, cut by the kit's play log, and asked where the play times come from. **04:06:** reviewer-two closed #3123's three P3s, with no P1 or P2.
- **04:1x, #3125 (stacked on #3123):** `same_voice_pairs --bus mic2_audio.jsonl --plays plays.tsv [--offset-ms]` reads a mic2 recording in Rust, cut by play times. It counts silent clips and lost blocks. On a stand-in recording it matched the WAV run. Ready for the mic2 round after 10:00. #3123 is in train 12.
- **04:13–04:20:** the kit logs its plays in `night/<round>.json` (Unix seconds). #3125's `--plays` now reads that log directly, with output identical to the TSV form. Now `b8a4ac51b`. Posted the one command for the mic2 round after 10:00.
- **04:22–04:38, #3125:** Fourteen found no P1, one P2 (a clip window could take the next play's line) and two P3s. Folded: clips are bounded by their neighbours, losses are counted inside the clips, and the doc keeps the 300 ms lead. On a 0.4 s-gap stand-in, stray too-short pieces went from 13 to 4. **04:2x:** `cargo clean` freed 200 GB (the disk was at 92%). **04:38:** #2842 is ready: `run.sh` (`54308d4b…`) also prints the voice's lines, and the ask and the yes are rendered with Piper (a made voice).
- **04:27–04:41:** Queue #2602 is full (2,500 comments), and the Queue is now **#3126**. Train 12 merged #3123 as `4f4379770`. #3125 is rebased onto lamp-voice as `b7f9c2e0a` (`=`) and posted NEXT TRAIN on #3126.
- **04:44–04:48, #2842's row on lamp-75c9** (the helper ran `run.sh` as root; the lock was mine; asks played in Piper's voice). Harness set aside: `no_harness` in 8 ms, and the lamp answered aloud (the wording is unverified). Harness signed in: `confirm_agent_task` ran, and a person-started Harness was working on the laptop at 04:47:21. Undone with 0 drop-ins, the lock released, and the recordings deleted. Recommended closing.
- **04:48–04:54, #3125:** reviewer-two found no P1 or P2 and two P3 areas. Folded as `ed3f94644`: base64 padding only at the end, an odd PCM byte is an error (`samples_of`, two model-free tests), and the counts are called diagnostics, not proof of the offset (`at_unix_ms` is publication time). Gate green; the stand-in output is unchanged.
- **04:51–04:55, #2842:** the first builder wants request 1's spoken words before closing. Read-only look at 75c9 under the lock (released): the journal has only piece sizes. `lamp-tasks`' refusal text is "Harness is not installed on this lamp; hand-overs are off until it is.", which has no set-up-later wording. The words are in the memory store (root-only, no sqlite3 on 75c9): asked my person for a read-only [hand-off].
- **04:56–05:04, #2842's words and #3131:** the helper's read-only read of the memory store gave request 1's line: "It wouldn't go through just now … want me to try again?" (alias left out), not set-up-later. Cause: the voice tells the model every prepare refusal as "could not be prepared just now". #3131 (`blue/no-harness-later`, `0ebe35a7f`): `no_harness` is told `NO_HARNESS` ("…can be set up later."), test first; lamp-voice 3535/0. Lamp check after it lands: the set-aside half again.
- **05:04–05:06:** reviewer-two's correction read of #3125 `ed3f94644` found no P1 or P2, and both P3s are closed. Their ask, the latest-base combined gate, passes: merged on lamp-voice `4afe078e9` (train 13), fmt, strict clippy, voice-eval 378/7/238, the example 2/0. Own-head ARM64 is green, so it's ready for train 14. Train 13 didn't touch #3131's files.
- **05:06–05:10:** #3125 rides train 14 (#3132) on `4afe078e9`. #3131: the first builder asked Eighth to read it. I answered both questions on the PR ahead of the read: only `no_harness` changes, and transient refusals keep "just now". I flagged `targets_newer` as the one other code a retry can't fix, left out unless asked. Ten's `judged_without` gap is Ten's; `late_other` already covers a late other-voice print.
- **05:14–05:19:** Eighth's read of #3131 found no P1 or P2. Their P3 (`targets_newer` is lasting too) is up as #3134 (`blue/targets-newer-later`, `627bf118d`, stacked on #3131): `TARGETS_NEWER`, "…will work again once the lamp is updated", test first; lamp-voice 3535/0. #3125 is in train 14 (#3132).
- **05:17–05:19:** train 14 merged #3125 as lamp-voice `4f20e7b39`. #3131 is in train 15 (#3133), own-head ARM64 and clippy green. #3134 waits for #3131 to land, then I'll retarget it to lamp-voice and push a no-change amend to start CI.
- **05:20–05:24:** Fourteen read #3134 with no P1, P2 or P3 (55/0 on `agent_tasks`). The first builder: Eighth reads it after the rebase onto lamp-voice, once train 15 (#3133, with #3131) merges. #3131 + #3134 + Eighth's #3135 merge together on lamp-voice with no conflict in `agent_tasks.rs` or `tasks.md`. reviewer-two holds #3135's read.
- **05:24–05:29:** Eighth read #3134 with no P1, P2 or P3, so it has two clean reads (with Fourteen's). It waits only on train 15 (#3133, ARM64 running); then the rebase onto lamp-voice and NEXT TRAIN.
- **05:30–05:35:** train 15 merged #3131 as lamp-voice `485b870f1`. #3134 is rebased onto it as `543e91846` (range-diff `=`, same patch-id) and retargeted to lamp-voice; train 16 (#3136) carries it. #2842's re-check after 07:40 is ready: `run.sh` (`6035c33d…`) runs train 16's lamp-voice through a `/run` drop-in and lamp-tasks set aside, then reads the lamp's words read-only and puts everything back. A silent look at 75c9's units under the lock (released): one voice binary, no drop-ins.
- **05:34–05:40:** no new asks for blue. Train 16 (#3136, with #3134): ARM64 running. Train artifacts are `lampos-arm64-<head sha>`, flat, with `SHA256SUMS` (train 15's `lamp-voice` checked: `cf78bc56…`). After train 16 lands I'll take its `lamp-voice` for #2842's re-check after 07:40.
