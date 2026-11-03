# P3 `lamp-tasks serve --new-folder` on lamp-c0a4, 3 Nov 2026

## Build

- Commit `afb138a4a3` (branch `subdarren/p3-new-folder`).
- `lamp-tasks` only, local Docker ARM64 build (not a CI artifact),
  sha256 `1023863a56e754c050d4b8599b4f0aeca56b6e1d36183fee1186fe7d73e5d363`.
- Installed as a test drop-in: the binary at
  `/opt/lampOS/tasks/test-subdarren/lamp-tasks`, with the product unit's own
  `serve` arguments unchanged plus `--new-folder`:
  `--enable --watch --progress --push-wait --close-told --harness-as-person
  --dir /var/lib/lampos-tasks/store --socket … --voice-socket … --targets …
  --harness /usr/local/bin/harness --new-folder`.
- Running binary after install: sha256 `1023863a…5d363`, `active`, NRestarts=0.

## How it was run

- The voice provider was in an outage, so no spoken asks. Each hand-over was
  sent the way lamp-voice sends one: as the lamp's tasks user, `prepare`
  (guard, text, target) on `request.sock`, then `confirm` (guard, id,
  `prepared_digest`, `session_id`) on `voice.sock`. Text: "Reply with the
  single word done. Do not change any files."
- A test-only target `check` was added to `targets.json`: the paired Mac,
  engine `claude`, folder `~/harnesses/lamp-p3-check`. `targets.json` was put
  back byte-for-byte afterwards.
- Before: `ls -d ~/harnesses/lamp-p3-check` on the Mac: No such file or
  directory; no `folders-seen.json` in the tasks folder.

## Results

| # | Time | What | Journal | Mac | Result |
|---|------|------|---------|-----|--------|
| 1 | 18:07:01 | first hand-over, folder absent | `the folder for 'lamp-p3-check' is new to this lamp on that computer, so Harness is asked to make it`, then `dispatch accepted (exit 0, 9.3 s)` | folder `~/harnesses/lamp-p3-check` made 18:07; `harness ps`: idle, answer "done"; `harness show`: asked + answer "done", no trust question; `~/.claude.json` has this folder with `hasTrustDialogAccepted: true` | pass |
| — | 18:07 | `folders-seen.json` | — | — | `{"version":1,"seen":[{"machine_id":"<mac-machine-id>","project":"~/harnesses/lamp-p3-check"}]}`, mode 0600 | pass |
| 2 | 18:08:25 | second hand-over, same target | only `dispatch accepted (exit 0, 9.3 s)`: no "new to this lamp" line, so the plain folder | idle, "done", same folder | pass |
| 3a | 18:09:26 | `folders-seen.json` removed while the part ran, then a hand-over | only `dispatch accepted (exit 0, 8.5 s)`; the file was not written again | idle, "done" | see note 1 |
| 3b | 18:10:51 | `folders-seen.json` still absent, `systemctl restart lamp-tasks`, then a hand-over | `… is new to this lamp …, so Harness is asked to make it`, 6.8 s later `the folder for 'lamp-p3-check' is on that computer already, so it starts there`, then `dispatch accepted (exit 0, 13.7 s)` | idle, "done", same folder | pass |
| — | 18:11 | `folders-seen.json` after 3b | — | — | written again with the same one entry | pass |

Note 1: the marks are read from the file once, when the part starts, and kept
in memory; removing the file under a running part changes nothing until a
restart. The doc says the file is there "so a restart doesn't ask `--new`
again", which matches; the step had to be done with a restart (3b).

Note 2: the journal does not print the `harness new` arguments, so `--new
lamp-p3-check` (1, 3b) and the plain folder (2, 3a, the 3b retry) are shown
by the journal lines the code writes for each path, the folder appearing on
the Mac at the first hand-over, and the "already exists" path in 3b.

Full filtered lines: `journal.txt` (machine id written as `<mac-machine-id>`).

## Harnesses closed

All four hand-overs' harnesses (`lamp-253a…`, `lamp-77cf…`, `lamp-edf8…`,
`lamp-5d77…`) were closed with the lamp's own Harness CLI as the tasks user,
`harness close --machine <mac-machine-id> NAME --json`: each `"closed":true`.
None left open. `~/harnesses/lamp-p3-check` (empty) was removed from the Mac.

## Restore

- Drop-in and `/opt/lampOS/tasks/test-subdarren` removed, `daemon-reload`,
  restart; `folders-seen.json` removed; `targets.json` copied back.
- After: running binary sha256
  `4a780296069e366e4a9d0c1a194a7d97c6b8f690c26578ef08ef5d1cb9efbdc3`
  (same as before), `active`, NRestarts=0, drop-ins: `exit-note.conf` only,
  `targets.json` sha256 `43ddde40…76390` (same as before), owner and mode as
  before.

## Not verified

- The exact `harness new` argv (`--new lamp-p3-check`) as sent: not logged;
  shown only through the code's own journal lines and the Mac side.
- A folder made by hand and never trusted (the doc says it still asks).
- Spoken hand-overs through lamp-voice (provider outage): the socket requests
  were sent directly.
