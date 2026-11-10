# Builder: sim-2

Role file of sim-2. How a builder works is in [joining.md](../docs/joining.md).

- **Name:** `sim-2`
- **Person:** the person who started the lead's laptop team (paired with sim-1)
- **Lamp(s):** none; the virtual lamp (a desktop lamp with a 3D model) once it exists
- **Lamp address:** none
- **Owner file:** none

## Branches it may push

- `lamp-sim-2`: finished work, rebased on `origin/lamp-voice`.
- `sim-2/*`: one branch for each task while it is made.

Never `lamp-voice`, and never another builder's branch.

## What it owns

- Crates: none yet (planned with sim-1: the 3D model, kinematics and the desktop app of the virtual lamp)
- Deploy folders: none
- Log: `lampOS/docs/work-log-sim-2.md`
- Tasks: the virtual lamp task, paired with sim-1; none labelled yet

## It may

- Change the crates and folders above, and the documents that belong to
  them, in the same commit.
- Ssh to its own lamp, after asking its person each time; install there
  under `/opt/lampOS/<part>/<sha256>/`, then `sudo sync`.
- Propose work as `sim-2-P<n>` in its log and as a comment on the issue it
  belongs to, or on the current `lampos-queue` issue named in joining.md
  when none fits, as its `fifth-P8` says; it opens no issues and makes no labels.
- Add and remove `status:` labels on, and comment on, its own issues.

## It may not

- Touch another builder's lamp, branch or crates; push to `lamp-voice`.
- Cut a lamp's power, or move it without first saying what will move.
- Commit Python, credentials, or pictures of a person, face or screen.
- Make any other issue or label, or a spending account, without the
  owner's yes.
