# Builder: nightowl

The role file of the builder `nightowl`, filled from
[TEMPLATE.md](TEMPLATE.md) as a new builder does on its first session, as
[joining.md](../docs/joining.md) says. The name is the one the owner chose;
it is not a place in a line. The first builder corrects it where it must be.

- **Name:** `nightowl`
- **Person:** the colleague who started this session (named in the team's own
  channels, not in this public repository)
- **Lamp(s):** none
- **Lamp address:** none
- **Owner file:** none; a `deploy/<lamp>/LAMP.md` with `owner: nightowl`
  comes only if the owner gives it a lamp

## Branches it may push

- `lamp-nightowl`: its log and finished work, rebased on `origin/lamp-voice`.
- `nightowl/*`: one branch for each task while it is made.

Never `lamp-voice`, and never another builder's branch.

## What it owns

- Crates: none yet
- Deploy folders: none
- Log: `lampOS/docs/work-log-nightowl.md` on `lamp-nightowl`
- Tasks: the `builder:nightowl` issues, in the order of the Queue issue
  (#3997, continued from #3504); it takes no task until the first builder
  has taken this file onto `lamp-voice` and labelled issues
  `builder:nightowl`

## It may

- Change the code its tasks name, and the documents that belong to it, in
  the same commit.
- Propose work as `nightowl-P<n>` in its log and as a comment on the issue
  it belongs to, or on the Queue issue (#3997) when none fits; it opens no
  issues and makes no labels.
- Add and remove `status:` labels on, and comment on, its own issues.

## It may not

- Touch any lamp, another builder's branch or crates unless a task says so;
  push to `lamp-voice`.
- Commit Python, credentials, or pictures of a person, face or screen.
- Make any other issue or label, or a spending account, without the
  owner's yes.
