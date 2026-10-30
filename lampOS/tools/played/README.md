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
- `code-once.sh LABEL FIXTURE`, about 2 minutes (#2839): a spoken
  sign-in's code is told once, and once more only when asked for again. It
  puts the stand-in Harness in as `ask-again.sh` does, with its code held
  (`code_hold_until`) until the script lets the question come. It counts
  lamp-tasks' journal lines `code handed to a start` and `code claimed`,
  which carry no digits: one after the first ask, still one 20 s later, two
  after "say the code again", still two 20 s later. Then it releases the
  question and answers no, so nothing signs in. FIXTURE and `TASKS=` are as
  for `ask-again.sh`, from a build with #4112's journal lines.
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
  lamp-tasks' own user. It runs on a lamp with no Harness CLI too
  (lamp-06c1): the product unit's `ConditionPathExists=/usr/local/bin/harness`
  would have systemd skip lamp-tasks, so nothing could be handed over, and the
  stand-in's drop-in clears it, needing its own wrapper instead; the put-back
  leaves lamp-tasks skipped again. If a setup step fails, it says which, and nothing
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
  voice answers "Yes, go ahead." (made, asked, that SHA-256 installed only
  after the yes, enabled, loaded, removed) or "No, not now." (nothing
  installed). `LAMP_MAKE_SKILL=on` rides on the stand-in's `/run` drop-ins,
  removed on exit with any skill it put on; a product unit already sets it
  on (fourteen-P15), and the run takes it as it is. The voice's made skills and told
  news are saved in `/usr/local/lib/played-make-skill-saved` (made as
  `….part` and renamed once whole) and put back on exit with lamp-voice
  stopped (nothing put back unless the stop worked and lamp-voice then
  reads inactive or failed, and started again straight after either way;
  the guard exits nonzero while the copy is still there), checked folder by folder and file by file
  (SHA-256); a put-back that fails exits 8 and keeps that copy
  (`sudo sh /usr/local/lib/played-make-skill-saved/restore` puts it back), the
  lamp's guard runs the same put-back first, and a copy left over, a save cut
  short (`sudo rm -rf /usr/local/lib/played-make-skill-saved.part` clears it;
  nothing was changed before it), or either path as anything but a regular
  file or a folder of them (a link, dangling or not), stops the next run
  before any change. Its stand-in test is `make-skill-test.sh`.

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

- `full-look-row.sh LABEL`, about 35 minutes (#2998, the product's full look): on a lamp on the
  product units with #2998, the eyes' cost of `--sight-from-look` off and on (a `/run` drop-in runs
  the unit's own line with `--look-on-change on` for the 10 s line's stage times, 4 minutes each,
  removed on exit and checked removed), then ten "Hey Lamp, what do you see?" asks as installed,
  each a scene of its own. It prints the medians of "ms a look", "sight file" and "decode", and per
  ask the first and second times the lamp starts speaking after the end of speech was decided (for
  a look, normally "Let me look." and its answer), with what the lamp said. Before each change to
  the eyes and each ask it waits for 45 s of quiet (`quiet_gate.sh`, up to `QUIET_MAX`, 900 s).
  Each look sends a still of the desk to the model, so it plays no ask while the eyes see a face
  (`/run/lampos-eyes/present`, a face on a screen too) and stops there. Its stand-in test is
  `full-look-row-test.sh` (zsh needed).

## The desk kit

`desk-kit.sh LABEL` compares sets of the voice's switches ("arms") on a
person at a desk who asks the lamp things hesitantly, between colleagues'
side talk. About 7 minutes a scene, TRIALS scenes an arm (default 2).

```sh
export LAMP_HOST=<your lamp's address>
# On a shared lamp (<lamp> = lamp-52e6, lamp-75c9), under its lease; the kit takes none itself:
ARMS="off:LAMP_X=off on:LAMP_X=on,LAMP_Y=500" BASE="LAMP_SPOKEN_TO=on" \
  lampOS/tools/lamp-lock.sh <lamp> run <holder> <session> "desk kit d1500" -- \
  lampOS/tools/played/desk-kit.sh d1500
ARMS="off:LAMP_X=off" lampOS/tools/played/desk-kit.sh --dry [SCENARIO ...]
```

- **Where the Mac is.** Put the Mac about 1 m from the lamp, where a
  person would sit, and leave its volume at 50 (the kit sets 50 and puts
  50 back). Never louder and closer: the lamp would hear a voice no person
  makes.
- **The asks** are `hesitant.tsv`: 12 asks to the lamp (mark `L`), each in
  2 or 3 variants with pauses of 0.2 to 0.9 s in different places, "uh"
  and "um", a restart ("what's a… a good name") or trailing off, and 3
  un-named follow-ups ("And a soft one?") said about 7 s after their ask,
  inside the lamp's answer's 8 s. Scene N plays each ask's variant N.
  Every line is one wav made before the scene starts, its pauses exact
  silences inside it: two `scenario.py` steps are 3 to 10 s apart, since
  each step makes its speech and reads the lamp's clock over ssh.
- **The side talk** (mark `C`) after each ask: a clip of a real meeting
  from the AMI Meeting Corpus, CC BY 4.0,
  https://groups.inf.ed.ac.uk/ami/corpus/, listed in
  `$DESK_KIT_AMI/clips.tsv` (columns `file meeting start_s end_s speakers
  overlap text`, `file` relative to that folder). Without that file, two
  `say` voices talk about work. AMI audio stays on your Mac, outside the
  repo: never commit it.
- **The kit's audio** is one versioned tar, `desk-kit-1.tar`, fetched once
  into `$PLAYED_OUT` from https://cdn.autonomous.ai/os/test-kits/desk-kit-1.tar
  (`gs://s3-autonomous-upgrade-3/os/test-kits/`) and refused
  unless its sha256 is `DESK_KIT_SHA256` (`e9f826f6…`), so every Mac plays
  the same audio. It holds 52 AMI clips (meetings EN2002a, EN2002b,
  EN2002d and ES2004b, individual headsets, from the corpus owners' copy at
  https://huggingface.co/datasets/edinburghcstr/ami, whose card gives the
  licence as `cc-by-4.0`), cut, summed for overlap, resampled to 48 kHz and
  levelled, with the credit "AMI Meeting Corpus, the AMI Consortium,
  https://groups.inf.ed.ac.uk/ami/corpus/, CC BY 4.0,
  https://creativecommons.org/licenses/by/4.0/" in its README. It also
  holds the asks, pre-made with macOS `say` (voice `Daniel` asks, `Samantha`
  the colleague with no clips, rate 185, 16-bit 16 kHz), so they are the
  same on every Mac. `DESK_KIT_TAR=none` uses `DESK_KIT_AMI` and this
  Mac's `say` instead.
- **Each arm** is `NAME:ENV=v,ENV=v`, put on lamp-voice with `BASE` through
  a `/run` drop-in (`zz-desk-kit.conf`), the voice restarted and its
  environment checked before anything is played; `VOICE_BIN` runs another
  lamp-voice for the run. It refuses to start when any `/run` drop-in is
  on lamp-voice (`zz-desk-kit.conf` included: another run's, or one left
  behind, to be removed by hand) or its face is already shown, and stops
  on an HTTP 429 from the relay in the voice's journal since that arm
  began (by the lamp's clock, so not an earlier arm's or run's). On any
  exit it closes the face, and
  removes the drop-in and restarts the voice only when this run wrote it.
  The file's first line names this run. Under one lock on the lamp
  (`flock /run/lock/desk-kit.lock`), the first arm writes it only if none
  is there, a later arm only while that line is still in it, and the
  put-back removes it only then. When the file is no longer this run's,
  it writes nothing and stops (exit 3). Each write is staged first, so a
  cut-off send leaves no empty drop-in. An interrupt before the first
  arm leaves the lamp as it was.
- **When the Mac is lost mid-run** (asleep, off the network): with each
  arm's drop-in the lamp arms its own guard, a `systemd-run` timer
  (`zz-desk-kit-guard`). One arm (`TRIALS` x 8 + 3 minutes) and 10 minutes
  after it, the guard removes the drop-in under the same lock, only while
  its first line still names this run, and restarts the voice only then.
  The put-back stops it. `stand-in.sh` arms the same kind of guard
  (`zz-played-stand-in-guard`, `PLAYED_GUARD_S`, default an hour), and
  `full-look-row.sh` one for the eyes. A guard is a transient unit, so a
  reboot clears it with the drop-in. Tests: `desk-kit-test.sh`,
  `stand-in-test.sh`.
- **The face**: `FACE`, an image or page on your Mac (never in the repo),
  shown full screen on the built-in display for the whole run (`FACE_SIZE`,
  default `1728,1117`), as a person facing the lamp. Without it no face is
  shown.
- **Who holds the lamp**: a line `HH:MM LOAN_NAME: ...` goes to `LOANS` on
  the lamp at the start and the end (default `~/loans.txt` and this Mac's
  user; `LOANS=` writes none). The loan line is a note, not a lock: the
  kit takes no lamp lock or lease. Its `flock /run/lock/desk-kit.lock`
  only orders its own drop-in writes. On a shared lamp, run the whole kit
  under the lamp's lease with `lamp-lock.sh <lamp> run` (as above; see
  `docs/joining.md`, "Sharing a lamp"), which renews it while the kit
  runs and stops the kit if the lease is lost.

What it scores, per arm (`desk_kit.py score`, printed and appended to
`$PLAYED_OUT/scenarios/desk-kit.tsv`; each scene's `.plan` beside it says
which line was which). From an ask's PLAY line to the next line's:

| Score | Means |
|---|---|
| answered whole | the lamp said something after a heard request that holds all the ask's key words (`hesitant.tsv`'s keys) |
| split | the ask came out as 2 or more heard requests |
| first words lost | the last request heard lacks the ask's first part (its longest word before the first pause, or "lamp"); of the asks with a pause that were heard |
| side talk let out | the lamp said something after a colleague's line, before the next line |
| answer time | from the end of the ask's wav to the first sound of the lamp's answer on its speaker (the scenario's `[lamp starts speaking]` line, its `speaker_first_write`; not the later transcript), ms, median and max; the start is read just before the clip begins, so 0.1 to 0.3 s long. Below zero: the lamp answered before the ask ended |

Empty heard and said lines are not counted. `desk_kit.py score SCENARIO`
also scores any scenario file without a plan: a line not ending in . ? !
goes on into the next, and it is `L` when it names the lamp (or give
`--marks LCC...`). `desk_kit.py selftest` checks the scorer on a fixture;
`--dry` runs it, prints the drop-ins and the scenes, and scores the
SCENARIO files given, with no lamp and nothing played.

## Night rounds (desk-kit-2)

`night.py ROUND [SCENE ...]` plays desk-kit-2's 50 made people's lines from
this computer's speakers to your lamp, scene by scene, and scores each line
from the voice's own events, its journal and the bus: words heard against
the script, lines split into pieces, cut-short words let go, lines joined
into a request whose answers never played (`joined`: the voice logs no line
for other joins) and joins it refused (`not_joined`), the
first answer sound, and whether each line was answered or held as its kind
says (desk talk between colleagues must be held, every other line
answered). A follow-up waits for the lamp's real answer: `a+S` plays S s
after the answer's last sample, `d+S` S s after its first (cutting in),
`g+S` S s after the previous clip. Nothing on the lamp is changed.

```sh
export LAMP_HOST=<your lamp's address>
lampOS/tools/played/night.py --check            # the kit only: no lamp, nothing played
lampOS/tools/played/night.py r1 weather desk1 moon
# On a shared lamp (<lamp> = lamp-52e6, lamp-75c9), under its lease; night.py takes none itself:
lampOS/tools/lamp-lock.sh <lamp> run <holder> <session> "night r1" -- \
  lampOS/tools/played/night.py r1
lampOS/tools/played/night_compare.py r1 r2      # two rounds side by side
```

- **The lines** are `desk-kit-2/lines.tsv`: 10 asks, 12 hesitant asks
  (`{0.8}` is a pause between pieces), 12 follow-ups, 4 long asks and 12
  lines of desk talk, from eight made voices coded `v1` to `v8` (ElevenLabs
  premade voices). The names in them are made up.
- **The audio** is one versioned tar, `desk-kit-2.tar`, fetched once into
  `$PLAYED_OUT` from https://cdn.autonomous.ai/os/test-kits/desk-kit-2.tar
  (`gs://s3-autonomous-upgrade-3/os/test-kits/`) and refused unless its
  sha256 is `NIGHT_KIT_SHA256` (`60128bcd…`), so every lamp hears the same
  lines. The WAVs (16-bit mono, 24 kHz) were rendered once through a lamp's
  own speech route, so no key is ever on a computer. `NIGHT_WAV=<folder>`
  uses WAVs on this computer instead.
- **Results:** `$PLAYED_OUT/night/ROUND.txt` (a summary line, then each line
  with what was said, heard and answered) and `ROUND.json`.
- **When:** run it in an empty room. It plays nothing between 12:00 and
  14:00 on weekdays (`LAMP_PLAY_AT_LUNCH=yes` overrides). It takes no lamp
  lock or lease: on a shared lamp, run it under `lamp-lock.sh <lamp> run`,
  as above.

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
| `NIGHT_KIT_TAR` | `desk-kit-2` | night.py's kit tar, fetched once into `$PLAYED_OUT`. |
| `NIGHT_KIT_SHA256` | the kit's | The sha256 the fetched tar must have, or it is refused. |
| `NIGHT_WAV` | the kit's `wav/` | A folder of the lines' WAVs on this computer, used instead of the tar. |

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
