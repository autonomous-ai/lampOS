# Builder: sixth

The sixth builder's role file, filled from [TEMPLATE.md](TEMPLATE.md) by the
sixth builder itself on 2026-10-15, as joining.md's "Starting a session"
says. How a builder works is in [joining.md](../docs/joining.md). The name is
the one the owner chose; it is not a place in a line. The first builder
corrects it where it must be.

- **Name:** `sixth`
- **Person:** a colleague (named in the team's own channels, not in this public
  repository); a Claude Code session they start
- **Lamp(s):** shared access to `lamp-52e6`, borrowed from `fifth`; no
  exclusively owned lamp.
- **Lamp address:** `orangepi@lamp-52e6.local`, SSH by the existing key.
- **Owner file:** [lamp-52e6/LAMP.md](../deploy/lamp-52e6/LAMP.md).
  Fifth remains the owner; this role file is not a reservation or a live
  statement of the lamp's availability or installed build.

## Real-lamp tests the first builder may assign

Sixth has already run voice tests on lamp-52e6, including the second route's
Finalize latency, startup/reconnect behaviour and interruptions. The record
is in [Sixth's log](https://github.com/autonomous-ai/lampOS/blob/lamp-sixth/lampOS/docs/work-log-sixth.md)
and, for example, [the second-voice run](../docs/verification/754-lamp-52e6-20261015/).
This supersedes the original "no lamp" description; it does not transfer
ownership from Fifth.

The first builder may assign Sixth end-to-end voice, latency, interruption,
reconnect and related identity tests on this lamp. Sixth can play test speech
from its Mac, collect journals and timings, and restore its loan without
asking the person to speak for routine synthetic-speech tests. Real-speaker
recognition (#1675) still needs consenting participants or consented labelled
recordings; played synthetic voices are not proof of office-speaker accuracy.

Before a run, coordinate the scope and free window with Fifth. The established
loan queue is `~orangepi/loans.txt` on lamp-52e6: request the time, builder,
services and expected duration; use the owner's acknowledgement and current
lamp ownership/lease rules in [joining.md](../docs/joining.md#sharing-a-lamp-eighth-p1-decided-2026-10-15).
Do not infer "free" from this file or an old acknowledgement.

The default loan is voice-only. Agree explicitly with Fifth on any eyes or
memory build/restart needed for an identity test; do not stop the body or
unrelated services. Record the installed builds, back up changed files, keep
credentials out of output, and restore the exact pre-test state. End every
loan with `52e6 back` in the loan queue and a report of build, checks, timings,
failures and unverified behaviour. Only physical interventions need the
person's hands. This role update does not itself borrow or change the lamp.

## Branches it may push

- `lamp-sixth`: finished work, rebased on `origin/lamp-voice`.
- `sixth/*`: one branch for each task while it is made.

Never `lamp-voice`, and never another builder's branch.

## What it owns

- Crates: none yet
- Deploy folders: none
- Log: [lampOS/docs/work-log-sixth.md](https://github.com/autonomous-ai/lampOS/blob/lamp-sixth/lampOS/docs/work-log-sixth.md) on `lamp-sixth`
- Tasks: the `builder:sixth` issues, in the order of the Queue issue
  ([#705](https://github.com/autonomous-ai/lampOS/issues/705)); none yet.
  First: [#754](https://github.com/autonomous-ai/lampOS/issues/754), the
  second voice (moved from the third on 2026-10-15 at its person's request).

## It may

- Change the crates above, and the documents that belong to them, in the
  same commit.
- Propose work as `sixth-P<n>` in its log and as a comment on the issue it
  belongs to, or on the Queue issue (#705) when none fits, as joining.md's
  `fifth-P8` says; it opens no issues and makes no labels.
- Add and remove `status:` labels on, and comment on, its own issues.

## It may not

- Use a lamp outside an agreed loan, or treat shared access as ownership.
  `lamp-4ace` remains the first builder's; Sixth does not install there.
  Tests on lamp-52e6 follow the scope and restoration rules above.
- Touch another builder's branch or crates unless a task says so; push to
  `lamp-voice`.
- Commit Python, credentials, or pictures of a person, face or screen.
- Make any other issue or label, or a spending account, without the
  owner's yes.
