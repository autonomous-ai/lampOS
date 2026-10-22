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

These you can run on their own:

- `open-name.sh LABEL`, about 3 minutes (#2355): "Hey Lamp, my name is",
  a pause, then a made-up name; does the lamp keep it? It then asks the
  lamp to forget the three names.
- `twopeople829.sh ROUNDS LABEL`, about 4 minutes a round: the two-person
  scenes alone.
- `quietroom.sh TRIALS LABEL`, about 1.5 minutes a trial (#2650/#2663,
  `LAMP_SAME_VOICE_CALLER_ONLY`): a caller's named question, then one
  second voice's statement not for the lamp, inside the conversation's
  window; trials 70 s apart, so the room stays quiet (the two-person scenes
  make it busy by themselves). It prints whether each statement was
  answered, and the count. Run it with the switch off and on in the same
  quiet hour.
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
  `stand-in.sh`, run as root, giving lamp-tasks an empty store of its own for the run (the real one is never touched), with the stand-in's HOME given to
  lamp-tasks' own user. If a setup step fails, it says which, and nothing
  is played. `stand-in-test.sh` runs that setup against a stand-in
  `systemctl`, with no lamp and no root: run it after changing the setup.
- `addon-say.sh LABEL ["the kept text"]`, about 2 minutes (#2675): an
  add-on's quoted say on the second route. A `/run` drop-in turns
  `LAMP_ADDON_SAY` on for the run and is removed on exit; the lamp says one
  quoted line, and the script counts the kept words it said in order, those
  it changed or added, and any refusal (`$PLAYED_OUT/scenarios/addon-say-LABEL.txt`).
  Its stand-in test is `addon-say-test.sh`.
- `make-skill.sh LABEL FIXTURE [yes|no|both]`, about 4 minutes an arm
  (#2424, "An owner makes their own skill"), with a stand-in Harness as
  `calendar.sh` has, so nothing real starts: a voice asks the lamp for a
  small skill and says yes to the hand-over; the stand-in answers with a
  fixed skill, and once the lamp asks "Shall I put it on the lamp?", the
  voice answers "Yes, put it on." (made, asked, that SHA-256 installed only
  after the yes, enabled, loaded, removed) or "No, not now." (nothing
  installed). `LAMP_MAKE_SKILL=on` rides on the stand-in's `/run` drop-ins,
  removed on exit with any skill it put on. Its stand-in test is
  `make-skill-test.sh`.

- `cut-in-keeps.sh LABEL [ROUNDS]`, about 15 minutes for 3 rounds each way
  (#2739, `LAMP_CUT_IN_KEEPS_LISTENING`): "Hey Lamp, tell me a long story
  about a lighthouse keeper.", two asides that pause the story and let it
  go on ("Hmm, interesting.", "What was that?"), then "Lamp, stop. Please
  stop the story." about 5 s after the second. A `/run` drop-in sets the
  switch off, then on, the voice restarted each time, and is removed on
  exit. Per round it prints whether the third line was heard, whether the
  story stopped (`user_interrupted`) and the gap from the line's start to
  the stop, from the voice's session events; a round whose asides were not
  two yields that resumed is not a trial. On passes when the line is heard
  and the story stops within about 1 s (`$PLAYED_OUT/scenarios/cut-in-keeps-LABEL.txt`).
  Its stand-in test is `cut-in-keeps-test.sh`.

- `cutin.sh LABEL [IDLE_STORIES] [ROUNDS]`, about 25 minutes with the
  defaults (3 and 1; #2602 "Cutting in", the measurement before any bound
  changes): long stories with nobody talking, counting false yields per
  minute of reply heard; then "Hmm, interesting." (an aside) and "Wait,
  what was the keeper's name?" (a cut-in), each 3, 8, 15 and 25 s into a
  fresh story. Per episode it prints onset to yield, yield to commit or
  resume, and for a commit end of words to the answer's first audio,
  accepted PCM and the microphone's proxy for acoustic output apart, with
  the build and flags, the per-block hearing from the yield's onset run to
  its decision, stop-word results and discontinuities with their sequence
  numbers (`$PLAYED_OUT/scenarios/cutin-LABEL.txt`). It changes nothing on
  the lamp unless `CUTIN_ENV` names switches for the run, which ride on a
  `/run` drop-in removed on exit. A yield's decision is only the one its
  own `duplex_yield_end` names, a cut-in's answer only the first write of
  the turn whose captured audio began within the line's words
  (unmeasured if the line was not submitted, matched no or several turns,
  or its turn had no audio), and the
  acoustic proxy only that answer's own blocks. It exits non-zero when a scene, journal read or report
  failed, or the drop-in could not be taken off. Its stand-in test is
  `cutin-test.sh`.

- `calendar.sh LABEL FIXTURE`, about 4 minutes (#751, "Email, calendar
  and other connectors, through hand-overs"): "Hey Lamp, what's on my
  calendar today?" is handed over to the computer's agent, which is the
  same stand-in Harness (`stand-in.sh`) answering with two made-up events.
  The lamp tells them back. The run gets its own targets file, one alias
  (the voice's own) on the stand-in's machine, and the voice gets
  `LAMP_TASKS_ACCOUNTS=on`. It prints PASS or FAIL for each step: asked to
  confirm (the lamp's own question first, answered once its answer has
  played; it also passes when the lamp offers at once), offered,
  handed over (the stand-in's `new`), the answer read (its `show`), told,
  and heard whole (not cut short, with both events in the lamp's words).
  `TASKS=` works as for `ask-again.sh`. Nothing reaches a real calendar.
- `email.sh LABEL FIXTURE`, the same for email: "Hey Lamp, did I get any
  email from the office today?", with the stand-in answering with two
  made-up messages. Both scripts set their ask, the stand-in's answer and
  two words of it, and run `account-ask.sh`.

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
