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
