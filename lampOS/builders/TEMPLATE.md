# Builder: <name>

The role file of one builder. Copy this file to `builders/<name>.md`, where
`<name>` is the identifier the owner chose (not a number or a place in a
line), and fill every line. How a builder works is in
[joining.md](../docs/joining.md).

- **Name:** `<name>`
- **Person:** <who starts its sessions and lends hands at the lamp>
- **Lamp(s):** <`lamp-xxxx`, or none>
- **Lamp address:** <`lamp-xxxx.local`>, by key only
- **Owner file:** <`deploy/lamp-xxxx/LAMP.md`, with `owner: <name>`>

## Branches it may push

- `lamp-<name>`: finished work, rebased on `origin/lamp-voice`.
- `<name>/*`: one branch for each task while it is made.

Never `lamp-voice`, and never another builder's branch.

## What it owns

- Crates: <`crates/...`>
- Deploy folders: <`deploy/lamp-xxxx/`, or none>
- Log: `lampOS/docs/work-log-<name>.md`
- Tasks: <the numbers in work-split.md, or the `builder:<name>` issues>

## It may

- Change the crates and folders above, and the documents that belong to
  them, in the same commit.
- Ssh to its own lamp, after asking its person each time; install there
  under `/opt/lampOS/<part>/<sha256>/`, then `sudo sync`.
- Propose work as `<name>-P<n>` in its log and as a comment on the issue it
  belongs to, or on the Queue issue (#1722, continued from #705) when none fits, as joining.md's
  `fifth-P8` says; it opens no issues and makes no labels.
- Add and remove `status:` labels on, and comment on, its own issues.

## It may not

- Touch another builder's lamp, branch or crates; push to `lamp-voice`.
- Cut a lamp's power, or move it without first saying what will move.
- Commit Python, credentials, or pictures of a person, face or screen.
- Make any other issue or label, or a spending account, without the
  owner's yes.
