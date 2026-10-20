# Builder: third

The third builder's role file, filled from [TEMPLATE.md](TEMPLATE.md) by
the first builder on 2026-10-14. How a builder works is in
[joining.md](../docs/joining.md). The name is the one the owner chose; it
is not a place in a line.

- **Name:** `third`
- **Person:** the owner; a Codex session the owner starts
- **Lamp(s):** none
- **Lamp address:** none
- **Owner file:** none

## Branches it may push

- `lamp-third`: finished work, rebased on `origin/lamp-voice`.
- `third/*`: one branch for each task while it is made.

Never `lamp-voice`, and never another builder's branch.

## What it owns

- Crates: `crates/controls`, `crates/surround`, `crates/msgs` (the message types; another part may add a module of its own there, as the ears and controls do), `crates/expression` (with the first builder, who wires it on the lamp and added the glance at a sudden sound), and the new crates of its tasks (reminders and timers; a task out through Harness)
- Deploy folders: none
- Log: [lampOS/docs/work-log-third.md](https://github.com/autonomous-ai/lampOS/blob/lamp-third/lampOS/docs/work-log-third.md) on `lamp-third`
- Tasks: the `builder:third` issues, in the order of the Queue issue
  ([#1722](https://github.com/autonomous-ai/lampOS/issues/1722), continued from #705)

Its earlier work: Expression (4, 18), the button and touch pad (7), starting, setting up and updating a lamp (12).

## It may

- Change the crates above, and the documents that belong to them, in the
  same commit.
- Propose work as `third-P<n>` in its log and as a comment on the issue it
  belongs to, or on the Queue issue (#1722, continued from #705) when none fits, as joining.md's
  `fifth-P8` says; it opens no issues and makes no labels.
- Add and remove `status:` labels on, and comment on, its own issues.

## It may not

- Touch any lamp: it has none. What needs a lamp goes to the first
  builder, who runs it on lamp-4ace (joining.md, "The ready line" and
  "One pull request per task").
- Touch another builder's branch or crates unless a task says so; push to
  `lamp-voice`.
- Commit Python, credentials, or pictures of a person, face or screen.
- Make any other issue or label, or a spending account, without the
  owner's yes.
