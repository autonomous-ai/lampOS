# Builder: sim-1

The role file of one builder. Copy this file to `builders/sim-1.md`, where
`sim-1` is the identifier the owner chose (not a number or a place in a
line), and fill every line. How a builder works is in
[joining.md](../docs/joining.md).

- **Name:** `sim-1`
- **Person:** whoever started this laptop team's sessions (the lead's laptop)
- **Lamp(s):** none (the virtual lamp, once it exists)
- **Lamp address:** none
- **Owner file:** none

## Branches it may push

- `lamp-sim-1`: finished work, rebased on `origin/lamp-voice`.
- `sim-1/*`: one branch for each task while it is made.

Never `lamp-voice`, and never another builder's branch.

## What it owns

- Crates: none yet (the virtual lamp's runtime and host devices once the design is agreed; paired with sim-2)
- Deploy folders: none
- Log: `lampOS/docs/work-log-sim-1.md`
- Tasks: the virtual desktop lamp, design first, with sim-2

## It may

- Change the crates and folders above, and the documents that belong to
  them, in the same commit.
- Ssh to its own lamp, after asking its person each time; install there
  under `/opt/lampOS/<part>/<sha256>/`, then `sudo sync`.
- Propose work as `sim-1-P<n>` in its log and as a comment on the issue it
  belongs to, or on the current `lampos-queue` issue named in joining.md
  when none fits, as its `fifth-P8` says; it opens no issues and makes no labels.
- Add and remove `status:` labels on, and comment on, its own issues.

## It may not

- Touch another builder's lamp, branch or crates; push to `lamp-voice`.
- Cut a lamp's power, or move it without first saying what will move.
- Commit Python, credentials, or pictures of a person, face or screen.
- Make any other issue or label, or a spending account, without the
  owner's yes.
