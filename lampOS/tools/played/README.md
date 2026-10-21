# The played test

A Mac beside the lamp speaks scripted lines from its own speakers, in macOS
`say` voices (Daniel, Samantha, Karen), and then reads the lamp's voice
session over ssh to print what the lamp heard and said, and when. A played
voice is a smoke test: the lamp hears replayed speech worse than a person.

`battery.sh LABEL` runs the whole played test, about 17 minutes:

1. **Two people** (`twopeople829.sh`, one round, #829): four scenes of two
   voices talking at the lamp. One asks while the other talks to them; the
   lamp is named in passing; one asks and the other follows up; one
   interrupts the lamp's answer.
2. **The day** (`day.sh`): twelve lines of a person's day, from "good
   morning" through the light, a look, a fact to remember, work handed to
   Claude Code, a reminder and a timer, to "see you later".
3. **Follow-ups** (`followups.sh`, #1465): named asks, each followed by an
   unnamed one that needs a tool; counts from the voice's journal the tool
   calls run and refused.
4. **Hand-overs** (`handovers.sh`, #854, #903): stories asked of the lamp
   must be told; work asked of Claude Code or Codex must be offered, then is
   declined with "No, not now." so nothing starts.

The day and the follow-ups first wait until the lamp has heard and said
nothing for 45 seconds (`quiet_gate.sh`).

Four more you can run on their own:

- `open-name.sh LABEL`, about 3 minutes (#2355): "Hey Lamp, my name is",
  a pause, then a made-up name; does the lamp keep it? It then asks the
  lamp to forget the three names.
- `twopeople829.sh ROUNDS LABEL`, about 4 minutes a round: the two-person
  scenes alone.
- `ask-again.sh LABEL FIXTURE`, about 3 minutes (#2702): a sign-in's
  question cut short is asked again 30 s later, then heard whole, and the
  yes after it is taken. It uses a stand-in Harness, so nothing signs in for
  real: FIXTURE is an ARM64 `lamp-tasks-harness-fixture` from a lampOS
  ARM64 run's artifact. It puts it in as lamp-tasks' Harness, with
  `--sign-in-by-code` and the voice's `LAMP_TASKS_SIGN_IN=on`, through
  `/run` drop-ins that it removes when it ends. It prints PASS or FAIL for
  each step, and hand-overs fail while it runs. A lamp whose own
  lamp-tasks is older than #2702 (no `--sign-in-by-code` in the binary) is told so, and
  nothing is played. Give `TASKS=` an ARM64 `lamp-tasks` from the same
  artifact, and it's used for the run only. The lamp side is
  `ask-again-setup.sh`, run as root, with the stand-in's HOME given to
  lamp-tasks' own user. If a setup step fails, it says which, and nothing
  is played. `ask-again-test.sh` runs that setup against a stand-in
  `systemctl`, with no lamp and no root: run it after changing the setup.
- `addon-say.sh LABEL ["the kept text"]`, about 2 minutes (#2675): an
  add-on's quoted say on the second route. A `/run` drop-in turns
  `LAMP_ADDON_SAY` on for the run and is removed on exit; the lamp says one
  quoted line, and the script counts the kept words it said in order, those
  it changed or added, and any refusal (`$PLAYED_OUT/scenarios/addon-say-LABEL.txt`).
  Its stand-in test is `addon-say-test.sh`.

## What it needs

- A Mac beside the lamp, its speaker where the lamp can hear it.
- `say` and `afplay` (part of macOS), `python3`, `zsh`.
- ssh to the lamp with a key, no password: the kit runs ssh with
  BatchMode, so ssh to the lamp once by hand first to accept its host key.
  The lamp's user needs `sudo -n` (the kit reads the voice's sessions under
  `/var/lib/lampOS` and puts the speaker's volume back).
- The lamp's voice running.

## Settings

| Setting | | |
|---|---|---|
| `LAMP_HOST` | required | Your lamp's address or name. There is no default: without it the kit refuses and plays nothing. |
| `LAMP_SSH_KEY` | optional | A private key file for the lamp, used alone. Otherwise ssh's own keys and `~/.ssh/config`. |
| `LAMP_USER` | `orangepi` | The lamp's user. |
| `LAMP_KNOWN_HOSTS` | optional | A known_hosts file the lamp's key must be in. Otherwise your own known_hosts decides. |
| `PLAYED_OUT` | `~/lamp-played` | Where results and the spoken lines go. |
| `LAMP_SAY_RATE` | `185` | `say`'s speaking rate. |

All ssh goes through `lamp_ssh`, which you can also use yourself:
`lamp_ssh 'systemctl is-active lamp-voice' </dev/null`.

## Running it

```sh
export LAMP_HOST=<your lamp's address> LAMP_SSH_KEY=<your key file>
lampOS/tools/played/battery.sh b1034
lampOS/tools/played/open-name.sh n1034
lampOS/tools/played/twopeople829.sh 1 t1034
```

LABEL names the run's files: make it new each time, for example the time.

## Results

- `$PLAYED_OUT/scenarios/battery-LABEL.txt`: the summary. Each scene line
  and what the lamp said, "day lines answered N of 12", the follow-ups'
  counts and the hand-overs' tally. The tallies are a guide: read the lines.
- `$PLAYED_OUT/scenarios/<time>-<name>.txt`: each scene in full, named by
  the UTC time it ended.
- `$PLAYED_OUT/prompts/generated/`: the spoken lines, made once by `say`
  and reused (`index.tsv` lists them).

## It plays sound in the room

Run it only when nobody is using the lamp or working near it. On weekdays
not between 12:15 and 13:45, when the office rests (`scenario.py` itself
refuses 12:00 to 14:00 on weekdays unless `LAMP_PLAY_AT_LUNCH=yes`).

The lines change the lamp as a person's would: its light gets warmer,
dimmer and brighter and is left as the lines leave it; it remembers a
favourite colour; it sets a reminder and a timer; and the day hands a
short story to Claude Code. The follow-ups put the speaker's software volume
back as they found it.

If a run seems stuck at the day, check that ssh to the lamp works: an
unreachable lamp counts as not quiet, and the day waits for quiet.
