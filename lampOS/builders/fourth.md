# Builder: fourth

The fourth builder's role file, filled from [TEMPLATE.md](TEMPLATE.md) by
the first builder on 2026-10-14. How a builder works is in
[joining.md](../docs/joining.md). The name is the one the owner chose; it
is not a place in a line.

- **Name:** `fourth`
- **Person:** the owner; a Codex session the owner starts
- **Lamp(s):** none
- **Lamp address:** none
- **Owner file:** none

## Branches it may push

- `lamp-fourth`: finished work, rebased on `origin/lamp-voice`.
- `fourth/*`: one branch for each task while it is made.

Never `lamp-voice`, and never another builder's branch.

## What it owns

- Crates: `crates/memory`, `crates/records`, `crates/lean`, and in `crates/policy` the local judge's own files (the policy itself is the first builder's), and the crates its tasks name (a new settings part; the log message in `crates/bus` and `crates/msgs`, by the task's leave)
- Deploy folders: none
- Log: [lampOS/docs/work-log-fourth.md](https://github.com/autonomous-ai/lampOS/blob/lamp-fourth/lampOS/docs/work-log-fourth.md) on `lamp-fourth`
- Tasks: the `builder:fourth` issues, in the order of the Queue issue
  ([#1722](https://github.com/autonomous-ai/lampOS/issues/1722), continued from #705)

Its earlier work: memory across days (13), whom it is said to (14), the records bound (15), the lean check (16), the local judge (17), the voice on the hand controls (21).

## It may

- Change the crates above, and the documents that belong to them, in the
  same commit.
- Propose work as `fourth-P<n>` in its log and as a comment on the issue it
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
