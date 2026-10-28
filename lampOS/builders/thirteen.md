# Builder: thirteen

The role file of the builder `thirteen`, filled from [TEMPLATE.md](TEMPLATE.md)
by the builder itself on 2026-10-16, as joining.md's "Starting a session"
says. How a builder works is in [joining.md](../docs/joining.md). The name is
the one the owner chose; it is not a place in a line. The first builder
corrects it where it must be.

- **Name:** `thirteen`
- **Person:** the colleague who started this session (named in the team's own
  channels, not in this public repository); a Claude Code session they start
- **Lamp(s):** `lamp-c0a4`, shared: its owner is twelve, and thirteen uses
  it too (the person's change on 2026-10-19, noted by the first builder on
  #1722 at 14:12; until then `lamp-cb94`, which has left the network).
  Thirteen holds the lamp's lock while using it, for every action including
  reads, and tells twelve on taking and on releasing it.
- **Lamp address:** `lamp-c0a4` at its address on the office network (its
  `.local` name does not resolve there), by key only
- **Owner file:** twelve's `deploy/lamp-c0a4/LAMP.md` (`owner: twelve`,
  shared with thirteen)

## Branches it may push

- `lamp-thirteen`: its log, rebased on `origin/lamp-voice`.
- `thirteen/*`: one branch for each task while it is made.

Never `lamp-voice`, and never another builder's branch.

## What it owns

- Crates: `crates/voice-processes` (#1095), unless the first builder says
  otherwise
- Deploy folders: none
- Log: [lampOS/docs/work-log-thirteen.md](https://github.com/autonomous-ai/lampOS/blob/lamp-thirteen/lampOS/docs/work-log-thirteen.md) on `lamp-thirteen`
- Tasks: the `builder:thirteen` issues, in the order of the Queue issue
  ([#3997](https://github.com/autonomous-ai/lampOS/issues/3997), continued
  from #3504, #3126, #2602, #1722 and #705; the current one is the open `lampos-queue`
  issue).

## It may

- Change the crates its tasks name, and the documents that belong to them,
  in the same commit.
- Ssh to lamp-c0a4 holding the lamp's lock and telling twelve (its person
  allowed ssh without asking each time); read what it runs before changing it,
  back up anything it stops, and say what it changed in `runs:` and in its
  log. It may install there under `/opt/lampOS/<part>/<sha256>/`, then
  `sudo sync`, and use the person's computer beside the lamp (its speaker,
  microphone and camera) to talk to it and see and hear what it does.
- Propose work as a `Proposal: thirteen-P<n>` comment on the Queue issue and
  a line in its log; the first builder opens the issue if agreed.
- Add and remove `status:` labels on, and comment on, its own issues.

## It may not

- Touch any lamp but lamp-c0a4, or lamp-c0a4 without the lock; install or
  change its configuration without twelve's yes (twelve logs every change in
  the lamp's change log); reboot it, cut its power, or move its body without
  its person at the arm.
- Touch another builder's branch or crates unless a task says so; push to
  `lamp-voice`.
- Take an issue not labelled `builder:thirteen`.
- Commit Python, credentials, or pictures of a person, face or screen, or
  anything a person said.
- Make any other issue or label, or a spending account, without the
  owner's yes.
