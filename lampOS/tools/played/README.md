# The played test

A Mac beside the lamp speaks scripted lines from its own speakers, in macOS
`say` voices (Daniel, Samantha, Karen), and then reads the lamp's voice
session over ssh to print what the lamp heard and said, and when. A played
voice is a smoke test: the lamp hears replayed speech worse than a person.

**Other voices (`PLAYED_WAV_DIR`, off unless set):** a line already rendered
in that folder as `<sha12 of "VOICE|TEXT">.wav` (16 kHz, 16-bit mono, e.g.
the ElevenLabs set in the draft release `kit-played-el-1126`, with its
`voices.tsv` from each macOS speaker to its ElevenLabs voice) is played as
it is, in `scenario.py`'s `wav_for`; any other line falls back to `say`.
`stop-lines.sh` takes `STOP_LINES_SAY=el-say.sh`, a `say` stand-in that does
the same. A figure in other voices is no figure in macOS voices: October's
74.7% two-person floor was scored in macOS voices, so the first run in
another set scores stable in it too, as that set's own baseline.

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

- `stop-lines.sh LABEL [ROUNDS]`, about 8 minutes a round
  (#4710): on the second route, a stop always gets "Okay, I'll stop there."
  (`LAMP_SPOKEN_STOP_ENDS`) and a cut-in that ended the answer with no words
  gets "Um, excuse me, did anyone say something?" (`LAMP_CUT_IN_WORDLESS_ASKS`). Each round
  plays "Stop." and "Stop Lamp." over a story, "Stop." about 1 s after a short
  answer ended (its turn's `turn_finished` by the lamp's clock, not a playback
  interval's retirement, which a wait sound or a gap between sentences also
  makes), and a short system sound over a story, at 45% volume (put
  back after), and counts from the journal which line fired and whether its
  `first answer audio` followed (`$PLAYED_OUT/scenarios/stop-lines-LABEL.txt`).
  `STOP_LINES_ENV` puts switches on through a /run drop-in, taken off after.
  Its stand-in test is `stop-lines-test.sh`.

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

## The work records ROW (#7930 part 1)

`work-records-row.py LABEL --tasks-bin LAMP_TASKS --voice-bin LAMP_VOICE --clips DIR`, one command,
about 15 minutes, put back in under 5. Does lamp-tasks keep a piece of work's status up to date while
the lamp's OpenClaw does it, and does the voice say how far it is (`docs/tasks.md` "Work records" and
"The voice asks about its work")? Only work started by `start_agent_task`/`confirm_agent_task` (kind
`task`) keeps a record; makes, skill runs and hard asks are out of this kit. Each leg writes a verdict file under
`$PLAYED_OUT/scenarios/work-records-LABEL/` (`L0.json` … `putback.json`, `verdict.txt`, `run.log`).

- **L0, setup.** Reads the installed builds and lamp-tasks' `LAMP_LAMP_ENGINE`. Not `openclaw`: a
  drop-in sets it (`lamp-tasks.service.d/zz-7930-row-engine.conf`), logged to
  `/home/orangepi/changes.txt`. Stages both binaries (SHA-256 checked on the lamp, the installed ones
  kept beside a state file in `/var/tmp/work-records-7930`), restarts only lamp-tasks and lamp-voice,
  and waits until lamp-tasks answers with its lamp target on OpenClaw. A state file left by an earlier
  run is put back first (as below, round-aware) and the run goes on; when that put-back fails, it stops
  with nothing staged and the state kept.
- **A lamp-update round** (its `current` links moved since L0, or its journal says `switched to build`
  or `installed /opt/lampOS/`; its routine "OpenClaw may be installed now" is not one) is looked for before each leg and when a record can't be read: the legs stop.
  lamp-update is never stopped or touched.
- **L1, a task, no voice.** First waits up to 60 s for `request.sock` and the kept session (lamp-tasks'
  journal `kept session … idle|working`); no socket is a clear FAIL. Then the voice's own ops: `status` and `prepare` on `request.sock`, `confirm`
  on `voice.sock`. Then `{"op":"work","id":…}` every 2 s, up to 4 minutes. PASS: at least two states,
  `tools` above 0 and rising, `step` set, and `done` with an answer.
