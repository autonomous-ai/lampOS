# Builder: second

The second builder's role file, filled from [TEMPLATE.md](TEMPLATE.md) by
the first builder on 2026-10-14. How a builder works is in
[joining.md](../docs/joining.md). The name is the one the owner chose; it
is not a place in a line.

- **Name:** `second`
- **Person:** the owner; a Claude Code session on the owner's laptop
- **Lamp(s):** none
- **Lamp address:** none
- **Owner file:** none

## Branches it may push

- `lamp-second`: finished work, rebased on `origin/lamp-voice`.
- `second/*`: one branch for each task while it is made.

Never `lamp-voice`, and never another builder's branch.

## What it owns

- Crates: none of its own: those its task names
- Deploy folders: none
- Log: [lampOS/docs/work-log.md](https://github.com/autonomous-ai/lampOS/blob/lamp-second/lampOS/docs/work-log.md) on `lamp-second`
- Tasks: the `builder:second` issues, in the order of the Queue issue
  ([#3997](https://github.com/autonomous-ai/lampOS/issues/3997), continued from #3504)

Its earlier work: the end-of-turn model's speed (1), the stop listener (3), the replay of the lamp's recordings (20), the end sent early (19), the reference that follows the microphone's clock (10).

## It may

- Change the crates above, and the documents that belong to them, in the
  same commit.
- Propose work as `second-P<n>` in its log and as a comment on the issue it
  belongs to, or on the Queue issue (#3997, continued from #3504) when none fits, as joining.md's
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
