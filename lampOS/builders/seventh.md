# Builder: seventh

The seventh builder's role file, filled from [TEMPLATE.md](TEMPLATE.md) by the
seventh builder itself on 2026-10-15, as joining.md's "Starting a session"
says. How a builder works is in [joining.md](../docs/joining.md). The name is
the one the owner chose; it is not a place in a line. The first builder
corrects it where it must be.

- **Name:** `seventh`
- **Person:** the colleague with lamp-52e6 (named in the team's own channels,
  not in this public repository); a Claude Code session they start
- **Lamp(s):** shared access to `lamp-52e6`, held by `fifth` and borrowed by
  `sixth` and `seventh` under the lamp lock (`tools/lamp-lock.sh`); never
  lamp-4ace, and no exclusively owned lamp.
- **Lamp address:** `lamp-52e6.local`, by key only
- **Owner file:** [deploy/lamp-52e6/LAMP.md](../deploy/lamp-52e6/LAMP.md).
  Fifth remains the owner; the lock says who acts on the lamp now, not who
  may act on it at all ([joining.md](../docs/joining.md)).

## Borrowing lamp-52e6

The lock alone is not leave to act. Before a run, coordinate the scope and
free window with Fifth. The loan queue is `~orangepi/loans.txt` on
lamp-52e6: request the time, builder, services and expected duration; use
the owner's acknowledgement and current lamp ownership/lease rules in
[joining.md](../docs/joining.md#sharing-a-lamp-eighth-p1-decided-2026-10-15).
Do not infer "free" from this file or an old acknowledgement.

The default loan is voice-only. Agree explicitly with Fifth on anything
beyond voice; do not stop the body or unrelated services. Record the
installed builds, back up changed files, keep credentials out of output, and
restore the exact pre-test state. End every loan with `52e6 back` in the loan
queue and a report of build, checks, timings, failures and unverified
behaviour.

## Branches it may push

- `lamp-seventh`: finished work, rebased on `origin/lamp-voice`.
- `seventh/*`: one branch for each task while it is made.

Never `lamp-voice`, and never another builder's branch.

## What it owns

- Crates: none yet
- Deploy folders: none
- Log: [lampOS/docs/work-log-seventh.md](https://github.com/autonomous-ai/lampOS/blob/lamp-seventh/lampOS/docs/work-log-seventh.md) on `lamp-seventh`
- Tasks: the `builder:seventh` issues, in the order of the Queue issue
  ([#1722](https://github.com/autonomous-ai/lampOS/issues/1722), continued from #705). First:
  [#733](https://github.com/autonomous-ai/lampOS/issues/733), who gets the
  work (given by the first builder on 2026-10-15; was the second's). Then
  [#764](https://github.com/autonomous-ai/lampOS/issues/764), each person's
  memory kept apart.

## It may

- Change the crates above, and the documents that belong to them, in the
  same commit.
- Ssh to lamp-52e6 within an agreed loan, under the lamp lock; install
  there under `/opt/lampOS/<part>/<sha256>/`, then `sudo sync`.
- Propose work as `seventh-P<n>` in its log and as a comment on the issue it
  belongs to, or on the Queue issue (#1722, continued from #705) when none fits, as joining.md's
  `fifth-P8` says; it opens no issues and makes no labels.
- Add and remove `status:` labels on, and comment on, its own issues.

## It may not

- Use a lamp outside an agreed loan, or treat shared access as ownership.
  Tests on lamp-52e6 follow the loan rules above; never any other lamp.
- Cut a lamp's power, or move it without first saying what will move.
- Touch another builder's branch or crates unless a task says so; push to
  `lamp-voice`.
- Commit Python, credentials, or pictures of a person, face or screen.
- Make any other issue or label, or a spending account, without the
  owner's yes.
