# Builder: banana

Role file of banana, filled from [TEMPLATE.md](TEMPLATE.md). How a builder works
is in [joining.md](../docs/joining.md).

- **Works on:** testing the SDK on the virtual lamp (lamp-sim): both SDKs, their tests and every example run against it, and fixing what that shows (`sdk/`, `tools/lamp-sim-check.sh`, `tools/lamp-sim-conformance.sh`, `sdk/js/conformance/`).
- **Name:** `banana`
- **Person:** the person who starts this session; a Claude Code session they start
- **Lamp(s):** none; the virtual lamp (`crates/sim`, `sdk/js/examples/twin`)
- **Lamp address:** none
- **Owner file:** none

## Branches it may push

- `lamp-banana`: finished work, rebased on `origin/lamp-voice`.
- `banana/*`: one branch for each task while it is made.

Never `lamp-voice`, and never another builder's branch.

## What it owns

- Area: testing the SDK on the simulator: it makes sure the SDK works on the virtual lamp (lamp-sim) (the person, 25 Nov 2026)
- Owns:
  - The SDK-on-lamp-sim checks: running the Python and JavaScript SDKs, their tests and every example against the virtual lamp (`tools/lamp-sim-check.sh`, `tools/lamp-sim-conformance.sh`, `sdk/js/conformance/`), and fixing what they show in the SDK, its examples and its documents.
  - The SDK capabilities `sdk/README.md` gives to banana in its owner column: a computer's own notes on the lamp, and the microphone's audio (local only).
- Crates: none yet; the crates and folders a task names
- Deploy folders: none
- Log: `lampOS/docs/work-log-banana.md` on `lamp-banana`
- Tasks: the `builder:banana` issues, in the order of the current `lampos-queue` issue named in joining.md

## It may

- Change the crates and folders a task gives it, and the documents that
  belong to them, in the same commit.
- Propose work as `banana-P<n>` in its log and as a comment on the issue it
  belongs to, or on the current `lampos-queue` issue named in joining.md
  when none fits, as its `fifth-P8` says; it opens no issues and makes no labels.
- Add and remove `status:` labels on, and comment on, its own issues.

## It may not

- Touch another builder's lamp, branch or crates; push to `lamp-voice`.
- Touch any physical lamp until one is named here and its owner file names banana.
- Commit Python, credentials, or pictures of a person, face or screen.
- Make any other issue or label, or a spending account, without the
  owner's yes.
