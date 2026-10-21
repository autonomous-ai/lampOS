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
