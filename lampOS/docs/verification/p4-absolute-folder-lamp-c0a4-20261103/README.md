# P4 `lamp-tasks serve --absolute-folder` on lamp-c0a4, 3 Nov 2026

## Build

- Commit `4d56524950` (branch `subdarren/p4-abs-folder`).
- `lamp-tasks` only, local Docker ARM64 build (not a CI artifact),
  sha256 `fca060727908b35c6f792f24eca15abab3773df154dda4eb8d851796527360c6`.
- Installed as a test drop-in: the binary at
  `/opt/lampOS/tasks/test-subdarren/lamp-tasks`, with the product unit's own
  `serve` arguments unchanged (`--enable --watch --progress --push-wait
  --close-told --harness-as-person --dir … --socket … --voice-socket …
  --targets … --harness /usr/local/bin/harness`), first without and then with
  `--absolute-folder`. Running binary checked after each install: sha256
  `fca06072…360c6`, `active`, NRestarts=0.

## How it was run

- The voice provider was in an outage, so no spoken asks. Each hand-over was
  sent the way lamp-voice sends one: as the lamp's tasks user, `prepare` on
  `request.sock`, then `confirm` on `voice.sock`. Text: "Reply with the
  single word done. Do not change any files."
- Target: the lamp's own `office` target (the paired Mac, engine `claude`,
  folder `~/harnesses/lamp-tasks`, already there and trusted). `targets.json`
  not changed.
- Three hand-overs with the switch off, then a restart with it on and three
  more, about 45 s apart. Every one was answered "done" on the Mac in
  `~/harnesses/lamp-tasks`.

## Results

`new` = the journal's `new answered in MS ms`; dispatch = `dispatch accepted
(exit 0, S s)`, from the start of the hand-over to Harness to its acceptance.

| Switch | # | Time | `new` ms | dispatch s | Folder sent |
|--------|---|------|----------|------------|-------------|
| off | 1 | 18:15:33 | 4990 | 7.2 | home-relative (`~/`) |
| off | 2 | 18:16:20 | 5353 | 7.9 | home-relative (`~/`) |
| off | 3 | 18:17:03 | 4792 | 7.2 | home-relative (`~/`) |
| on | 1 | 18:20:54 | 5338 | 7.5 | home-relative (`~/`): learning the home |
| on | 2 | 18:21:38 | 4204 | 6.5 | absolute, from the computer's home learned earlier |
| on | 3 | 18:22:20 | 3753 | 6.0 | absolute, from the computer's home learned earlier |

| Side | `new` median (range) | dispatch median (range) |
|------|----------------------|-------------------------|
| off, all 3 (`~/`) | 4990 ms (4792–5353) | 7.2 s (7.2–7.9) |
| on, all 3 | 4204 ms (3753–5338) | 6.5 s (6.0–7.5) |
| on, the 2 sent absolute | 3979 ms mean (3753–4204) | 6.25 s mean (6.0–6.5) |
| every `~/` send (4) | 5164 ms (4792–5353) | 7.35 s (7.2–7.9) |

The two absolute sends were about 0.6–1.6 s faster in `new` than every
`~/` send; the first send after the switch went as `~/` and learned the home,
as the doc says. Three per side over the Harness relay: a small sample, and
the spread of the `~/` sends alone is about 0.6 s.

Full filtered lines: `journal.txt`.

## Harnesses closed

All six hand-overs' harnesses were closed with the lamp's own Harness CLI as
the tasks user (`harness close --machine <mac-machine-id> NAME --json`): each
`"closed":true`. The Mac's `harness ps` list afterwards was the same as
before the test. None left open.

## Restore

- Drop-in and `/opt/lampOS/tasks/test-subdarren` removed, `daemon-reload`,
  restart.
- After: running binary sha256
  `4a780296069e366e4a9d0c1a194a7d97c6b8f690c26578ef08ef5d1cb9efbdc3`
  (same as before), `active`, NRestarts=0, drop-ins: `exit-note.conf` only,
  `targets.json` sha256 `43ddde40…76390` (unchanged).

## Not verified

- That Harness made one fewer `fs_list_dir` round trip: shown only as time
  and by the journal's KIND; Harness's own trace was not read.
- A failing `new` in a learned home forgetting it. (A first hand-over after
  a restart going as `~/` was shown: flag-on #1.)
- Spoken hand-overs through lamp-voice (provider outage).
