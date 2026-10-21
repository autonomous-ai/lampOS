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
