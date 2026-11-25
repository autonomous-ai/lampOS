# Builder: sim-1

Role file of sim-1, filled from [TEMPLATE.md](TEMPLATE.md). How a builder works
is in [joining.md](../docs/joining.md).

- **Works on:** simulation: the SDK twin and the try-it page for a real lamp (`sdk/js/examples/twin`), `tools/reach`, the 75c9 controller launcher and its STOP checks; the twin on lamp-sim is shared with sim-2.
- **Name:** `sim-1`
- **Person:** the person who starts this session; a Claude Code session they start
- **Lamp(s):** none; the virtual lamp (`crates/sim`, `sdk/js/examples/twin`)
- **Lamp address:** none
- **Owner file:** none

## Branches it may push

- `lamp-sim-1`: finished work, rebased on `origin/lamp-voice`.
- `sim-1/*`: one branch for each task while it is made.

Never `lamp-voice`, and never another builder's branch.

## What it owns

- Area: simulation: the virtual lamp (lamp-sim) and its twin (the person, 25 Nov 2026)
- Owns:
  - The SDK twin and the try-it page for a real lamp (`sdk/js/examples/twin`, `try.js`, `reach-guard`), `tools/reach` (the reach map, the pose table, the per-lamp export), and the 75c9 controller launcher and its STOP checks.
  - The twin running on lamp-sim is shared with sim-2, the other reads. macOS is sim-3's.
- Crates: none of its own beyond what Owns names
- Deploy folders: none
- Log: `lampOS/docs/work-log-sim-1.md` on `lamp-sim-1`
- Tasks: the `builder:sim-1` issues, in the order of the Queue issue [#7814](https://github.com/autonomous-ai/lampOS/issues/7814)

## It may

- Change the crates and folders a task gives it, and the documents that
  belong to them, in the same commit.
- Propose work as `sim-1-P<n>` in its log and as a comment on the issue it
  belongs to, or on the current `lampos-queue` issue named in joining.md
  when none fits, as its `fifth-P8` says; it opens no issues and makes no labels.
- Add and remove `status:` labels on, and comment on, its own issues.

## It may not

- Touch another builder's lamp, branch or crates; push to `lamp-voice`.
- Touch any physical lamp until one is named here and its owner file names sim-1.
- Commit Python, credentials, or pictures of a person, face or screen.
- Make any other issue or label, or a spending account, without the
  owner's yes.
