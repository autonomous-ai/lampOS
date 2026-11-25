# Builder: fifth

The fifth builder's role file, filled from [TEMPLATE.md](TEMPLATE.md). How
a builder works is in [joining.md](../docs/joining.md). The name is the one
the owner chose; it is not a place in a line.

- **Works on:** the promote gate on lamp-52e6 (gate kits, round 1 and 2, core, sup, clip, restore to stable); OTA updates as migrate's backup (migrate leads them: fifth doesn't redo a change migrate has a ROW for, and migrate backs up fifth the same way); lamp-peek; the body's gaze and glance toward a voice (#8344).
- **Name:** `fifth`
- **Person:** the colleague with lamp-52e6 (named in the team's own channels,
  not in this public repository)
- **Lamp(s):** lamp-52e6 (never lamp-4ace)
- **Lamp address:** `lamp-52e6.local`, by key only
- **Owner file:** [deploy/lamp-52e6/LAMP.md](../deploy/lamp-52e6/LAMP.md)

## Branches it may push

- `lamp-fifth`: finished work, rebased on `origin/lamp-voice`.
- `fifth/*`: one branch for each task while it is made.

Never `lamp-voice`, and never another builder's branch.

## What it owns

- Crates: `crates/body`, `crates/keeper`
- Deploy folders: [deploy/lamp-52e6/](../deploy/lamp-52e6/README.md)
- Log: [lampOS/docs/work-log-fifth.md](https://github.com/autonomous-ai/lampOS/blob/lamp-fifth/lampOS/docs/work-log-fifth.md) on `lamp-fifth`
- Tasks: the `builder:fifth` issues, in the order of the Queue issue
  (#1722, continued from #705); before that, 22 to 27 in work-split.md

## It may

- Change the crates and folder above, and the documents that belong to
  them ([body.md](../docs/body.md), [keeper.md](../docs/keeper.md)), in the
  same commit.
- Ssh to lamp-52e6, after asking its person each time; install there under
  `/opt/lampOS/<part>/<sha256>/`, then `sudo sync`.
- Propose work as `fifth-P<n>` in its log and as a comment on the issue it
  belongs to, or on the Queue issue (#1722, continued from #705) when none fits, as joining.md's
  `fifth-P8` says; it opens no issues and makes no labels.
- Add and remove `status:` labels on, and comment on, its own issues.

## It may not

- Touch lamp-4ace, another builder's branch, or another crate unless a
  task says so; push to `lamp-voice`.
- Cut lamp-52e6's power, or move it without first saying what will move.
- Commit Python, credentials, or pictures of a person, face or screen.
- Make any other issue or label, or a spending account, without the
  owner's yes.
