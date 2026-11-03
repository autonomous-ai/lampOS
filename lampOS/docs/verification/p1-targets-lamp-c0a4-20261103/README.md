# P1 on lamp-c0a4: the tasks unit with no targets file (3 Nov 2026)

## What was tested

The product `lamp-tasks.service` passes `--targets /var/lib/lampos-tasks/targets.json`, and nothing writes that file. A lamp that turns Harness on without it crash-looped (`tasks: No such file or directory (os error 2)`, five restarts, then `Start request repeated too quickly`; seen on this lamp the same morning). This branch adds `--set-up-later --targets-from-machines claude` to the unit's ExecStart.

## How it was taken

- **Binary:** the product `lamp-tasks` already on the lamp (`/opt/lampOS/tasks/current` → `4a780296…`). The change is only the unit's flags; the binary is unchanged.
- **Before:** the unit was active with NRestarts=0. `targets.json` was the one written by hand that morning (lampos, 0600, 138 bytes, SHA-256 `43ddde40…6390`). The only drop-in was `exit-note.conf`. Harness was signed in as `lampos`.
- **The check:**
  1. At 16:49:50 +07, `targets.json` was moved aside.
  2. One drop-in, `zz-<test>.conf`, held an empty `ExecStart=` and then this branch's ExecStart line exactly, from `deploy/product/lamp-tasks.service`.
  3. `daemon-reload`, then a restart at 16:49:51.
  4. 30 s later, the unit state, the process's command line and the journal (`journal.txt`) were read.
- **Restore:**
  1. At 16:50:38 the drop-in was removed and the hand-written `targets.json` was moved back over the one written by the part.
  2. `daemon-reload` and a restart.
  3. Checked: SHA-256 `43ddde40…6390` again, lampos 0600; the product ExecStart without the two flags; active, NRestarts=0; only `exit-note.conf`.

## Result

| What | Seen |
|---|---|
| Start with no targets file | Active, NRestarts=0, `tasks: no task targets yet; hand-overs are off until they are written` |
| Process command line | The product binary with `--set-up-later --targets-from-machines claude` |
| Targets from the machines | `tasks: Harness signed in: yes`, then `tasks: 1 task targets written from the machines` and `the targets file changed: 1 task targets`, within 0.4 s of the start |
| The file written | lampos, 0600, 138 bytes: one target `office`, engine `claude`, the person's computer's machine id, project `~/harnesses/lamp-tasks` (the same shape as the hand-written one) |

## Not shown

- A lamp that is not signed in to Harness at start (the part waits for the sign-in; the unit tests and #2841 cover it).
- A fresh lamp after `lamp-update harness-on` from a build that carries this unit. The unit was applied here with a drop-in.

First can see it on lamp-4ace once this unit ships: `systemctl show lamp-tasks -p NRestarts` stays 0, and `journalctl -u lamp-tasks | grep "task targets"` shows the targets written.

`journal.txt` is the lamp-tasks journal from the restart (16:49:51) to the restore, with times cut to the microseconds within the second. The computer's machine id is written `<machine>`.