- **`step` only with the steps switch** (c0a4-16): lamp-tasks reads tool calls into a record's `step`
  only with `LAMP_TASKS_STEPS=on`, which is off in the product, so `step` stays null there. The kit reads
  lamp-tasks' environment once (logged `LAMP_TASKS_STEPS=on|off`). On, L1, L4 and HW require the step as
  above. Off, they require its state, a tool counted and its session instead (L1: `session_filled` in
  place of `step_set`; L4: `record_has_id_state_tools_session` in place of `record_has_id_state_step`;
  HW: the same check on the record as the how was heard), and the reply naming a state.
- **L2, the list.** `work_list` while L1 runs and after it: the task newest both times.
- **L4, the voice side.** No spoken yes: a task is handed over by `request.sock` as in L1 and waited
  for until `working` (with a tool if one comes, up to 90 s). Then only the "how far is it?" clip plays
  (`en-how`, later `vi-how`, the same task while it still works; `--how-clip what` for "what is it doing
  now?"), from the Mac at its own output of at most 70% (read, never changed; the lamp's volume is never
  touched). PASS: `check_agent_task` called on that turn or the done record's answer told (in any
  language: two of its numbers said, within 1, or 2 for a percentage said as a fraction such as "a
  quarter" or "three quarters"; or three of its longer words), the reply naming the state (the answer
  told counts), and the record's id, state and step; the record's answer is kept. A `conflict` on the
  hand-over's prepare or confirm (another task moved the store's revision) is read again and asked again,
  up to four times. Every lamp-voice line naming a request in each turn is kept in `L4.json`
  (`journal_by_request`: the read-back, a yes taken in code, the calls).
