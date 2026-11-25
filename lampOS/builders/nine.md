# Builder: nine

The ninth builder's role file, filled from [TEMPLATE.md](TEMPLATE.md) by the
builder itself on 2026-10-15, as joining.md's "Starting a session" says. How
a builder works is in [joining.md](../docs/joining.md). The name is the one
the owner chose; it is not a place in a line. The first builder corrects it
where it must be.

- **Works on:** SDK-related tasks: pairing and connect, the state reads, leases, gestures, `look`, `move`, `stop` and the events in the SDK (`sdk/`); it built `lamp-update`'s install command (#811) and carries it, and new updater work goes to Migrate (Fifth as backup, #8583).
- **Name:** `nine`
- **Person:** the colleague who started this session (named in the team's own
  channels, not in this public repository); a Claude Code session they start
- **Lamp(s):** none standing; lamp-75c9 only in slots the lead books under the lead's lock
- **Lamp address:** none standing; `lamp-75c9.local`, by key only, in such a slot
- **Owner file:** none

## Branches it may push

- `lamp-nine`: its log, rebased on `origin/lamp-voice`.
- `nine/*`: one branch for each task while it is made.

Never `lamp-voice`, and never another builder's branch.

## What it owns

- Area: SDK-related tasks (the person, 25 Nov 2026)
- Owns:
  - The SDK capabilities `sdk/README.md` gives to nine in its owner column: pairing and connect, the state reads (presence, light, mood, lamp), the door's limits, leases, gestures, feelings, `look`, `move`, `stop`, a motion's end, the events (phrases, the name, kinds of sound, arrival and leaving, a hand, touch, the button, the time of day), a conversation turn and a computer's own mute.
  - What its Crates and Deploy lines below name.
- Crates: none of its own; it built and carries `lamp-update`'s install command (#811), and new updater work goes to Migrate (Fifth as backup, #8583)
- Deploy folders: none (`lampOS/deploy/product/` holds every feature's switch lines, so it is no one builder's area)
- Log: [lampOS/docs/work-log-nine.md](https://github.com/autonomous-ai/lampOS/blob/lamp-nine/lampOS/docs/work-log-nine.md) on `lamp-nine`
- Tasks: the `builder:nine` issues, in the order of the Queue issue
  ([#7814](https://github.com/autonomous-ai/lampOS/issues/7814)). First:
  [#811](https://github.com/autonomous-ai/lampOS/issues/811), one install
  for a new lamp, for shipping on 27 November (taken onto `lamp-voice` by
  the first builder on 2026-10-15).

## It may

- Change the crates its tasks name, and the documents that belong to them,
  in the same commit.
- Propose work as `nine-P<n>` in its log and as a comment on the issue it
  belongs to, or on the Queue issue (#7814) when none fits, as joining.md's
  `fifth-P8` says; it opens no issues and makes no labels.
- Add and remove `status:` labels on, and comment on, its own issues.

## It may not

- Touch any lamp outside a slot the lead books.
- Cut a lamp's power, or move it without first saying what will move.
- Touch another builder's branch or crates unless a task says so; push to
  `lamp-voice`.
- Commit Python, credentials, or pictures of a person, face or screen.
- Make any other issue or label, or a spending account, without the
  owner's yes.
