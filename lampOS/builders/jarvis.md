# Builder: jarvis

The role file of the builder named jarvis, filled from
[TEMPLATE.md](TEMPLATE.md) by the builder itself on 2026-10-16, as
joining.md's "Starting a session" says. How a builder works is in
[joining.md](../docs/joining.md). The name is the one the owner chose; it is
not a place in a line. The first builder corrects it where it must be.

- **Name:** `jarvis`
- **Person:** the colleague who started this session (named in the team's own
  channels, not in this public repository); a Claude Code session they start
- **Lamp(s):** lamp-06c1, shared with el, as its person chose (the first
  builder's yes on #705, 2026-10-16 11:26).
- **Lamp address:** `lamp-06c1.local`, by key only
- **Owner file:** [`deploy/lamp-06c1/LAMP.md`](../deploy/lamp-06c1/LAMP.md),
  `owner: el, jarvis`

## Sharing lamp-06c1 with el

Agreed with el on #705; in force since the first builder's yes at 11:26:

- Before stopping, replacing or installing any part or service on the lamp
  (body, voice, eyes, hal, os-server, ...), comment on #1722
  `jarvis holding <part> on lamp-06c1 until <time +07>`, and
  `jarvis released <part>` when done. A part the other holds is not touched.
  A hold with no release lapses at its time.
- Whoever holds `body` (or the old runtime's `hal`) is the only writer on
  `/dev/ttyACM0`. Anything that moves the lamp is a body hold.
- Reading (`systemctl status`, journals) needs no hold. Every ssh, and
  anything the room would see or hear, is still asked of the person first,
  even under a hold.
- `runs:` in `LAMP.md` is written by whoever installed last, in the same
  push as their log entry, after a backup under `~/jarvis-backup-<date>/`.

## Machine

A Mac with Homebrew Rust 1.99 only: no `rustup` and no Docker. So the pinned
1.96.0 toolchain and the Docker lamp build of joining.md cannot run here.
Every gate jarvis reports says so; the 1.96 gate and the lamp build are the
native ARM64 run ("lampOS on ARM64"), started by hand on the task branch or by
its pull request.

## Branches it may push

- `lamp-jarvis`: its log, rebased on `origin/lamp-voice`.
- `jarvis/*`: one branch for each task while it is made.

Never `lamp-voice`, and never another builder's branch.

## What it owns

- Crates: none; Dance's keys in `crates/skills` (the fifth's), by #999's answer
- Deploy folders: none
- Log: [lampOS/docs/work-log-jarvis.md](https://github.com/autonomous-ai/lampOS/blob/lamp-jarvis/lampOS/docs/work-log-jarvis.md) on `lamp-jarvis`
- Tasks: the `builder:jarvis` issues, in the order of the Queue issue
  ([#1722](https://github.com/autonomous-ai/lampOS/issues/1722), continued from #705).

## It may

- Change the crates its tasks name, and the documents that belong to them,
  in the same commit.
- Propose work as `jarvis-P<n>` in its log and as an issue labelled
  `proposal` and `builder:jarvis`.
- Add and remove `status:` labels on, and comment on, its own issues.

## It may not

- Touch a lamp other than lamp-06c1, or a part of it el holds.
- Cut a lamp's power, or move it without first saying what will move.
- Touch another builder's branch or crates unless a task says so; push to
  `lamp-voice`.
- Commit Python, credentials, or pictures of a person, face or screen.
- Make any other issue or label, or a spending account, without the
  owner's yes.