- **L4s** (`--l4-spoken`, its own leg, never blocking): the old spoken flow, ask, yes 25 s later (after
  the lamp's read-back), then how; scored as L4 plus the hand-over call.
- **The running binaries, before and after every leg** (L1, L2, L4, L4s, HW): each unit's
  `/proc/<MainPID>/exe` is read (its SHA-256, one sudo). Either one not the installed test build's makes
  the leg VOID ("the binary changed mid-leg: lamp-update rolled the lamp"), never PASS or FAIL; the run
  stops there and the put-back follows. lamp-update stays running by rule, and a stable promote mid-run
  would overwrite the hand-copied builds.
- **A skipped leg costs nothing**: the lock and a round are looked at just before each leg that runs, never
  for a skipped one.
- **HW, "how far is it?" while a long do-task runs** (only when `--only` names it; en only): plays
  `en-long-do` (a do-task through `start_agent_task`, then `en-yes`); its work record (kind `task`) is
  polled until `working` with a tool (`--running-cap 150`), then `en-how` plays and the record is read as
  it ends. PASS: `check_agent_task` on the how turn, the record `working` as the how was heard, and the
  reply naming a running state ("working", "still", "looking", "checking", "searching") or the step's
  tool as words, not the answer. VOID when the record was already over then ("finished too fast"). Render
  the long do-task with `work-records-clips.sh --only en-long-do DIR`; `--clips DIR:DIR` looks in each folder
  in turn.
- **`--langs en,vi`** (both by default): the languages L4 and L4s play and score, en first; `--only L4
  --langs vi` plays vi's how alone and keeps only vi's checks.
- **`--only L4s`** (or `--only L4,L4s`): those legs alone; `--only` takes L1, L2, L4, L4s and HW. L0 and
  the put-back always run, the rest are `SKIP (--only)`. Naming L4s turns it on.
- **The lamp lock.** Before L0, before each leg and before the put-back, the kit runs
  `lampOS/tools/lamp-lock.sh <--lock-name, lamp-c0a4> status` (`WORK_LOCK_SCRIPT` names another
  script, the tests' stand-in); held by anyone but the runner's own holder and session (or free,
  expired, or not readable), it stops at once, logs `lock not held: <holder>`, and does nothing more on
  the lamp: no put-back either (`putback NOT RUN`; the state stays for `--put-back` under the lock).
  The holder and session have no default: `--lock-holder builder --lock-session session`, or
  `LAMP_LOCK_HOLDER` and `LAMP_LOCK_SESSION`; with either unset the kit refuses to start and says which.
  While it runs it renews the lock (`renew <holder> <session>`) every 3 min (`--lock-renew`); a failed
  renewal stops it the same way.
- **Stopping it.** `--deadline HH:MM` or `HH:MM:SS` (local time) stops the legs there and begins the put-back;
  SIGINT (Ctrl-C) or SIGTERM do the same at once. The leg under way gets no verdict (`stopped
  STOPPED` in `verdict.txt`). A signal during the put-back is held, so it always runs to its end.
- **Put-back**, always, and alone with `--put-back`: the installed binaries back (checked against the
  SHA-256 read before), the drop-in removed, L0's ask helper (`/tmp/work-records-ask.py`) removed, both
  units restarted, read twice (the helper gone in each), logged. A lamp-voice
  hard-asks drop-in an older kit run left (`lamp-voice.service.d/zz-7930-row-hard-asks.conf`, under
  `/run` or `/etc`) is removed too, and both places are checked in its reads; with no state file, such a
  leftover is still removed and the units restarted. After a round,
  nothing is copied over the files the units now run: our build is taken out of the old release folder
  alone, and the units are judged by their current links (not our build), not by L0's SHA-256.

`work-records-clips.sh DIR` renders the clips through the lamp's own voice relay (ElevenLabs, the
lamp's credential read as root into curl and never printed) and checks each one's length and that
none is silent, has over 0.3 s of quiet before its first word or a pause over 0.4 s inside; each clip's
pauses are trimmed to 200 ms first (`--trim DIR` alone), so "Hey Lamp" is never heard as a line of
its own. `--check DIR` checks only. The dry run, no lamp: `work-records-test.sh`, against
`work-records-stand-in.py` (a stand-in lamp-tasks with its sockets, units and journal under a temp
folder; `WORK_SSH` and `WORK_ROOT` point the kit at it, never set for a lamp).

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
- **Level:** each clip is played at one loudness, -20 LUFS (BS.1770-4),
  by one linear gain under a -1 dBTP true-peak ceiling and never limited
  (`level.py`). A clip that cannot reach -20 under the ceiling is capped
  and marked. A clip with nothing above the -70 LUFS gate (silent or near
  silent) is copied at 0 dB and marked `unlevelled`. A clip under 400 ms,
  shorter than one gating block, has no integrated loudness in BS.1770-4:
  `level.py` meters the whole clip as one block and levels it by that
  (a 200 ms 1 kHz tone at -20 dBFS reads about -23 LUFS here, -70 in
  ffmpeg's `ebur128`), so its level is a best guess, not the standard's.
  The levelled copies are made once into
  `$PLAYED_OUT/desk-kit-2/level-m20/` and listed in its `level.json`, with
  the meter's version (`METER_VERSION` in `level.py`): a copy is made again
  when its source, the target, the ceiling or the meter's version changes.
  Each file is written through a temp file of its own, so two runs into one
  `PLAYED_OUT` do not overwrite each other's. The tar's clips are never
  changed. Each row of `ROUND.json` keeps the clip's `source_sha256`,
  `played_sha256`, `gain_db`, `lufs_out`, `capped` and `unlevelled` (true
  for a raw WAV with `NIGHT_LEVEL=off` and for a clip `level.py` could not
  level), and the summary its `level` (`-20`, or `off` with
  `NIGHT_LEVEL=off`, as every round before #5540 was) and its counts of
  `capped` and `unlevelled` rows. `night_compare.py` shows both counts and
  lists each line played levelled in one round and unlevelled in another:
  compare `not_heard` across levels only saying so.
  `level.py DIR OUT [--target N]` levels a folder of WAVs by hand. Its
  test is `level-test.sh` (python3 only; ffmpeg's `ebur128` checks it too
  when ffmpeg is there).
- **Results:** `$PLAYED_OUT/night/ROUND.txt` (a summary line, then each line
  with what was said, heard and answered) and `ROUND.json`.
- **When:** run it in an empty room. It plays nothing between 12:00 and
  14:00 on weekdays (`LAMP_PLAY_AT_LUNCH=yes` overrides). It takes no lamp
  lock or lease: on a shared lamp, run it under `lamp-lock.sh <lamp> run`,
  as above.

## The scored voice set (voiceset.py)

`voiceset.py LABEL [GROUP | GROUP/SCENE ...]` plays every voice use case as
a scene, in macOS voices from this computer's speakers. It then scores each
line from the voice's own events against what should happen:

- `A`: answered.
- `Q`: left alone.
- `S`: the lamp stops talking.
- `-`: played and kept, not scored.

It is the set for runs compared day by day and week by week: the same lines,
the same voices and the same beds every time. `voiceset.py --list` prints
the groups:

| Group | What it plays |
|---|---|
| `calls`, `calls-2` | "Hey Lamp" in every form and position, in about 20 voices and accents |
| `follow-ups`, `multi-turn`, `corrections` | Conversations of 2 to 4 turns with the name only at the start, a change of mind, and a line 35 s after an answer (left alone) |
| `questions`, `everyday`, `controls`, `controls-2`, `timers`, `timers-2` | Asks of each kind, the light by voice (and back), and timers rung out; never the volume |
| `delivery` | Quiet, fast, slow, hesitant and long asks |
| `cut-ins` | Stopping the lamp, and a new ask over its answer |
| `busy`, `room`, `room-2`, `two-people` | Colleagues' talk, phone calls, TV, greetings to people, another person's name, and two people at the lamp |
| `noise` | A call and its follow-up over make_noise.py's beds (babble, TV, a fan, music, typing) at two levels, with someone else's line in the noise |
| `day` | The day from `day.sh`, its memory lines and computer task swapped for lines of the same kind, at day.sh's gaps (`october-day`) and as a person paces it (`paced-day`) |

```sh
export LAMP_HOST=<your lamp's address> LAMP_SSH_KEY=<your key file>
lampOS/tools/played/make_noise.py              # once: the noise beds
lampOS/tools/played/voiceset.py v1125          # every group, about three hours
lampOS/tools/played/voiceset.py d1125 day/october-day calls room
```

- **Waits.** A line's wait is seconds after it, or `6a`: 6 s after the lamp
  is done with the line, its answer played to its last sample or its turn
  ended with none (`scenario.py`'s `lamp_done`). A follow-up then comes as a
  person's would, however long the answer was.
- **Beds.** A `("bg", NAME, VOLUME, '')` step plays `$PLAYED_OUT/prompts/noise/NAME.wav`
  under the rest of the scene, at afplay's VOLUME. `make_noise.py` makes the five
  beds, deterministic, with no recording anyone owns.
- **Safe on a person's own lamp.** No line writes to the lamp's memory,
  starts work on a computer, puts the lamp to sleep, turns privacy on or
  moves its body, so a lamp with no head touch never needs waking. Run it
  in an empty room. Scenes are 30 s apart (`VOICESET_GAP`), longer than a
  conversation's 20 s, so each starts outside one.
- **Results.**
  - `$PLAYED_OUT/scenarios/voiceset-LABEL.txt`: the scorecard. A line a group, each miss with what was heard, and the first answer sound's median and p90.
  - `voiceset-LABEL.tsv`: a row a played line.
  - `voiceset-history.tsv`: a row a group a run (when, label, lamp, group, right, lines), for the trend.
- **Its limits.** A played voice from a speaker is heard worse than a
  person, and `say` voices are not people. The lines are a smoke test of
  each use case, and the desk kit's made voices (above) are the better test
  of hearing.

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
| `NIGHT_LEVEL` | `-20` | The loudness in LUFS night.py plays every clip at (one gain, -1 dBTP ceiling), or `off` for the raw WAVs. |

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
