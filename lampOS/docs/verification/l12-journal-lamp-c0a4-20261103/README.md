# L12 journal: one line per Harness call, on `<lamp>` (2026-11-03)

PR #5092: lamp-tasks writes one `tasks: harness VERB: …` journal line per
Harness CLI call, with the verb, 8-character ids, exit, ms, stdout bytes and
the parsed state, and never the words.

## Build

- `76ddad7ab3` (branch `subdarren/l12-journal`), local Docker ARM64 build,
  sha256 `1405469faa925008e72fcac172afe0f17b4e19e0baf1fdbd157c3f36462d8b5f`.
- Installed as `/opt/lampOS/tasks/test-subdarren/lamp-tasks` with a temporary
  drop-in carrying the product unit's own ExecStart arguments, no extra flag:
  `serve --enable --watch --progress --push-wait --close-told --harness-as-person --dir … --socket … --voice-socket … --targets … --harness /usr/local/bin/harness`.
- Before: `active`, NRestarts=0, running binary sha256 `4a780296…efbdc3`,
  targets.json sha256 `43ddde40…576390`, drop-ins: `exit-note.conf` only.
- Cloud on, volume untouched.

## What was run (times +07)

1. 18:36:56 install, then about 2 minutes idle with nothing handed over.
2. 18:38:52 one hand-over to the existing `office` target: `prepare` on
   `request.sock`, then `confirm` on `voice.sock` (no voice), ask "Reply with
   the single word done. Do not change any files." Task 28, confirm ok in 40 ms.
3. The agent came back idle at 18:39:30 (shown 18:39:34); closed at 18:39:52
   with the lamp's own `harness close` as `lampos` (exit 0, `closed: true`).
4. Journal collected from install to restore: `journal.txt`.
5. Restored at about 18:40.

## Verbs seen

| Verb | Fields present | Sample line |
|------|----------------|-------------|
| machines | exit, ms, out, listed | `tasks: harness machines: exit=0 ms=2102 out=903 listed=2` |
| new | machine, exit, ms, out, state, id | `tasks: harness new: machine=5d53993e exit=0 ms=5593 out=1445 state=ok id=28a48734` |
| ps | machine, exit, ms, out, listed, states | `tasks: harness ps: machine=5d53993e exit=0 ms=4023 out=5974 listed=11 states=idle:6,unknown:5` |
| wait | machine, agents, exit, ms, out, listed, states, ids | `tasks: harness wait: machine=5d53993e agents=1 exit=0 ms=16983 out=325 listed=1 states=idle:1 ids=28a48734` |
| show | machine, agent, exit, ms, out, state, tools, id | `tasks: harness show: machine=5d53993e agent=28a48734 exit=0 ms=3486 out=408 state=idle tools=0 id=28a48734` |

Five Harness calls, five lines, each with an `exit=`, `ms=` and `out=`.
The dispatch line (`tasks: lamp-2a33…: dispatch accepted (exit 0, 7.7 s)`)
comes right after `new`; its 7.7 s is `machines` (2.1 s) plus `new` (5.6 s).

No `close` line: the close was the lamp's CLI run by hand, not by lamp-tasks.
No `ps` line during the 2 idle minutes: with no task handed over, the watch
makes no Harness calls; the first `ps` came 4 s after the hand-over.

## Privacy checks (on the five `tasks: harness` lines)

| Check | Hits |
|-------|------|
| `Reply`, `single word`, `Do not`, `=done` | 0 |
| folder paths (`/Users`, `/var/`, `harnesses/`) | 0 |
| machine name or person name | 0 |
| full harness name `lamp-2a33…` | 0 |
| full machine id, full agent UUID | 0 |
| id lengths: `machine=` x4, `agent=` x1, `id=` x2, `ids=` x1 | all 8 characters |

No cloud ids came up in this run, so the `cloud_` + 8 form was not seen.
The older `progress read of …` and `wait on …` lines, not part of this PR,
still print the full machine id (shown as `<mac-machine-id>` in
`journal.txt`).
The existing `dispatch accepted` line (`journal.txt` line 5), also outside
this PR, names the task's harness in full (`lamp-` and 32 hex digits). That
name is lamp-tasks' own, made from the task, with no words of the ask in it.
The checks above cover only the five new lines (the reviewer-lead's P3 on
#5092).

## Restore

- Drop-in and `/opt/lampOS/tasks/test-subdarren` removed, daemon-reload, restart.
- After: `active`, NRestarts=0, running binary sha256
  `4a780296069e366e4a9d0c1a194a7d97c6b8f690c26578ef08ef5d1cb9efbdc3`,
  targets.json sha256
  `43ddde40d7374913de8b21675795ab781e3294090fba40150715e79e0d576390`
  (unchanged, `lampos:lampos 600`), drop-ins: `exit-note.conf` only.

## Not verified

- Verbs not called in this run: `reply`, `route`, `close` (by lamp-tasks),
  `start`, `engines`, `auth-status`; their lines are covered by the unit
  tests in `crates/tasks/tests/harness.rs` only.
- Failure forms (`exit=timeout`, `exit=killed`, a non-zero exit) on the lamp.
- Cloud ids (`cloud_` + 8).
- Voice: the hand-over was confirmed over the socket, no speech.
