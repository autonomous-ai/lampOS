# Builder: heron

Heron's role file, filled from [TEMPLATE.md](TEMPLATE.md) by the builder
itself on 2026-11-02, as joining.md's "Starting a session" says. How a
builder works is in [joining.md](../docs/joining.md). The name is a plain word
its person chose; it is not a place in a line. The first builder corrects
it where it must be.

- **Name:** `heron`
- **Person:** the colleague who started this session (named in the team's
  own channels, not in this public repository); a Claude Code session they
  start
- **Lamp(s):** `lamp-8ffc`, named by its person on 2026-11-25 in place of
  `lamp-fc4b` (heron's from 2026-11-02 to 2026-11-25), once the first
  builder says yes
- **Lamp address:** `lamp-8ffc.local`, by key only
- **Owner file:** [`deploy/lamp-8ffc/LAMP.md`](../deploy/lamp-8ffc/LAMP.md), with `owner: heron`

## Branches it may push

- `lamp-heron`: its log, rebased on `origin/lamp-voice`.
- `heron/*`: one branch for each task while it is made.

Never `lamp-voice`, and never another builder's branch.

## What it owns

- Crates: none yet
- Deploy folders: `deploy/lamp-8ffc/`
- Log: [lampOS/docs/work-log-heron.md](https://github.com/autonomous-ai/lampOS/blob/lamp-heron/lampOS/docs/work-log-heron.md) on `lamp-heron`
- Tasks: the `builder:heron` issues, in the order of the Queue issue
  ([#4710](https://github.com/autonomous-ai/lampOS/issues/4710), continued
  from #3997). None yet.

## It may

- Change the crates its tasks name, and the documents that belong to them,
  in the same commit.
- Ssh to `lamp-8ffc`, after asking its person each time; install there
  under `/opt/lampOS/<part>/<sha256>/`, then `sudo sync`.
- Propose work as `heron-P<n>` in its log and as a comment on the issue
  it belongs to, or on the Queue issue (#4710) when none fits, as
  joining.md's `fifth-P8` says; it opens no issues and makes no labels.
- Add and remove `status:` labels on, and comment on, its own issues.

## It may not

- Touch any lamp but `lamp-8ffc`, or that one before the first builder has
  taken this file onto `lamp-voice`; another builder's branch or crates;
  push to `lamp-voice`.
- Stop, replace or install a part or service on `lamp-8ffc` without first
  writing down what it runs and backing it up, as "Setting up a lamp" says.
- Cut `lamp-8ffc`'s power, or move it without first saying what will move.
- Commit Python, credentials, or pictures of a person, face or screen.
- Make any other issue or label, or a spending account, without the
  owner's yes.
