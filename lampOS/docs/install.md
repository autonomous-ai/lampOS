# Installing lampOS on a new lamp

Task [#811](https://github.com/autonomous-ai/lampOS/issues/811) (the ninth
builder), for shipping to homes on 27 November (Queue #705). Today every part
reaches a lamp by hand: the first builder's scripts and unit files
(`deploy/lamp-4ace/`, fourteen `*_deploy.sh` kept off the repository), or the
fifth's on lamp-52e6 (`deploy/lamp-52e6/`). A lamp in a home needs one
install that puts every part, its unit and its settings on a blank lamp from
a signed build, and keeps them current.

This page is step 1 of the task: what a lamp runs today, part by part, and
which of it is the product and which is one lamp's own. The product units
(`deploy/product/`) and the install command come after it and are written
against it.

**Kind:** a document. Nothing on any lamp changes.

## What exists and what we take

Read on 2026-10-15 at `lamp-voice` `462d04cee`; the updater's entry is
brought up to date on 2026-10-28 for #4065, a part added over the air.

- **lamp-4ace's units** (`deploy/lamp-4ace/`, 15 units and a timer, the
  controls' and Expression's settings, `RUNBOOK.md`). Every part is its own
  unit under `multi-user.target`, run as the board's user `orangepi` (the
  voice as root), with no `lampos.target`, no slices and no keeper
  (`LAMP.md`: "each its own systemd unit, no keeper yet"). Taken: every
  option and switch as the product's starting values, below. Not taken: the
  host name in each unit, the board user, settings kept inside a build's
  folder.
- **lamp-52e6's units** (`deploy/lamp-52e6/`, 12 units and its own
  `lampos.target`), the fifth's. Closer to the templates: `PartOf=` and
  `WantedBy=lampos.target` on every unit, the body `Type=notify` with
  `WatchdogSec=5`. Taken as the second lamp to compare with: what two lamps
  share is the product, what differs is either one lamp's own or one lamp
  being behind (below).
- **The templates** (`deploy/units/`, 12 units, the target and two slices):
  `@NAME@` placeholders filled by hand for each lamp
  ([README](../deploy/units/README.md)). Taken: the target, the slices, the
  order and the limits ([keeper.md](keeper.md)). Not taken: the
  placeholders. They mix three things: the build's hash (which an update
  must change), a lamp's measurements (which only that lamp has), and values
  every lamp shares; and `@VOICE_OPTIONS@` holds the voice's options
  whole, though the two lamps' are the same.
- **The updater** (`lamp-update`, `crates/update`, [updates.md](updates.md),
  #737/#741). It installs a signed build's binaries part by part into
  `/opt/lampOS/<folder>/<sha256>/` and switches a part by writing the new
  hash into its unit (`crates/update/src/unit.rs`, `with_build`). It knows a
  part only by a unit in `/etc/systemd/system` whose `ExecStart=` names such
  a folder, adds a part only as `install` would and only from level 8
  (updates.md, "A part added over the air"), and carries files
  beside a binary from the old folder to the new (the controls' settings).
  Taken: the signed manifest, the install beside, the quiet moment, the
  watch and the way back, all as they are. To change: what a switch writes
  (below, "One question for the updater").
- **The keeper's package** (`crates/keeper/src/package.rs`, keeper.md, "One
  service for the lamp"). The old runtime's updater installs one package,
  `/opt/lampOS/lamp-os/<version>/` with `bin/service` (the keeper),
  `VERSION` and `units/`; the keeper links every unit in `units/`, enables
  each part, ties `lampos.target` to its own unit and starts it. Shown on
  lamp-52e6 on 2026-10-14. Taken: linking the release's units, the bus
  root made as lamp-4ace has it, the old runtime's units masked. Open: this
  is a second way for software to reach a lamp, beside `lamp-update`
  (below, "Two ways in").
- **Setup** (#806, the third; `docs/setup.md` on `third/setup`). V2 ships
  with V1's setup flow. Setup writes the Wi-Fi, the voice's credential file
  and the person's name and time zone, and a marker; nothing of lampOS
  listens before it. Taken: install writes none of what setup writes, and
  starts nothing before the marker. The update key (`/etc/lampOS/update/key.pub`)
  comes with the image, not from setup (the third's proposal on #806).

## The parts

Every binary with a unit on a lamp today, and the three outside the lamp
itself. "User" is lamp-4ace's; the template's is in brackets where it
differs. Folders are what the unit makes or names.

| Part | Binary | User | Folders it keeps | Settings it reads | Sockets and files it makes |
|---|---|---|---|---|---|
| voice | `lamp-voice serve` | root | `/var/lib/lampOS/voice/` (sessions, first output, reminder delivery); `/run/lampos-voice/` (0700, kept across restarts) | `/etc/lampOS/voice/provider.json` and the credential it names; the character file | the thread `/run/lampos-voice/thread.json` |
| body | `lamp-body` | orangepi, `dialout` | `/run/lampos-body/` (0755); the servo lock folder | none: its poses are options | `mood.sock`, `gaze.sock` |
| eyes | `lamp-eyes` | orangepi, `video` | `/var/lib/lampos-eyes/` (0700, `people.json`); `/run/lampos-eyes/` (0755); the camera lock folder | two models, `/opt/lampOS/eyes/*.onnx`, outside the build's folder | `permit.sock`, `who.sock`, `present`, `known`, `sight.jpg` |
| name | `lamp-name` | orangepi | none | models, `/opt/lampOS/name/models`, outside the build's folder | the bus |
| expression | `lamp-expression serve` | orangepi | none | a catalogue, `/opt/lampOS/expression/catalogues/*.json` | the bus |
| actions | `lamp-actions` | orangepi, `audio` (product: lampos, `audio`) | `/run/lampos-actions/` (0755, kept across restarts); product: `/var/lib/lampos-output-caps/` (0700) | none | its folder; the bus |
| memory | `lamp-memory` | orangepi (`@MEMORY_USER@`: its own) | `/var/lib/lampos-memory/` (0700); `/run/lampos-memory/` | none | `memory.sock` |
| policy | `lamp-policy` | orangepi (template: `DynamicUser=yes`) | `/var/lib/lampos-policy/` (0700) | the judge's key, `/etc/lampOS/policy/jev.key`, as a systemd credential | the bus |
| health | `lamp-health serve` | orangepi | none | none: what it expects is options | the bus |
| controls | `lamp-controls serve` | orangepi | none | `controls.json` and `controls-pins.json`, inside the build's folder | the bus |
| ears | `lamp-ears` | orangepi, `audio` | none | none | the bus |
| schedule | `lamp-schedule serve` | orangepi | `/var/lib/lampos-schedule/` (0700); `/run/lampos-schedule/` (0700) | none | `call.sock` |
| air | `lamp-air` | orangepi, `/dev/i2c-0` | none | none | the bus |
| air's bus clock | `lamp-air limit-clock --bus-number 0` (a oneshot, before air and controls) | root | none | none | I2C bus 0's `device/freq` |
| tasks | `lamp-tasks serve` | orangepi | `/var/lib/lampos-tasks/` (0700); `/run/lampos-tasks/` (0700) | `targets.json` in its state folder; the Harness program in the user's home | `request.sock`, `voice.sock` |
| mood | `lamp-mood serve --zone system` (off unless `LAMP_MOOD=on`, `mood.md`) | lampos | none | the lamp's zone, `/etc/localtime` | the bus (`mood/state`) |
| addons | `lamp-addons serve` (off unless `LAMP_ADDONS=on`, `addons.md`) | lampos | none | none | the bus (`addon/proposal` and the lamp's state in, `light/request` out) |
| cloud | `lamp-cloud --bus-root /run/lampos-bus` (on in the product, `LAMP_CLOUD=on` in its unit since #1815, `cloud.md`) | lampos | `/var/lib/lampos-cloud/` (0700, its `StateDirectory`) | setup's `backend.json`, the voice's key and update's `state.json`, through `LoadCredential=` | the company broker and the backend's ping; reads the bus |
| records | `lamp-records check` (a timer, hourly) | root | `/run/lamp-records/` | `/etc/lampOS/records.json`, which install places (`deploy/product/settings/records.json`; it does nothing without it) | `last.json` |
| selftest | `lamp-selftest --occasion first-boot` (a timer: 5 min after the lamp starts, then every 6 h; #812) | lampos, `CAP_SYSLOG` | `/var/lib/lamp-selftest/` (its own `StateDirectory`) | none | the bus (`lamp/selftest`), its runs' reports |
| update | `lamp-update serve` | root | `/var/lib/lampos-update/` (0700) | `/etc/lampOS/update/key.pub` | none |
| keeper | `lamp-keeper serve` | root | its service data folder | none | `lampos.target.d/keeper.conf`; the bus root |
| skills | `lamp-skills` | lampos (product unit, #811) | `/var/lib/lampos-skills-state/` (0700, `daily.json`); `/run/lampos-skills/` (0755, runtime) | the skill folders, `/var/lib/lampOS/skills/` (every product skill from install) | its tools for the voice, `tools.json` and `call.sock` in its runtime folder |
| surround | `lamp-surround serve` | `@SURROUND_USER@` | not read for this page | `/etc/lampos/surround.json` (lower case, unlike every other part) | not read |

The bus root, `/run/lampos-bus`, is made by the keeper (root, the parts'
group, mode 2770) or by whichever part comes first; every part above but
update, records and surround publishes or reads there.

Binaries the build carries with no unit, which an install leaves alone:
`lamp-turn-end` (a worker the voice starts from its own folder), `lamp-bus`
(a tool), `lamp-audio`, `lamp-observer`, `lamp-voice-eval`,
`lamp-sound-fixtures` ([deploy/units/README.md](../deploy/units/README.md)).

What else a part needs that no build carries: the voice's turn-end model
and ONNX Runtime library (beside the voice in its folder,
[endpoint-detection.md](endpoint-detection.md)), the eyes' two ONNX models,
the name's models, Expression's catalogue, the controls' two files, the
character file, `provider.json`, `records.json`. lamp-update's manifest
carries binaries only ([updates.md](updates.md), "Not shown": "models and
libraries a build does not carry, configuration under `/etc/lampOS`").

## Order at boot

From [keeper.md](keeper.md) ("The parts and their order"), which names
eleven parts; the four it does not name are placed by what they serve
(proposed here, for the fifth to correct):

| Step | Parts | Why |
|---|---|---|
| 1 | memory, actions, health; **schedule, tasks** | The voice calls each of their sockets from its first turn (`LAMP_SCHEDULE_SOCKET`, `LAMP_TASKS_DIR`), as it does memory's |
| 2 | voice | |
| 3 | body, name, controls, policy; **ears, air, mood** | Ears, air and mood only publish on the bus, as name does |
| 4 | eyes, expression | |

Outside `lampos.target`, each started by `multi-user.target` on its own:
the keeper (which starts the target), update, surround, and the records
timer. Stop is the reverse.

Only a `Type=notify` part makes an `After=` wait. Today that is the body on
lamp-52e6 and in the template; every lamp-4ace part is `Type=exec` or
`simple`, so its order is only an order of forks.

## Product, or one lamp's own

Compared line by line: lamp-4ace's units against lamp-52e6's, with every
build hash taken out. Same board (Orange Pi 4 Pro), two lamps.

**The same on both: the product.**

- The voice's options: `--noise-suppression off --turn-taking duplex
  --faint-floor-db -28 --microphone mic2 --reference-clock capture
  --reference-recovery bounded --provider-continuation kept-start
  --playout-start-ms 350 --end-silence-ms 600
  --controls-stop --endpoint smart-turn-fast --call-wait-ms 2000`. On
  17 October `bounded` replaced `off` (lamp-cb94's voice ended on a held-up
  speaker with `off`; bounded recovery kept it up, #1298) and the call's
  wait came from lamp-4ace. The template's
  `@VOICE_OPTIONS@` calls them "the measured options of this lamp"; on two
  lamps they are one set. The ring's `--ring-channel-ceiling 24` is not among
  them: the template carries it itself (#2529), and the voice refuses the flag
  given twice, so it never goes into `@VOICE_OPTIONS@`.
- Every option of name, ears, air, actions, memory, schedule and controls
  (`lamp-name --microphone mic2 --least 0.9`, `lamp-ears --device
  plug:device_micro1 --rise-db 30`, `lamp-air --bus-number 0`,
  `--mixer-card allwinnerhdmi --mixer-control Speaker --volume-ceiling 100`,
  `--keep-days 30 --memory-days`, …), and `CPUAffinity=6 7` (the board's
  two fast cores; keeper.md).
- The body's speeds and limits: `--reach 150,15,1,250 --speed 300
  --servo-speed 1000 --servo-acceleration 60 --gaze-reach 480,120
  --stand-share 100 --stand-seconds 6 --awake-seconds 90 --idle on
  --force-limit 600 --wake on --rest-glance on --look-at-name on`.
- The controls' wiring, `controls-pins.json`: a property of the board.

**Different: one lamp's own.** Only these, and each is measured on its lamp
([body.md](body.md)):

| What | lamp-4ace | lamp-52e6 |
|---|---|---|
| Body `--gaze` | `727,553` | `782,557` |
| Body `--stand` | `1772,2043,2015,2657,2602` | `1772,2035,2035,2657,2602` |
| Body `--folded-arm` | `2048,2047` | `2053,2069` |
| Body `--home` | `1950,2043,2015` | `1771,2035,2035` |
| Body `--pitch-window`, `--torque on` | not given | `1670,1995`, `on` |

The camera's USB port is `3-1.1.4` on both, but not on every unit:
lamp-a63e's and lamp-dbda's is `3-1.3.4`, and the hardware can't change
that. So the product's eyes name the camera by its ids alone, `--usb
01da:5875`, with no port and no `--device`, and take the one camera with
those ids on whichever port it is, refusing none or two (eyes.md, #2190,
#4074). Their camera helper keeps `01da:5875@3-1.1.4` until every channel
carries a lamp-eyes whose helper takes no port, since these units come from
the updater's own build and an older helper exits 2 without one. The
install test holds both units to it.

**Different: one lamp behind the other.** lamp-4ace has switches lamp-52e6
lacks: the voice's `LAMP_TASKS_DIR`, `LAMP_SCHEDULE_TOOLS`,
`LAMP_SCHEDULE_SPEAK`, `LAMP_SCHEDULE_DELIVERY_DIR`, `LAMP_MEMORY_DAYS`,
`LAMP_EYES_WHO_SOCKET`; the actions'
`LAMP_SCHEDULE_TOOLS`; health's `--expect policy`. By joining.md's rule
("The switch is for that first minute, not for life") a switch on for
lamp-4ace becomes the product's default, so the product takes lamp-4ace's
set. The product's hello is the Welcome back skill (`lamp-skills`, #811),
spoken through the voice's say service; the voice's own hello and its
`LAMP_SPEAK_FIRST` were removed (#2825), so the lamp greets once
([speaking-first.md](speaking-first.md#one-hello-from-the-skill-2825)).

**19 October: every behaviour that has a skill runs as the skill** (the
owner: "everything is a skill"). Install puts every product skill in
`/var/lib/lampOS/skills/` (`install::PRODUCT_SKILLS` in `crates/update`),
and every core skill is a product skill (the owner: core skills are always
installed, installable ones as apps; `tests/install.rs` holds the two
equal, Laugh glance parked out of both): twenty-five folders are installed,
seventeen enabled, one of them hidden unless `LAMP_UNPLUG=on`. Welcome back, Reminders, Task news, Onboarding, Stretch
break, Stuffy air, Dance and Not feeling well keep their enabled defaults.
Night sleep (#940) is on: when the lamp sleeps at night, moved out of
Expression ([alive.md](alive.md#night-sleep-is-a-skill)), with Expression's
built-in sleep as the fallback for one release.
Startle (#2822) is the startle's move, Expression's glance at a sudden
sound: Expression reads its `[startle]` when it starts and plays it as its
reflex ([expression.md](expression.md#the-startle-a-glance-at-a-sudden-sound));
lamp-skills only checks it, and the catalogue no longer carries it.
Not feeling well (#1681) came with the product after its lamp check, row 8
on lamp-52e6 with the lead (20 Oct 11:35): one slow droop for a failed
part, and the light back after; it adds no function. Prepare-to-unplug is
enabled but stays hidden and inactive unless `LAMP_UNPLUG=on`.
Bedtime and Morning is included with `enabled = false`, awaiting its lamp
comparison (#1672); it adds no active trigger or advertised tool. Watches
you leave and Music sway (#1669) are on in the product since #2834, with
the eyes' `LAMP_ACTIVITY=place` for Watches you leave: the sitter followed
by place only, never by a face print. With `room/activity` said, Stretch
break, already on, also waits while a head is down (reading, writing) and
lets that sitting's stretch go if they don't look up within its ten minutes. Music sway reads
the ears' sound classes, which stay off in the product until YAMNet's
weights may be shipped (sound-events.md), so until then it never sways.
Seven core skills not yet tried on a lamp are
installed disabled (`install::PRODUCT_SKILLS_OFF`, each with why): Light out
when you leave (#1677, [presence.md](presence.md), which must never take the
light of someone reading face down), Light scenes, Point the light and Light
show (#1679), Smoky air and Dusty air
(#1684), until they are measured for false alarms on real traces, and
Feelings (#3691), which needs Mood on (`LAMP_MOOD=on`, trial-only). Install
and a round's bundle write `enabled = false` (`install::OFF_LINE`) above
each one's own `skill.toml`, so the folder in `lampOS/skills/`, and a lamp
run from it, keeps its own default and its cases; each adds no trigger and
no function, and a trial turns one on on the lamp itself. Each
folder is installed whole but for its `tests/`. The voice's own paths still
give way in the product's `lamp-voice.service`: it has no hello of its own
(#2825), and `LAMP_SCHEDULE_SPEAK=skill` and
`LAMP_TASK_NEWS=skill` make a due reminder and the news of a task wait up
to 10 s for the skill's ask (`from = "schedule"`, `from = "tasks"`). The
voice says each once, with the skill's ticket when the skill asked, so the
lamp looks up, sways or droops with them, or by itself when no ask came,
in the skill's own words either way: it reads `[news.words]` and
`[reminder.words]` from the installed `skill.toml` each time (#940), and
says its built-in words, with a journal line, for this release only, when
the installed file is missing, has none, can't be read, or has words that
fail the check (`or_built_in`). Each skill asks once, and an ask is gone 20 s
later (`say::FROM_LIVES_US`), so the two are timed to meet (the reviewer's
P2 on #1667). Reminders asks only while the voice rests (`voice/state`),
keeping a reminder due during a talk until the talk ends (`wait`), when
the voice can say it too: said then, with the look. Task news asks the
moment the work settles (its per-task trigger cannot wait), so the voice
counts its 10 s from when it had the news, not from when it can first tell
it: news that settled while nobody was there, or more than 20 s before a
talk ended, is told at once when it can be, by the voice, without the sway.
The voice says a reminder itself 10 s after it can when `lamp-skills` is
down, or when the ask came too early: at a pause after which the talk went
on for over 20 s, or while the voice rested but could not say it (paused
or muted: what `voice/state` says then was not checked). Task news takes news that
stopped or asks something too (#1722), with a look, and a curious tilt for a
question; the voice only words it, as it does done and failed. Left out, with
why (`install::NOT_PRODUCT_SKILLS`): Desk tutor, which reads handwriting
from the full look (`LAMP_LOOK_FULL=on` with `lamp-eyes --full-look`, in the
product since #2791's product half) but whose two functions with its marks'
would take a Live session past its 32, and its marks, called only after its look; the
other installable skills, which their owner puts on; and Laugh glance,
parked (#1669). A test
fails on a folder in `lampOS/skills/` that is in neither list. With them a
product lamp tells each session 31 functions of the 32 a session takes
(`lamp_gemini::MAX_TOOLS`): the voice's own 13, the actions part's 17 and
Dance's (`crates/update/tests/functions.rs`, which fails if one is left out
or the count changes). On the product's route each chat request adds the
web search's `web_search`, the 32nd, and leaves out its `fetch_page`, so
`dance` reaches the model
(`a_product_chat_request_offers_dance_at_the_bound`). At the bound a core
skill's function is never the one cut (the owner's rule, on #2614): a chat
request leaves out the search's `fetch_page`, then `the_time` (asked only
for elsewhere), then the skills' functions that are not a core skill's,
the last first, then `web_search`; a Live session only the skills'
not a core skill's. Core skills say theirs in the skills' tools file's
`core` list (`lamp_skills::tools_file`). The product's voice has
`LAMP_REPORTS=on` (#2862), two more of the voice's own, `reports_stop` and
`reports_state`: 37 of the lamp's own in all on the product's route, which
offers every one, the fetch among them
(`a_product_lamp_declares_every_function_it_has_within_the_bound`). A Live
session (a per-lamp drop-in, not the product's route) is cut to 32, and its
error line names `reports_state`; `reports_stop` and `dance` stay
(`with_reports_on_dance_stays_at_the_bound`). The built-in
instruction tells the model its head sits on an arm that moves and that a
dance is a call (lamp-0c89, 21 Oct, answered "I've no legs to dance with"
with `dance` given; `crates/voice/src/config.rs`). The voice leaves out by that order what
is past 32, so Desk tutor's two (installable) would push its marks'
`show_verdict` out, never `dance`. lamp-4ace's
own unit keeps `LAMP_SCHEDULE_SPEAK=on` and the voice's own news until the
skills have run there; the voice's own code for them is removed after.

Install only writes skills, never removes one: a skill later moved out of
`PRODUCT_SKILLS`, or a file dropped from a skill's folder, stays on every
lamp installed before and keeps running. Taking a skill out is the removal
work's (`lamp-skills remove` and the core and installable kinds, on
`first/skills-core-installable`), not install's.

A product skill's file is the build's, never the lamp's own (#1722): one
changed by hand is written over at install, as one removed is, and as an
update round puts both back ([updates.md](updates.md), "The product's
skills"). The person's own stays: the skills turned off
(`/var/lib/lampos-skills-state/off`), a skill's state beside it, and any
skill they installed in the same folder. A core skill turned on stays on:
install keeps it in `/var/lib/lampos-skills-state/on` and writes its
`skill.toml` turned on ([skills.md](skills.md), #2585).

The core `onboarding` folder is installed too. It contributes no model tool;
its privacy invitation bridge remains inactive without the voice's explicit
onboarding switch, Controls' privacy receipts (`LAMP_PRIVACY_CONTROLS=on`,
which the product unit doesn't set), valid hardware profile and pending
invitation. Private progress lives under `/var/lib/lampOS/voice/onboarding/`;
factory reset wipes that state, not the skill package.

The product units also name private durable stores for
[Bedtime and Morning](bedtime-morning.md). Skills is given
`Environment=LAMP_SKILLS_DAILY_FILE=/var/lib/lampos-skills-state/daily.json`
with `StateDirectory=lampos-skills-state`; Actions is given
`Environment=LAMP_OUTPUT_CAPS_DIR=/var/lib/lampos-output-caps` with
`StateDirectory=lampos-output-caps`. Both declare
`StateDirectoryMode=0700` and run as `lampos`; systemd creates and owns these
directories when their services start. Install writes the unit declarations,
not the runtime journals. The paths do not enable a feature.
`LAMP_SKILL_ROUTINES` (Skills) and `LAMP_REST_REQUESTS` (Expression) are on
in the product units for Night sleep (#940): its rest leased to Expression
and its claim on the night, with Expression's built-in sleep as the
fallback for one release; `LAMP_OUTPUT_CAPS` remains unset, and Bedtime
and Morning remains disabled. Actions
requires its caps directory only when `LAMP_OUTPUT_CAPS=on`.
The paths come as `Environment=` lines, not as the binaries' `--daily-state`
and `--caps-dir` options, so an older Actions or Skills from the channel
ignores them and starts as before (#2157). Factory reset empties both durable directories, including
interrupted writes and state kept aside, while retaining the private
directories ([setup.md](setup.md)).

**17 October: lamp-4ace's switches of that day became the product's** (joining.md's rule), taken from lamp-4ace's unit as it ran that day; most were switched on there on 16 or 17 October, several only as part of the groups of #1238's census, so their lamp record is that day's running, not a run of their own. They're the switch groups (lead-in to a tool, a waited answer kept, the stop told, the asker first only recorded, names checked, the staff note, task news at a pause, memory rules), the light requests (the voice's ring and the skills' half, `lamp-skills.service`), the quiet pace, starting offline, the asleep ring, a sorry in another language, the lamp's name in passing ignored, first words kept, tasks asked and who asked, a cut keeping the name, the self-test's speaker, the stall recorder, the strong ring eye, and short sounds judged in a quiet room after a check. The head-touch carry (#711, #2948) pairs the voice's `LAMP_HEAD_TOUCH=on`, `--controls-mute` (its record in `/var/lib/lampOS/voice/mute`, which `lampos.conf` makes and a reset empties) and `LAMP_RESTORED_MUTE=on`, Expression's `--head-touch`, the body's `LAMP_HEAD_TOUCH=on` (it folds when a hand puts the lamp to sleep, #1362), and the pins without `legacy_head`, all together. Not taken: those that need the second voice's configuration (stop words, its down-line and wait cue, the voice link), which a product lamp doesn't have; the office's own (its Harness target, its say group); those that need another part's flag the product doesn't pass (the Policy holding actions needs `lamp-policy --act attention`, its hello and progress news need `--hello` and `--progress`, the full look needs the eyes' `--full-look`); and "that was weird", a builder's review tool whose marks (`/var/lib/lampOS/voice/marked/`) the reset doesn't wipe. Expression's catalogue differs too (`expression-alive-name-speech` on
lamp-4ace, `expression-alive` on lamp-52e6): the product takes lamp-4ace's, plus the `speaking` section (#933): the nods on stressed words, the base's sway between sentences (#1067) and the settle at the end, which no catalogue in `deploy/` had, so a product lamp only leaned while it spoke (the body's lean, #1010, needs no catalogue). `speaking_engine`'s `the_products_catalogue_nods_sways_and_settles_as_the_alive_one_does` holds the product's to the alive catalogue's.

The head-touch evidence is two separate checks. Fifth's attended lamp-52e6
hand trial (#2857) passed tap, stroke, a 3 s hold to sleep, a wake tap and
20 s with hands away. Thirteen's lamp-c0a4 check killed Voice with SIGKILL
while listening, observed the restart guard's mute, then reached readiness
about 300 ms after an injected tap; a chosen sleep mute stayed closed.
That injected-input result is recovery/readiness evidence, not a real-hand
or first-speaker latency measurement. Separately, the lamp-c0a4 hand trial
found the tap cue audible only with an ear at the lamp at −30 dBFS, faint at
−25, and clear at −20 at the product speaker level. The 23 October decision
sets its default to −20 dBFS. That tap trial does not qualify the shared
sleep-breath or other hum paths, or establish acceptance on this exact build.

**Neither: the person's.** The time zone (#820). Setup links `/etc/localtime`
to the zone the person's phone gives and names it in `/etc/timezone` (#806).
Memory, Expression and skills take `--zone system`, which follows that file,
across daylight saving and without a restart (skills since #1384). Schedule
takes `--zone ${LAMP_ZONE}` from `zone.env`, which install writes from the
image's zone and setup writes again with the person's. The voice
reads `/etc/localtime` through the same crate, `lamp-local-time` (#820):
the time it tells, night, and the days of what is remembered and marked,
each moment with its own offset, a new zone taken up without a restart. The policy keeps none: only its
hello and progress news would use one, and the product doesn't pass them.
`install`'s `the_parts_that_keep_local_time_read_the_zone_setup_saves` holds
the units to this.

**Paths and users that are lamp-4ace's by history, not by need:** the board
user `orangepi` and things in its home (the servo and camera lock folders
under `~/.local/share/lampOS-voice/`, the Harness program in
`~/.local/bin`); the voice under `/opt/lampOS/releases/` where every other
part is under `/opt/lampOS/<part>/`; `ConditionHost=` in every unit.

## The product layout (proposed)

What steps 2 and 3 build, unless an answer on #811 changes it.

- **One user**, `lampos`, a system user with no login, in `audio`, `video`,
  `dialout` and the group that may read the GPIO chips and `/dev/i2c-0`;
  memory its own (`lampos-memory`) as the template says; policy
  `DynamicUser=yes`; the voice, update, records and the keeper root, as now.
  `lampos` is then the parts' group the keeper gives the bus root.
- **Builds** under `/opt/lampOS/<part>/<sha256>/` as now, and the voice
  under `/opt/lampOS/voice/`. Each part's unit names
  `/opt/lampOS/<part>/current/lamp-<part>`, `current` being a link to the
  build's folder, so an update changes the link and not the unit. A few
  binaries go into another part's folder. The voice's `lamp-turn-end` is
  required (`install::BESIDE`): a build without it is refused whole. The
  body's factory tools `lamp-body-measure` and `lamp-body-mood` go beside
  `lamp-body`, where `deploy/factory/steps.sh` runs them (#2189), but only
  when the build carries them (`install::BESIDE_IF_CARRIED`). A build from
  before they were made (19 Oct) still installs every part, and install says
  once for each tool that it was left out, so a lamp can be put back on an
  older build (#2249). A test reads steps.sh's `*_BIN` defaults and fails when
  a step runs a tool that install does not put there.
- **What a build does not carry**, never inside a build's folder: models in
  `/opt/lampOS/<part>/models/` (the eyes' two, the name's), beside
  `current`; the product's settings in `/etc/lampOS/<part>/` (decided by the
  first builder on #811, 2026-10-15 17:37): Expression's catalogue as
  `/etc/lampOS/expression/expression.json`, the controls' two files in
  `/etc/lampOS/controls/` (the pins name the button, the head pad lines and
  the reset button, line 100; the unit serves `--touch`, the path whose
  reset event `watch-reset` reads. The product's Voice and Expression read
  the head's `controls/touch` together, with Voice's `--controls-mute` and
  the pins' `legacy_head` absent: a tap stops speech and a 3 s hold asks
  Voice to sleep, including its software mute, #711). A pins file changed by
  hand is kept, and must pass the touch path's checks, which are stricter
  than the old reader's (chip 0, the button on line 99, head pads on 37,
  96, 97 or 98), or Controls does not start, and the button with it. A
  lamp's own pins file with no `privacy` section gains the build's, and
  only that key; the rest and its mode are kept, and install says so
  (`own_pins_follow_build`). The product's units select the privacy route, and
  without the section Controls has no privacy source and the lamp never
  hears (the reviewer on #2143). A lamp's own pins file whose head pads are
  still the old guess (37 and 96, active-high, pulled down, with no
  `head_rest`) gets the build's four lines the same way (#1798): the guess
  read no touch on lamp-52e6 or lamp-cb94. Pins the factory step wrote are
  kept. A lamp's own `controls.json` that names the old two pads then names
  the same four, so a Controls put back from before #1798, which wants both
  files to name the same pads, still starts there. It doesn't where the
  factory step wrote the pins (two or three pads, kept): install's
  `controls.json` now names four. Going back by hand across #1798 on such a
  lamp, also put back `controls.json`, or name the pins' own pads in it
  (updates.md, "To go back by hand"); a round's put-back writes no settings.
  A lamp's own `legacy_head: true` goes too, as the build's pins have none:
  the product's voice and Expression read the head from `controls/touch`
  (#711), and a pat must not also be the old engine's `Pet` beside them
  (#1176). Install first verifies the resulting product readers and the
  relevant local overrides it preserves, before touching the pins or units
  (the preflight in "The install command", below). `/etc/lampOS` is made
  0755 and each of these
  folders 0755, files 0644: readable by `lampos`. A default found with install's own bytes but another mode is install's, and is given its mode (lamp-cb94's settings were there before any install at 0600, and Controls and Expression could not read them, #705); a lamp's own setting found closed is kept as it is, and said. lamp-4ace's `/etc/lampOS`
  is root's alone; `install` does not open one made closed (credentials may
  be below it) and says so. The voice's turn-end model and ONNX
  Runtime library stay beside the voice, where the voice looks for them,
  and lamp-update carries them from build to build as it does now.
- **The Store, prepared but off.** Install writes
  `/etc/lampOS/skills/store.env` with `LAMP_SKILLS_STORE=off`, read by both
  `lamp-cloud` and the skills manage helper. It also writes the helper's
  `/etc/systemd/system/lamp-skills-manage@.service.d/store.conf`, which reads
  that file and prepares IPv4 and IPv6 for fetching packages. These are
  defaults: a lamp's edits are kept on another install. They are install's
  `STORE_SETTINGS`, not files carried by an update round's settings manifest.
  The product cloud unit keeps the Store off; both parts require a valid
  separate Store key before accepting an install, and the helper creates
  no HTTP client while off or without that key. Removing a skill, or turning it off or on, remains
  available. The prepared helper sandbox has network permission even while
  off; its switch and key checks prevent a fetch.

  Factory step 3 places `/etc/lampOS/skills/store-key.pub` with
  `--store-key FILE` (below); neither installing nor placing the key
  enables the Store. After provisioning the key and qualifying the Store
  on the lamp, root turns on the shared switch with one line:

  ```sh
  sed -i 's/^LAMP_SKILLS_STORE=off$/LAMP_SKILLS_STORE=on/' /etc/lampOS/skills/store.env && systemctl restart lamp-cloud
  ```

  The next helper connection reads the changed file itself; no helper
  restart or unit edit is needed. Setting it back to `off` and restarting
  `lamp-cloud` closes future install requests. The helper still reads its
  key again before a fetch, and never falls back to the update key.
- **A lamp's own values** in one file per part, `/etc/lampOS/<part>/lamp.env`,
  read with `EnvironmentFile=` and put into `ExecStart=` as `$NAME`
  (systemd splits an unquoted `$NAME` into words): today only the body's,
  `BODY_POSES`. The file is required: without it the body does not start,
  so a body never moves on poses measured on another lamp. The product unit
  never changes for one lamp; a lamp's file is written once, when the lamp
  is measured, and no update touches it.
- **The zone**, `/etc/lampOS/zone.env`: `LAMP_ZONE` (schedule) and
  `LAMP_UTC_OFFSET_MINUTES` (for a unit that takes an offset), written by
  the install from the system's zone and again by setup with the person's
  (#820). Memory, Expression and skills follow `/etc/localtime` instead
  (`--zone system`, #1384).
- **Locks, the bus and the voice's state**, `/etc/tmpfiles.d/lampos.conf`:
  `/run/lampos-bus` as the keeper makes it, and `/run/lampos-locks/{motor,camera}`
  (each 0700 and `lampos`'s, under a root 0755 parent, as the body and the
  eyes require) in place of lamp-4ace's lock folders under the board user's
  home; `/var/lib/lampOS/voice/sessions` (root, 0700), which the voice's
  unit names and nothing else makes; and `/var/lib/lampOS/consent`
  (root:lampos, 0750), where the voice keeps the room sounds yes for
  lamp-ears (#1722 item 1). Both were found on lamp-06c1 on
  2026-10-16: with the locks 0775 the body never opened the servo port, and
  without the sessions folder the voice could not start.
  It also makes `/run/lampos-photos` (`lampos`'s, 0700), the app's photos
  for the eyes when `LAMP_PHOTO_FACES` is on (cloud.md).
- **The body's unit id** is the host name (`--unit-id %H`), and no unit
  says `ConditionHost=`.
- **The clock** (#811): a lamp has no battery clock, so after a boot its
  wall clock is its last saved time until chrony sets it, 30 to 70 s later.
  Every part that acts on the time of day trusts it only once the kernel
  says it is set (`lamp_ipc::clock_synced`): the schedule with
  `--kernel-clock` (not `--trust-clock`), so nothing calendar-timed is made
  or falls due before; Expression with `--kernel-clock`, so no night by a
  wrong clock; the voice with `LAMP_KERNEL_CLOCK=on`, so it says it does not
  know the time rather than a wrong one. The Policy's hours need an offset
  the product unit does not give, so they act on no clock yet.
- **Switches** are in the product unit as `Environment=` lines, on; a lamp
  that must have one off says so in a drop-in
  (`/etc/systemd/system/lamp-<part>.service.d/`), which no update touches.
  A relevant head-reader override makes install refuse if its resulting
  mode conflicts with the product or cannot be verified; install leaves
  that drop-in and the head files untouched.
- **The person's settings** (zone, name, Wi-Fi, credential) are setup's, in
  setup's files.
- **Nothing starts before setup:** every part in `lampos.target`, and the
  target itself, has `ConditionPathExists=/var/lib/lampos-setup/done`,
  setup's marker (the path agreed with the third on #811, 2026-10-15
  17:07). The condition is on each part because systemd checks a unit's
  condition only when its own job runs, after the jobs it pulls in are
  queued: the target's alone held back none of its parts (the reviewer on
  #815, from systemd 252's `systemd.unit(5)`, "Conditions and Asserts").
  `lamp-update.service` and `lamp-records.timer` are outside the target
  and listen to nothing; they run before setup as after it. The install
  enables the target and the parts and starts none of them. On a lamp brought up by
  the keeper, the keeper's `--setup-marker` (#816, the fifth's) starts only
  `lamp-setup.service` until the marker is there; the target's condition
  keeps the same rule on a lamp without the keeper. `lamp-setup.service`
  joins `deploy/product/` when the third hands back its unit (#806).

## The updater and the link

Decided on #811 (the first builder 17:37, the second 17:46): a round knows
a product part by `current/lamp-<name>` and switches it by replacing the
link, keeps the old build in its journal and `previous/`, and puts the link
back to go back; a part that names its build in its unit (lamp-4ace,
lamp-52e6) is switched as before, both in one round
([updates.md](updates.md), "One round").

## Two ways in

A lamp can get lampOS as the keeper's package through the old runtime's
updater (lamp-52e6, 2026-10-14), or part by part through `lamp-update`
(lamp-4ace, build 188, 2026-10-15). For a lamp in a home there should be
one. Which one is the first builder's and the owner's call; the install
command is written for `lamp-update`'s layout, since signed updates are on
the ship list (#737). It does not install the keeper's unit or setup's yet
(below, "What the image must hold").

## What the image must hold

`install` puts lampOS on a lamp; it does not make the image.
`scripts/imager/build-orangepi-v2.sh` (`make build-v2`, #2538) makes one
that holds all of this and runs `install --setup on` into it, from one
signed build (the imager's README, "Lamp V2 image"). The build host authenticates
the exact manifest with the supplied public key before downloading or executing
its bootstrap updater. Both install attempts use that staged signed envelope,
and the recorded sequence and commit must match it; a moving remote channel
cannot silently select a second build. It expects to
find, from the third's list on #811 (2026-10-15 17:07) and from what it
reads itself:

- Debian bookworm (arm64) with systemd, `useradd`, `systemd-tmpfiles` and
  `date`; the groups `audio`, `video`, `dialout` (and `i2c`, `gpio` if the
  board has them).
- The update trust root, `/etc/lampOS/update/key.pub`, put there at the
  factory. Setup does not write it.
- For the optional Skills Store, its separate trust root,
  `/etc/lampOS/skills/store-key.pub`, provisioned by factory step 3 with
  `--store-key FILE`. Its absence does not block the rest of installation;
  the Store remains off and never borrows the update key. A build may ship
  the Store's key later, in `/var/lib/lampOS/skills/store-key.pub`, which
  wins over the factory's (#3692).
- No voice key: it is the user's (the owner on #806, 2026-10-16 16:13).
  Setup takes it from the account page or the app; on a lamp that ran V1,
  install copies V1's `llm_api_key` once (step 6). The factory writes none.
- The zone data in `/usr/share/zoneinfo`, and `/etc/timezone` or
  `/etc/localtime`.
- For setup (#806): nginx installed and not enabled, with the setup site;
  V1's built setup page in `/usr/share/nginx/html/setup`; V1's radio scripts
  `device-ap-mode` and `device-sta-mode` with hostapd and dnsmasq, as
  `scripts/imager/build-orangepi.sh` writes them; no
  `/var/lib/lampos-setup/done`.
- None of the old runtime's units running (`hal`, `os-server`,
  `led-boot`, `led-shutdown`): the voice refuses to start beside them
  (keeper.md).
- **Setup's settings**, `/etc/lampOS/setup/serve.json` and
  `/etc/lampOS/setup/reset-plan.json` (docs/setup.md, "On a lamp"). Written
  by `install --setup on` (below); without it they are not, and
  `lamp-setup.service` does not start (its conditions), so a lamp does not
  enter setup. The reset plan is the product's and never the person's
  (#806): at every install, with or without `--setup on`, a plan there that
  isn't the build's is replaced by it, the old one kept first as
  `/var/lib/lampos-update/reset-plan.json.before-<build>` unless it is the
  plan install recorded. A lamp with no plan gets none without `--setup on`
  (docs/updates.md, "Setup's reset plan").

## At the factory, once per lamp

Three things are this lamp's own and no image can carry them (#1682). They
are done once, in this order, before the lamp leaves, each with the lamp's
units stopped:

1. **The body's poses**, `/etc/lampOS/body/lamp.env` (the body does not
   start without it). With the body stopped and the lamp at rest, its arm
   folded and its head down as it lies when switched off:

   ```sh
   # as an unprivileged user in the servo port's group (dialout), never root
   install -d -m 0700 ~/measure-lock
   lamp-body-measure --device /dev/ttyACM0 --unit-id <this lamp> \
     --lock-directory ~/measure-lock --write ~/lamp.env
   # then root installs the file it wrote, never over one already there
   if sudo test -e /etc/lampOS/body/lamp.env; then
     echo "/etc/lampOS/body/lamp.env is already there: move it aside first"
     false
   else
     sudo install -o root -g root -m 0644 ~/lamp.env /etc/lampOS/body/lamp.env
   fi
   ```

   The root step refuses if `/etc/lampOS/body/lamp.env` is there, as the
   tool does for its own file: a lamp with poses set by hand keeps them
   (Fourteen's P3 on #3919). It looks as root, so a folder the person can't
   search still shows the file (Fourteen's P3 on #3932), and says why when
   it refuses (Sixth's), ending false so a script reads the refusal as one
   (Fourteen's P3 on #3940). To replace them, move the old file aside first.

   `lamp-body-measure` runs **unprivileged**. As root it refuses by design:
   a process with `CAP_SYS_ADMIN` could bypass the servo port's exclusive
   lock (`preflight: CAP_SYS_ADMIN bypasses tty exclusivity; use an
   unprivileged worker`). Its lock folder must be the running user's,
   mode 0700, with no group- or world-writable folder above it, so neither
   `/tmp` nor the body's own `/run/lampos-locks/motor` (owned by `lampos`)
   will do for another user. The product's factory unit,
   `deploy/units/lamp-body-measure.service`, runs it as the body's user for
   the same reason. On lamp-75c9 (28 Oct) the root run and a `/tmp` lock
   folder each refused with only "did not complete" until #3919 made it say
   why.

   It reads the five joints once at rest with torque off
   (`lamp-body-inspect`'s read-only plan: no register writes, no torque, no
   motion), prints the file and writes it, never over one already there. It
   holds `--home` and `--folded-arm` as found, `--stand` as lamp-4ace's
   with this lamp's roll and yaw, the default `--pitch-window` checked to
   lie inside the pitch servo's own limits, and `--torque on`. A rest pitch
   a little outside the body's home range is a head on its stop: `--home`
   takes the nearest edge and the file says so; one over it farther, within
   the body's wake reach, is a head moved off home by a hand, and `--home`
   keeps the high edge, said too; one farther out is refused, a head off its
   stop (the range and how far, as the code reads them:
   [body.md](body.md), on `lamp-body-measure`). The body starts from such a
   head by waking it home, as it does unless given `--wake off`. It refuses, with why, where the body would refuse the
   poses, and when any joint has torque on (the lamp is then held, not at
   rest). `--gaze` is not in it: it is measured with a face and the camera
   ([body.md](body.md)), and the body runs without it until then: the
   product unit's `--gaze-socket` and `--gaze-reach` without `--gaze` are
   gaze off, said in the body's first line (thirteen-P1; before it, the body
   refused them and never started on a factory `lamp.env`). Without
   `--write` it only prints, so a lamp can be read first and written later.
   Not a factory step yet: `lamp-body-measure sweep ... --seconds 25
   [--write /etc/lampOS/body/lamp.env]`, torque off, while a person moves
   each joint from end to end, records each joint's range of motion as an
   arc (across the 0/4095 seam where it crosses it) and can add it to that
   file as one comment line (#3504; [body.md](body.md)).
2. **The touch pads**, `/etc/lampOS/controls/controls-pins.json`, with a
   person at the lamp. GPIO head pads differ by unit: the hardware team's test
   passes when two or more of four candidate lines (37, 96, 97, 98) read a
   touch, and each lamp's pads are on its own lines: lamp-52e6's on 97 and
   98, driven high while touched (#1362), lamp-dbda's on 37, 96 and 97
   (#2182). Each unit's pads also rest
   and read differently (#1764): lamp-cb94's line 97 flipped at rest and read
   touched for 77 s with no hand on it, and lamp-c0a4's pulsed under a hand.
   So `lamp-controls discover-head --seconds 40 --rest-seconds 10`, hands
   away for its first 11 s and then each pad held about 2 s (or touched
   twice; on lamp-c0a4 a person's two touches merged into one long one,
   thirteen-P4), finds this
   unit's pads (two to four) and their polarity, which go into `head_pads`, and each pad's own
   baseline, which goes into `head_rest` (its rest, and whether a touch holds
   its level or pulses; controls.md, "Each lamp's own head pads"). A pad
   whose line changed with hands away is refused and nothing is written.
   Discovery also refuses missing sample coverage of 100 ms or more inside
   the hands-away phase, using its existing freshness bound, and requires
   at least one sample inside that phase. Only the portion of a gap inside
   the phase counts; delays wholly before or after it keep normal recovery.
   An invalid scan must be started again, rather than repaired by later
   touches. It exits unsuccessfully, so the existing step restores Controls
   and leaves the original pins profile untouched. This adds no wait to the
   40 s flow and changes no report fields or live touch settings. A partial
   report's zero `rest_changes` is an observed-transition count, not proof
   of a completed baseline; no-rest discovery and older-report compatibility
   remain as before.
   The step names each pad as it is found (its line, its rest, and whether
   it read level or pulses). The
   product's default reads all four lines, active-high, with no `head_rest`,
   until this step writes the unit's own (controls.md, "Each lamp's own head
   pads"). On an MPR121 strip it is
   `lamp-controls calibrate-touch` (controls.md, "Run without a lamp, then
   qualify on one": First's on-lamp sequence), its output installed there,
   but **that half is skipped while the base strip is off**: V2 doesn't use
   the strip, at the project lead's decision (19 Oct), and `lamp-controls`
   reads it only with `LAMP_CONTROLS_STRIP=on` ([controls.md](controls.md),
   "The base strip is off"); `deploy/factory/steps.sh` then skips it, reading
   the switch from `lamp-controls`' own environment, and finds the head pads
   as on any lamp. `install` keeps a pins file changed on the lamp.
3. **The public keys** (still named "the update key" in the step's output).
   The update key, `/etc/lampOS/update/key.pub`, is the
   public half of the owner's signing key ([updates.md](updates.md)).
   `install` refuses to run without it (step 1 below), and setup never
   writes it. The optional Store key has its own path,
   `/etc/lampOS/skills/store-key.pub`: pass `--store-key FILE` to provision
   it. Both are 32-byte Ed25519 public keys in base64, installed 0644 under
   root's 0755 directory. A missing Store key is skipped, leaving Store
   installs disabled; a supplied key that is malformed fails the step.
   The same existing key is kept, and a different one is never replaced.
   This step writes no Store switch and restarts no service. A lamp
   already shipped gets the Store's key, and its rotations, over the air
   instead: a build's `/var/lib/lampOS/skills/store-key.pub` wins over
   this one (#3692, [skills.md](skills.md), "From the Skills Store").

Then `lamp-selftest --occasion factory` ([selftest.md](selftest.md)), **on a
lamp set up from the app**. The self-test reads each part from its running
owner, and on a product lamp every one of them waits for setup's marker. So
on a lamp not set up the body never starts, and every part would be
`not_checked`, exit 3, which counts as done: a pass that checked nothing
(lamp-52e6 with the V2 image on 2026-10-26: step 4 timed out on lamp-body's
mood socket). The factory therefore runs steps 1 to 3 on the unset lamp, sets
it up from the app on the factory's Wi-Fi, and runs step 4 (`--from 4`).
A passed step 4 then asks for the reset. It first stops the lamp and waits
for every part: `lampos-setup-done.path` (with the marker still there it
would start the lamp again), then `lampos.target` with each of its parts
named (its `ConsistsOf`). Then it runs the reset as the button does (setup's
`reset-now` on the lamp's plan, in the unit `lamp-setup-wipe`), and the lamp
reboots into setup, unset. The stop waits because `systemctl stop
lampos.target` alone returns before the parts have stopped (systemd 252:
0.04 s, a part's write on stop 3.1 s later). On lamp-29e7 on 26 Oct, Voice's
onboarding, written as it stopped, outlived the wipe, so the customer got no
first five minutes. A lamp that cannot be stopped is not reset, and step 4
fails. The lamp is then removed from the factory
account in the app, and packed. A reset declined leaves step 4 failed, "NOT
READY TO PACK", so a lamp set up at the bench is never called done; the
keeper's pause, where the image has a keeper (the product image has none),
keeps it at rest meanwhile. The product reset plan
keeps what steps 1 to 3 wrote (`/etc/lampOS/body/lamp.env`, the pins file,
`/etc/lampOS/update`) and the self-test's record in `/var/lib/lamp-selftest`;
it wipes the person's data, the Wi-Fi, the credentials and the marker. A
written step 4 on a lamp not set up refuses before it starts anything, and
says so.

`deploy/factory/steps.sh --unit-id <this lamp> --update-key <key.pub> [--store-key <store-key.pub>]` runs
the four in this order, as root on the lamp. Without `--write` every step is
dry: it reads the body at rest into `/tmp`, says what it would write, and
writes nothing to `/etc`. With `--write`, step 1 also takes this arm's own
`--stand`: with a person's yes at the console and a hand by the arm, it runs
the body by hand on a private mood socket (no voice, idle, wake or glances),
tells it `listening` with `lamp-body-mood`, and asks it to stand at 30%,
then at 100%, and writes where the arm stopped at 100% in place of
lamp-4ace's. An arm that does not reach its full stand writes nothing.
Ctrl-C during a stand sends the body SIGTERM, which eases the arm back, and
stops the steps. Every body run passes `--let-go-at-stop on` (#1776), so the
lamp is let go once it is down and step 1 can be done again: a lamp that has
run `lamp-body` since power-on is otherwise held, torque on, and the rest
read refuses it. Found held, a written step 1 asks, lets it go with a body
run that only starts and stops (a head resting outside its start window is let go at once, torque off only, only by a run given `--folded-arm SHOULDER,ELBOW` with the arm found within 20 counts of it; the folded counts must come from a read the person confirmed was folded, never from this run's own read of where the arm is now; step 1 passes none, so there it is refused, nothing written, #1682), and reads it then; dry, it says so. Written, steps 2 and 4 also ask first; each is refused
without a console. On GPIO head pads the touch step runs `discover-head`
with `lamp-controls` stopped, and writes the two lines that each gave two
touches, with their polarity (pull-up for active-low, pull-down for
active-high, so a line with nothing on it reads as not touched), the rest of
the pins file kept; anything but two such lines writes nothing. On an MPR121
it calibrates. Either way `lamp-controls` is started again however the step
ends, and dry it only says what it would run, without stopping
`lamp-controls`. A listening voice that loses the privacy switch ends its
conversation on it and keeps the restart guard's mute
([controls.md](controls.md)), so the microphones stay closed, `lamp-ears`
with them, and the ring stops saying what it shows, even across a restart:
on lamp-52e6 step 4's self-test failed `room_mic` and `ring` on
`ears/level` and `light/state` as old as the step's stop, with
`record.json.restart-guard` left (#2818, after #3074's restart). So
`lamp-voice` and `lamp-ears`, those of them running, are stopped before
`lamp-controls` is ("stopped lamp-voice lamp-ears"), a stop the voice was
asked for, which releases the guard and keeps a person's own mute, and
started again once Controls is back, after the step, however it or the
script ends ("started lamp-voice lamp-ears again"); the self-test still
needs a fresh read of each, so a dead microphone or ring still fails. For a test with no person, `HEAD_LINES_FROM=FILE` passes
`discover-head --lines-from FILE`, the head lines read from FILE (the high
lines' numbers, or `none`), which a script writes to touch them (#2818). Its
probes are forced off, so its pads would all read active-high: with it the
live pins file is never written, under `ETC_ROOT` too, the found pads go to
`<pins>.test-input` beside it ("test input: live pins not written") and
`lamp-controls` is not restarted. Written, step 4 stands the lamp up for the
self-test's `head_move`, which checks only a lamp at rest and standing and
never stands one itself (#2232): after the yes (a hand by the arm as it
rises, not on it), `lamp-expression`, if it runs, is stopped: it asks the
body on `expression/body` all the time (its resting breath), the body keeps
only the newest request, and `head_move` wants nobody else asking for a
minute (lamp-52e6 on #2238: "asked to move 0 s ago by someone else"). So
61 s (`QUIET_WAIT`) are waited, and it is started again after the self-test
however the step or the script ends, Ctrl-C included. Then the running
`lamp-body` is told `listening` on its own
mood socket, `/run/lampos-body/mood.sock`, with `lamp-body-mood`, five times
0.2 s apart (an up voice tells it `resting` every 40 ms), and given 9 s
(`HEAD_STAND_WAIT`) to stand and rest again, standing, before the self-test
runs. It folds by itself 90 s after (`--awake-seconds`), however the steps
end. Step 1 needs `lamp-body` stopped and nothing after it starts it
again, so step 4 starts a `lamp-body` that is not active
(`systemctl start lamp-body`), whatever step the run began at, before the
stand, and waits up to 30 s (`BODY_START_WAIT`) for its mood socket; one
that does not come up fails step 4 and says to fix it first, since a
self-test without it leaves `head_move` unchecked (#2818). A passed step 4 then runs `lamp-keeper pause` (`/usr/local/bin/lamp-keeper`, [keeper.md](keeper.md), "Named states and the pause"): left as it is, the up voice wakes the lamp to anyone talking and an awake lamp stands, so on lamp-52e6, in a talking office, the arm never rested after the steps (#2818), and a factory is no quieter. Paused, every mood is rest and calls, looks and Expression's requests are dropped, so the body folds once its awake time runs out and holds there, and the voice takes no turn; the pause lives in `/run`, so the lamp's next start is not paused. A pause the keeper cannot say fails step 4 ("the arm is not left at rest"), and a failed self-test pauses nothing. Step 4's stand runs `lamp-keeper resume` first, when the keeper is there, so that a second pass in the same power-up can stand the lamp. If an executable keeper refuses `resume`, step 4 fails immediately after that command, before
starting the body, stopping Expression, sending a mood or running the self-test.
It cannot turn that refusal into a successful but unchecked head test. This
adds no wait to the successful path. A key already in place is kept when
it is the same and fails the step when it is not. A failed step stops
the rest and says which; `--from N` goes on from it. It runs nothing but
the lamp's own binaries and the shell. `deploy/factory/steps-test.sh` tests
it with no lamp; run it by hand after a change (the ARM64 workflow does not
run it).

## Setup at first boot, without the keeper

The keeper honours `--setup-marker` only when it runs as the old updater's
package (#816); a lamp installed by `install` has no keeper bring-up. So the
product does it with systemd alone (#811):

| Unit | Runs | Does |
|---|---|---|
| `lamp-setup.service` | at boot, `WantedBy=multi-user.target`, only while `/var/lib/lampos-setup/done` is missing and setup's two settings exist | the hotspot and V1's setup page until the home's Wi-Fi and the person's settings are saved; then writes the marker |
| `lampos-setup-done.path` | at boot, `WantedBy=multi-user.target` | starts `lampos.target` once the marker exists: the moment setup writes it, and at every later boot |
| `lamp-setup-reset.service` | in the target, after setup | the reset button held 5 s: stops the lamp, wipes, removes the marker, reboots into setup. Sandboxed (#3710): it writes only its subscriber's socket under `/run/lampos-bus` (`ProtectSystem=strict`, `ProtectHome`, `PrivateTmp`, `NoNewPrivileges`), and hands the reset to `lamp-setup-wipe` through `systemd-run` (the plan's `detach`), which runs outside it. So it needs a plan with `detach`, as every plan a build ships has (the install test checks it): a plan kept by hand without one, which only a keeper-installed lamp can have (`crates/keeper/src/profile.rs`, a hand-changed plan is kept), would reset inside this read-only unit, fail on its journal and restart without wiping; give such a plan the shipped `detach` |
| `lamp-waking.service` | early at boot, `WantedBy=sysinit.target`, only once the marker exists | the waking face until the voice asks for the ring (#1681, [ring-choreography.md](ring-choreography.md), "The waking face"); off unless `LAMP_WAKING_FACE=on`, and then it exits at once. Sandboxed (#3710): `ProtectSystem=strict` with nothing written but its runtime directory and the ring's `/dev/spidev3.0`, `ProtectHome`, `NoNewPrivileges`; no `PrivateTmp`, which would order it after the /tmp mounts and delay first light |

Setup, its reset and the waking face start the same binary, `/opt/lampOS/setup/current/lamp-setup`.
`install` writes and enables all four and starts none.

## The install command

```text
lamp-update install --channel-url URL [--channel NAME] [--key-file PATH] [--setup on|off]
```

As root, on a lamp with the base image (`crates/update/src/install.rs`):

1. Reads the channel's newest build and checks it with the image's key
   (`/etc/lampOS/update/key.pub` unless `--key-file` says otherwise), as a
   round does. No key, no install.
   After taking the updater lock, it preflights the product Voice,
   Expression, Controls and Body units it will install against the relevant
   local overrides it will preserve. This uses the updater's existing
   literal-mode rules: Voice's head reader, its `--controls-mute`,
   Expression's head reader and Controls' delivery must agree, with their
   enabled readers on the same bus, including Voice's enabled privacy
   reader and Body's enabled sleep reader. Body retains the product's
   `%H` and `$BODY_POSES` substitutions and its known calibration files
   (`/etc/lampOS/body/lamp.env`, and the generic unit's optional
   `/var/lib/lampos-body/measured.env`). A present calibration file must
   contain only comments, blank lines and literal single-line, double-quoted
   `BODY_POSES` assignments; other environment settings could override the
   sleep reader and are refused. An unmeasured lamp may still lack its
   required poses file, which keeps Body stopped until calibration.
   Conflicting or unknown modes, including relevant drop-ins or
   expansions that those rules cannot verify, refuse the install. This
   happens before reading or repairing state, making users, moving links,
   or writing units or head files. Local drop-ins and head files remain
   untouched on that refusal; benign unrelated drop-ins remain supported.
   The local reader opens without waiting for a FIFO writer and checks the
   type of that same open file before reading contents. Regular files and
   links to regular files remain supported. An actual null-device drop-in
   mask remains empty, checked by its character-device identity without
   reading the device; a filename alone does not grant that exception.
   FIFOs, directories and other nonregular inputs are refused. Missing
   files, including dangling links, keep their existing absence behavior.
   This covers the local units, drop-ins, known Body calibration and pins
   that the head-pair reader inspects; it does not add a deadline for
   regular-file storage or work in the live touch path.
2. Makes the users `lampos` (in `audio`, `video`, `dialout`, `i2c` and
   `gpio`, each only if the image has that group, and says which it has
   not) and `lampos-memory` (in `lampos`), unless they are there.
3. Checks that the build carries a binary for every product part, and the
   voice's `lamp-turn-end`, before writing anything.
4. Fills each part's folder, `/opt/lampOS/<part>/<sha256>/`, under a
   temporary name, each file hashed as it comes and compared with the
   manifest; renamed only when whole. A folder already there and holding
   exactly the build's files is kept. Only when every folder is whole does
   any `current` link move, each replaced whole by a new link renamed over
   it. Then it copies the build's `lamp-update` to
   `/usr/local/lib/lampos/lamp-update-back` (0755) if that file isn't already
   the same: lamp-update's fallback, which only install writes and no round
   can (updates.md, "lamp-update switching itself", #1680).
5. Writes each product unit, its reserved exit-note drop-in and
   `lampos.conf`, each only if it is not already exactly that. The drop-ins are
   `/etc/systemd/system/UNIT.d/exit-note.conf`, for exactly Cloud's fixed
   service list (including templates), using the existing root recorder.
   Other drop-ins are left alone. They record only bounded local stop notes;
   install grants no reporting consent, changes no reporting switch or
   service write permissions, and runs no recorder. Cloud's existing gates
   still control transmission ([cloud.md](cloud.md#crashes-the-exit-note)).
   Systemd waits for `ExecStopPost` before restarting the service. A failed
   stop adds a recorder process and its `systemctl show` query, capped at
   2 seconds; total restart overhead has not been measured on a lamp.
   A unit, drop-in, `lampos.conf` or setting found already
   holding install's exact bytes is given install's mode (0644; the voice's
   provider, its key and the Policy's key 0600, so a key is only ever
   narrowed), read and changed through one open of the file, never through
   a link: a link is left as it is (#1293). A lamp's own setting found
   closed to other users is kept as it is and said: `PATH is closed to
   other users (600): the parts run as lampos cannot read it`. The product settings (Expression's catalogue, the
   controls' two files, and the product skills, each in
   `/var/lib/lampOS/skills/<skill>/`: its `skill.toml`, `SKILL.md` and
   lights, not its tests) are written only where one is missing or is still
   exactly what an install wrote before, by the SHA-256 kept for it in
   `state.json`, so a newer build's default reaches a lamp that never
   changed it. A setting changed on the lamp since, or there before any
   install (lamp-4ace's own pins), is the lamp's: it is kept, and said
   (`kept /etc/lampOS/controls/controls-pins.json: …`). With `--setup on`
   (off unless set), setup's two settings are among them, exactly the setup
   crate's `fixtures/serve-own-page.json` (setup's own page on port 80, with
   the setup light on the ring) and
   `fixtures/reset-plan-product.json` (the reset stops `lampos.target`, there
   being no keeper). Which key setup takes and whether a reset wipes it are
   setup's (#806; the owner's 16:13 decision: setup takes the user's, a reset
   wipes it). With them and no marker the lamp starts setup at
   boot, so a lamp set up by hand, whose updater is upgraded by rerunning
   install, is never given them by default. Rerunning install is
   how the updater upgrades itself, so this holds at every run (the
   reviewer on #815). What it is about to write is recorded in
   `state.json` (`settings_writing`) before the first write, and what it
   wrote after the last, so a run stopped or failed in between leaves no
   file of install's that the next run takes for the lamp's. A file that is
   install's only by such a record is recorded as install's before the next
   run's own intent takes the record's place, so two failed runs in a row
   lose nothing either (the reviewer on #815, 21:42).
   With the units and settings written, install names every kept file that
   turns a product switch off, and changes none of them. A product switch
   is a variable a product unit sets `on` in its `[Service]`
   (`Environment=LAMP_CLOUD=on` in `lamp-cloud.service`), read from the units
   themselves, so a new switch is covered with no list to keep. It reads each
   product unit's kept drop-ins (`/etc/systemd/system/UNIT.d/*.conf`, all but
   `exit-note.conf`), with several assignments on one line, quotes and an
   empty `Environment=` (which clears the unit's own lines) read as systemd
   reads them, and every `EnvironmentFile=` the unit and its drop-ins name
   (store's `store.env` is lamp-cloud's), which systemd reads over
   `Environment=`. They are read together as systemd merges them: the
   drop-ins in name order, then the environment files (a file named twice
   counts once, at its last place). Only a switch still not `on` at the end
   is said, by the file that left it off, at every run, so a lamp
   from its builder days that moves to stable never comes up quietly off the
   cloud:
   `kept drop-in /etc/systemd/system/lamp-cloud.service.d/old.conf turns LAMP_CLOUD off ("off"); the product sets it on`,
   `edited setting /etc/lampOS/skills/store.env, read by lamp-cloud.service, turns LAMP_CLOUD off ("off"); the product sets it on`
   (a file that is not a setting is said as `environment file PATH`). A
   drop-in that sets the switch `on`, or sets other variables, is not said;
   a clean install says none. A path with a `%` specifier is not read.
   Remove the file, or set the switch back `on`, to take it back.
6. Writes `/etc/lampOS/zone.env` from the system's zone if it is not there,
   and again when it names another zone than the one the lamp was set up in
   (`/etc/timezone`): a lamp set up before setup wrote it kept the image's
   zone for schedule (#820). Only to a zone file in
   `/usr/share/zoneinfo`, as setup checks one: a wrong `/etc/timezone` keeps
   the file, and says so, since schedule would not start on it.
   Writes the voice's service, `/etc/lampOS/voice/provider.json`
   (`deploy/product/settings/voice-provider.json`: V1's route, the
   campaign-api proxy's Gemini Live path, `resumption` off because the proxy
   does not forward it). The voice's key, `/etc/lampOS/voice/credential`, is
   the user's (the owner on #806, 2026-10-16 16:13): setup takes it, and a
   reset wipes it. On a lamp that ran V1, install copies V1's `llm_api_key`
   from `/root/config/config.json`, but only into a lamp that never had one
   from install: a key install wrote that is now gone was removed on purpose
   (a reset), and V1's config, which may still hold the previous owner's, is
   never used to put it back (said instead). It writes the second route's
   services too, `/etc/lampOS/voice/second.json`
   (`deploy/product/settings/second.json`: the relay, every service on that
   same key; the lamps' voice since 19 Oct, #1712). It's kept and replaced as
   `provider.json` is, and a lamp's own (lamp-4ace's) is kept. All three files root's and 0600, as
   the voice requires, and kept up to date as the settings are: while each is
   still what an install wrote (its SHA-256 in `state.json`), a newer
   provider, or a key V1 rotated in its config, replaces it; one setup wrote,
   or changed on the lamp, is kept, and said. The key is never said; with
   none, install says the voice cannot reach its service until setup takes
   the user's. The same key is the Policy's judge key, `/etc/lampOS/policy/jev.key`
   (root's, 0600, handed in by systemd; the same rules), and
   the product Policy asks Jev through V1's route, the campaign-api proxy, as
   V1's Jev client does (#811 point 2).
   It never writes `/etc/lampOS/body/lamp.env`, and says that the body waits
   for it.
7. Records the build and the folders it made in
   `/var/lib/lampos-update/state.json`, as a round does, under the same
   lock, before systemd is asked anything. It reads them as a round does
   (`Records::state`): one that cannot be read stops install, never written
   over; a corrupt one is set aside as `state.json.corrupt-<ms>`, said, and
   begun again. Then what install wrote before is not known, so V1's key is
   not copied into a missing key file (said instead): a reset may have
   removed the user's, and V1's config may still hold the previous owner's
   (#1094's review). Every key path, missing or there, is then recorded as
   once written (`SET_ASIDE`, no hash a file can match). `Records::state`
   marks them as it sets the file aside, so whichever reader saves first, a
   round as much as install or prepare, keeps the mark, and later runs, and
   one after a later reset, keep to the rule for a key removed since it was
   written. A key there that is V1's gets its real hash; any other, and one
   setup writes later, is kept as the lamp's own. No `state.json` with a
   `state.json.corrupt-*` beside it (a run that could not save the mark) is
   taken as set aside too (Third and the reviewer on #1263). That systemd is owed
   (`systemd_owed`) is recorded earlier still, before the first unit,
   exit-note drop-in or `lampos.conf` is written (step 5), and after anything
   else changed.
8. If systemd is owed: `systemd-tmpfiles --create`, `systemctl
   daemon-reload`, and `systemctl enable` of every part, `lampos.target`,
   `lamp-update.service`, `lamp-records.timer`, `lamp-selftest.timer`,
   `lamp-update-back.timer`, `lampos-setup-done.path`, and the helpers'
   sockets `lamp-skills-manage.socket`, `lamp-setup-change.socket` and
   `lamp-setup-wifi.socket` (#2853; each template is started by its socket,
   never enabled; each timer's service is started by the timer
   alone; lamp-update-back's also by lamp-update's `OnFailure=`, #1680;
   `lamp-update-apply.service` is never enabled: only a round starts it,
   updates.md, "Settings and tmpfiles");
   then records the debt
   paid. It starts nothing and never writes setup's marker. A run whose
   enable fails, or that is stopped anywhere after its first unit is
   written, leaves the debt, and the next run pays it though it writes
   nothing (the reviewer on #815, 19:17 and 19:32).
9. **Harness at boot** (#811, the first builder's recipe, 01:56), once
   Harness is on the lamp at `/usr/local/bin/harness` (its own installer,
   run as `lampos`, puts it in `~/.local/bin`, then a link is made there; see
   "What was not shown"; without it, install says so and goes on). Unless an
   earlier install recorded it done (`/var/lib/lampos-update/harness-at-boot`,
   written only after `harness service install` succeeded; the CLI's own
   files are no proof, since it writes its unit before registering it and
   keeps it when a later step fails, the reviewer on #934): makes `/var/lib/lampos-tasks` `lampos`'s own (0700, the
   tasks unit's state folder); gives `lampos`'s own user manager that folder
   as its `HOME`, with a drop-in
   `/etc/systemd/system/user@<uid>.service.d/lamp-home.conf`
   (`Environment=HOME=/var/lib/lampos-tasks`), `systemctl daemon-reload`
   and `systemctl try-restart user@<uid>.service` (a manager already
   running takes it; one not running is left alone); runs `loginctl
   enable-linger lampos` (a user manager for `lampos` from boot, with no
   login, started with that `HOME`); then the Harness CLI's own `harness
   service install` as `lampos` with `HOME=/var/lib/lampos-tasks` and its
   `XDG_RUNTIME_DIR`.
   - **Why the drop-in** (#1676): `lampos` is created with no home
     (`/nonexistent`), and a user manager looks for user units under its
     `HOME`. Without the drop-in, Harness wrote its unit under
     `/var/lib/lampos-tasks/.config/systemd/user` and the manager answered
     "Unit file harnessd.service does not exist" (lamp-75c9, 19 Oct 14:52).
     With it, on lamp-75c9 at 15:13, the manager's `HOME` was the folder
     (PAM didn't set it back), and Harness's service installed and ran
     (`installed`, `registered`, `running`).
   - **Only that manager:** it runs nothing but Harness. The 13 other
     product units that run as `lampos` keep the `HOME` systemd gives them
     from the user database. Changing `lampos`'s passwd home would have
     moved all of them into the tasks part's folder (the first builder,
     14:58). No unit of
   install's own: the daemon's runtime, entry point and platform variable
   are the Harness team's (docs/harness.md). A refusal stops the install,
   records nothing, so the next install asks again, and is said; it is taken to the Harness team, not worked around. How a
   home lamp's Harness signs in is theirs too.

   **Put on by `lamp-update harness-on`** (as root, #1676 B5; the first
   builder and the lead, 19 Oct 10:41: Harness's own installer). On a lamp
   lampOS is installed on (it refuses without `/var/lib/lampos-update`),
   with no Harness at `/usr/local/bin/harness`, these steps, in order:
   `id -u lampos`; `/var/lib/lampos-tasks` made `lampos`'s own (0700);
   Harness's installer as `lampos` with `HOME=/var/lib/lampos-tasks`,
   `XDG_RUNTIME_DIR`, `PATH=/usr/local/bin:/usr/bin:/bin` and
   `SHELL=/bin/sh`: `curl -fsSL --proto =https --tlsv1.2
   https://cdn.autonomous.ai/harness/cli/install.sh | sh`, the address its
   header names. Then `test -x` of the launcher it writes,
   `/var/lib/lampos-tasks/.local/bin/harness`, then `ln -sfn` of it at
   `/usr/local/bin/harness`.
   - **What the installer does** (read from `cli/scripts/install.sh`, not
     ours): per user, with no root and no question. Node, tmux and the CLI
     go under `~/.harness`, each checked against Harness's sha256 metadata.
     The launcher is a two-line script that runs `~/.harness/cli/cli.js`,
     so Harness's own updates change `cli.js` and never the launcher that
     `lamp-tasks` checks (its executable identity). With `SHELL=/bin/sh` its
     one PATH line goes into `~/.profile` in that home.
   - **What it trusts** (the reviewer on #1777): a script fetched from
     `cdn.autonomous.ai` runs as `lampos`, the user every lampOS part runs
     as, and its sha256 checks come from the same CDN, so they guard against
     a bad download, not a bad CDN. That's the installer the first builder
     and the lead chose (19 Oct 10:41), as Harness's team publishes it.
   - **Then:** Harness at boot as above, with its record, unless already
     recorded; then `systemctl restart lamp-tasks.service`, only when
     something changed.
   - **Safe to repeat:** a Harness already at `/usr/local/bin/harness` is
     kept as it is when `harness version` (as `lampos`) says 0.4.23 or
     later. Run again, it changes nothing and restarts nothing.
   - **An older one it put there is brought up to date** (#1676, the first
     builder at 09:46 on 20 Oct): signing in by a spoken code (`harness
     login --code`, `lamp-tasks serve --sign-in-by-code`) needs Harness
     0.4.23. When `/usr/local/bin/harness` is `harness-on`'s own link to
     `/var/lib/lampos-tasks/.local/bin/harness` and says an older version,
     Harness's own installer is run again, the same steps as above, to the
     version it ships, and `harness version` is read before and after:
     "Harness updated from 0.4.19 to 0.4.24 by its own installer". The
     installer writes nothing it hasn't checked against Harness's sha256
     metadata. Harness's running daemon updates itself from then on (its
     installer's header); this is for a lamp where it hasn't run.
   - **`harness_too_old`, and the rest goes on,** when the installer failed,
     when it left a version still too old, or when an older Harness at
     `/usr/local/bin/harness` wasn't put there by `harness-on` (one put
     there by hand is never installed over). The Harness there is kept,
     and hand-overs go on with it; only signing in by voice waits.
   - **Each step is bounded** (`world::query`, group-safe): `systemctl`
     and Harness's service steps 120 s, Harness's installer, which fetches,
     15 min, `id -u` 5 s and `harness version` 30 s. One that runs past its
     bound is an error naming it and the bound. A `harness version` past
     30 s is not known rather than missing: the Harness there is kept as it
     is, never installed over, and it is asked again on the next run
     (`harness_on_keeps_a_harness_whose_version_cannot_be_read_in_time`).
   - **A failing step stops there,** before the link, so a lamp is never
     left with a link to nothing. Nothing is recorded, and the next run
     starts again.
   - **Signing in is the person's,** not done here: by a spoken code since
     Harness 0.4.23 (openharness#1410), with `lamp-tasks serve
     --sign-in-by-code` and the voice's `LAMP_TASKS_SIGN_IN`.
   - **Shown only in tests** (`crates/update/tests/install.rs`): the steps
     in their order through stand-ins; a Harness already there kept; each
     failing step leaving no link and no record; run again, nothing; its
     own older Harness updated, a failed or still too old install and one
     put there otherwise each kept and said `harness_too_old`
     (`harness_on_updates_its_own_older_harness_and_says_one_too_old`). On a
     lamp, only its "already there" path is shown: lamp-75c9, 19 Oct
     19:45 (Eighth, #1843), with Harness at `/usr/local/bin` after
     `harness-off`, it said "Harness is already at /usr/local/bin/harness:
     kept as it is", set it at boot again (record, drop-in, linger, the
     service running) and started `lamp-tasks` again. Installing on a lamp
     without Harness still needs a person's `sudo` on one.

   **Undone by `lamp-update harness-off`** (as root, #811; the first
   builder's yes on #705, 18 Oct): with that record, as `lampos` with the
   same `HOME` and `XDG_RUNTIME_DIR`, the Harness CLI's own `harness service
   uninstall`, then `harness stop`, since the uninstall starts the daemon
   again the usual way (`cli/src/lib/serviceCommand.ts`; the verbs read from
   Harness 0.4.17 on lamp-75c9, Nine on #705); then `harness service status
   --json` and `loginctl disable-linger lampos`; then the manager's `HOME`
   drop-in removed and `systemctl daemon-reload`; then the record goes, last,
   so a failure keeps it and a later run tries again. Without the record it
   changes nothing; without Harness at `/usr/local/bin/harness` it refuses
   and keeps the record. Harness itself, the sign-in under
   `/var/lib/lampos-tasks/.harness` (a reset's to wipe) and the `lampos`
   user stay. What Harness still has afterwards, its service or a `harnessd`
   running as `lampos` (`pgrep -u lampos -f harnessd`), is said with its pid
   and never stopped. An install afterwards sets it up again. The lock never
   waits: a second run during the first is refused ("another lamp-update is
   running"), and one that checked the record just before the first
   finished finds it gone under the lock and changes nothing. The linger
   goes off whether or not install turned it on: nothing else turns on
   `lampos`'s linger today; if something did, install would have to note it
   in the record for `harness-off` to leave it (not built). In tests
   (`crates/update/tests/install.rs`): the real steps, through stand-ins
   for `id`, `runuser … harness`, `loginctl` and `pgrep` that log each call
   and fail on demand, in their order; each failing step keeping the record
   for a later run; what the status and `pgrep` still find, said; and the
   status reading. **Shown on lamp-75c9** (19 Oct 19:45, Eighth, #1843,
   with `harness-off-check.sh` and `lamp-update` from lamp-voice
   `e3134347a`): before, the record, the drop-in, linger and the service
   `installed/registered/running`; after `harness-off` (exit 0, "Harness
   does not start at boot as lampos"), the record and the drop-in gone,
   linger off, the service `installed:false, registered:false,
   running:false`, and Harness, its link and `.harness` kept; run again,
   "nothing to undo". `harnessd` was gone too, though `harness-off` never
   stops it: linger off ended `lampos`'s user manager, which stopped it.

**An install that ends before its units says so** (#3733). Steps 1 to 4
(no build or a wrong one, the updater lock held by another lamp-update, a
refused head reader, a user that cannot be made, a part the build does not
carry or whose file does not match, a link or `lamp-update-back` that
cannot be written) end it with exit 1 and a last line
`lamp-update: units not written: <why>`, and so does a failure to record
the systemd debt just before the first unit. Every unit is then as it was,
which may be an older build's. A unit that fails within step 5 ends it
with `units not all written: <why>`: those before it are the build's, the
rest as they were, none half-written. A run killed by a signal says
nothing; its units are each whole, old or new, and `check-units` names
each that differs.

Run again with nothing new and nothing owed, it writes nothing and asks
nothing of systemd.
With a newer build, it installs the new folders beside the old and moves
the links; the old folders stay. It does not restart a running part:
switching a running lamp at a quiet moment is a round's work. So a changed
unit or setting takes effect when its part next starts, at a reboot or a
`systemctl restart` of that part: the product's `lamp-controls` gaining
`--touch` and the reset button (#1167) is one (Nine on #1167).

The product units and settings are carried inside the program
(`include_str!` of `deploy/product/`), so they are those of the commit the
program was built from; the binaries are the channel's newest build.
An install run by an older `lamp-update` therefore writes that build's
units around the newest build's binaries: any lamp-voice build up to 3505
(19 Oct, before #1815 at `fe277b92a`) writes the `lamp-cloud.service` that
is off unless a drop-in turns it on, the state lamp-c0a4 was found in on
27 Oct (#3733; which `lamp-update` ran there is not known). Since #3733's
hand-over, below, install does this itself.

**Install hands over to the build's own `lamp-update`** (#3733). Once
steps 1 to 4 are done, just before the units, it compares the sha256 of
the file it runs from (`/proc/self/exe`) with the build's `lamp-update` in
the manifest. When they differ, it says `handing over to build 7071's own
lamp-update, /opt/lampOS/update/<its sha256>/lamp-update, to write its
units` and runs that file, just placed in step 4, in its own place
(`exec`) with the same arguments and `LAMP_UPDATE_HANDED_OVER=1`. The
records' lock is let go at the `exec`, and the build's own `lamp-update`
installs again: the binaries and links are already there, so it writes
only what is missing, then the build's own units. Its own `changed`
lines are the ones printed at the end; the first run's are not repeated.
Between the `exec` letting the lock go and the build's own run taking it,
a timer's round can take it first. The handed-over install then ends with
`another lamp-update is at work` and `units not written`, changing
nothing: run `install` again once the round is done (rare, and safe).
A run with `LAMP_UPDATE_HANDED_OVER` set never hands over again: if the
channel moved to a newer build between the two runs, it says `this
lamp-update was handed this install already, so it writes its own units
over build <N>'s binaries`, then the warning below. A hand-over that
cannot happen says `could not hand over: <why>`, and install goes on with
its own units, as before #3733's hand-over, and says, e.g.
`lamp-update: this lamp-update (sha256 4f1c09a2be77) is not build 7071's
(9d3e5b10c4aa): the units written are this lamp-update's own, not build
7071's; run build 7071's own, /opt/lampOS/update/<its sha256>/lamp-update
install, to write its units` (the full sha256 in the path), and goes on: it
does not refuse, and writes the units as before. It names the build's own
folder, not `current`: on a lamp held below the build's updater, `current`
is the older lamp-update itself.
A running file that cannot be read says only `this lamp-update could not
be checked against build <N>'s: <why>`. The build's own says nothing, a
build that carries no `lamp-update` has nothing to check against and says
nothing, and an install that ends before its units says nothing of it.
None of these hands over: only a running file known to be another. Shown only in
tests (`crates/update/tests/install.rs`,
`a_lamp_update_not_the_builds_own_is_named_and_install_still_writes_the_units`
and the four beside it, the hand-over in
`another_lamp_update_hands_over_to_the_builds_own_before_any_unit`, and the
guard in `a_run_handed_over_to_already_hands_over_no_further`), not yet on
a lamp.

So a unit can meet a binary older than itself. `lamp-actions` and
`lamp-skills` exit 2 on an option they don't know, and with
`Restart=on-failure` they would loop, so their product units pass a new
setting as an `Environment=` line, which an older binary ignores: the
Bedtime and Morning stores come as `LAMP_SKILLS_DAILY_FILE` and
`LAMP_OUTPUT_CAPS_DIR` (#2157). `lamp-update`'s
`product_units_pass_only_options_an_older_binary_takes` holds both units'
`ExecStart` to the options their binaries took before; a new option joins
that list only once every channel carries a binary that takes it.

## A hand install's skill folders (`deploy/sync-skills.sh`)

A hand install swaps binaries but not skill folders, so a lamp installed
that way lacks every product skill added since its install: on 23 Oct
lamp-06c1 (build 1009) had no `startle`, and lamp-4ace, whose lamp-skills
runs from its own `--dir`, had no `reminders` where the voice reads it
(the first builder on #3126, 06:39). The product's update round carries
them; a hand install runs this as well:

```
# the list, nothing written; REV is the build the lamp's parts run
lampOS/deploy/sync-skills.sh --rev REV --lamp orangepi@lamp-XXXX.local
# then write them (sudo's password given at run time, never stored)
LAMP_SUDO=… lampOS/deploy/sync-skills.sh --apply --rev REV --lamp orangepi@lamp-XXXX.local
```

It writes each skill of `install::PRODUCT_SKILLS` at REV into
`/var/lib/lampOS/skills` as install does: every file but the skill's
`tests/` and hidden files, 0644 in 0755 folders, and the skills of
`install::PRODUCT_SKILLS_OFF` with `install::OFF_LINE` above their
`skill.toml`. A skill the build ships off (that line, or its own
`enabled = false`, as Bedtime and Morning) that the lamp has turned on
stays on, as a round keeps it (`product_skills::keep_turned_on`): when the
lamp's `skill.toml` turns it on, or lamp-skills' `on` (in its
`--state-dir`) names it and the lamp's file is missing or still the
build's, it is written as the build's file turned on. A file the lamp
turned off again (off, and not the build's) is the person's last choice
and wins over `on`: it is left as it is, and said (`left off:`). It
lists each file it adds or changes, and never deletes: a folder the build
doesn't name, a skill the person installed, is never read or written, and
a file in a product folder that the build lacks is left and listed. It
never writes lamp-skills' state. Each file goes in by a rename from a copy
unpacked beside the folder, so lamp-skills, which reloads the folder by
itself, never reads half a file, and a changed file keeps its owner.
`--dir` and `--state-dir` name other folders; without `--lamp` it runs
here, on the lamp's own shell. Its ssh is key only (`BatchMode`);
`LAMP_SSH="sshpass -e ssh"` with `SSHPASS` reaches a lamp by password. It
runs on a Mac's bash 3.2 too. It never carries an AppleDouble `._*` or a `.DS_Store`: a Mac's tar packs with `COPYFILE_DISABLE=1`, and `--apply` refuses to write either name, and says so; one already on the lamp is listed as safe to remove (23 Oct, seven written into lamp-06c1 from a Mac before this). Then restart the parts that read the folders at start
(lamp-voice, lamp-expression; lamp-skills reloads by itself) and check
their journals for `skill.toml: No such file`. On lamp-75c9 (installed by
`lamp-update install` of `ef1eb0cf2`), its dry run at that build found
all 47 files the same, and at `ae4ab0022` eight to change.
`deploy/sync-skills-test.sh` tests it with no lamp; run it by hand after a
change (the ARM64 workflow does not run it).

## Checking a lamp runs the product's units (`lamp-update check-units`)

After an install (Sunday's B8 on #806), whether the owner tests what
customers get: does the lamp run exactly `deploy/product/`'s units?

```sh
# on the lamp, as root, so no file it can't read is left out:
sudo systemctl cat 'lamp-*.service' 'lamp-*.timer' 'lampos*' > units.txt
# anywhere, with a lamp-update built from the commit installed:
lamp-update check-units --cat units.txt
# or against a checkout's deploy/product/ at another commit:
lamp-update check-units --cat units.txt --product lampOS/deploy/product
# with the image's own units compared too, for a pro lamp:
lamp-update check-units --cat units.txt --image robots/lamp --variant pro
```

- **Each product unit** is `same`, `missing on the lamp`, or `differs` with
  each differing line, `-` the product's and `+` the lamp's. One that
  differs and does not set a switch its product unit sets `on` says so
  after its lines, as an older build's base unit does (#3733, lamp-c0a4's
  19 Oct `lamp-cloud.service`, with no drop-in):
  `unit /etc/systemd/system/lamp-cloud.service leaves LAMP_CLOUD off; the product sets it on`. Install writes
  every unit whole into `/etc/systemd/system/`, with no `@…@` placeholder
  (`install::tests::no_product_unit_names_a_lamp`), so units are compared
  byte for byte, comments included: a changed comment is a file that isn't
  the product's.
- **Every drop-in** (a file under a `.d/` folder) is said with its text: on a
  product lamp each one is a finding. After the office extras go back, the
  check must name `office.conf` and nothing else. A drop-in of a product unit
  (or of a template's instance) that turns one of the unit's `on` switches
  off, read with the unit's other drop-ins in the order printed, says so
  after its text, as install does:
  `drop-in /etc/systemd/system/lamp-cloud.service.d/old.conf turns LAMP_CLOUD off ("off"); the product sets it on`.
  `systemctl cat` prints no `EnvironmentFile=`'s contents, so an edited
  `store.env` is named only by install.
- **Every lamp unit the product has no file for** is said (`not in the
  product`), and a product unit read from anywhere but
  `/etc/systemd/system/` (`not where install puts it`).
- **The image's own units** that updates carry (`lamp-cmedia-speaker.service`,
  `respeaker-lite-softvol.service`, #1369) are said as `the image's`, not
  `not in the product`. With `--image robots/lamp [--variant NAME]`
  (standard unless named, as the lamp's `/etc/autonomous/hardware-profile`
  says) each is compared with that variant's, and one that differs is a
  finding; without it, each is only said.
- **The settings writer's one-shot**, `lamp-update-writer.service`, is the
  updater's own: a round writes it to refresh the writer, and install never
  does (updates.md, "The writer's one-shot unit", #3689). It is said as
  `the updater's`, not `not in the product`, and compared with the text the
  checking lamp-update carries. One that differs, or one read from anywhere
  but `/etc/systemd/system/`, is a finding. A lamp that never refreshed its
  writer has none, and that is no finding.
- **A masked unit** (`# Unit NAME is masked.`) is a finding too.
- It ends with **0** when everything is `same` and nothing else is found,
  **1** when not, and **2** for a wrong option or a file it cannot read, so a
  script tells a bad input from a lamp that differs.
- `systemctl cat` puts `# PATH` before each file, on the first line or after
  an empty line. A line counts as one only there, only for a path not
  printed before, and only when it is `# `, an absolute path with no space
  and a unit's or drop-in's suffix. So a unit's own comment that begins with
  a path (`# /etc/lampOS/body/lamp.env as …`), or a unit that begins with its
  own path line (lamp-4ace's `lamp-skills.service`), is read as text and
  compared whole (the reviewer on #1346).
- `systemctl cat` itself **exits 1 here every time**: `'lampos*'` also
  matches the implicit `lampos.slice`, the two slices' parent, which has no
  file, and it says `No files found for lampos.slice.` on stderr. Save only
  stdout (`>`), and don't run it under `set -e`. Saved with `2>&1`, the line
  is read as that notice and nothing more.
- A glob matches only units systemd has loaded: a product unit on disk but
  neither enabled nor pulled in reads `missing on the lamp`. Every product
  unit hangs off `lampos.target` today, so a `missing` is a unit install
  didn't write or systemd never loaded (Twelve on #1346).
- **On lamp-75c9 on 17 October** (installed 16 Oct, read under the lock,
  checked against `lamp-voice` at the check's commit): 19 units the same; the
  two self-test units missing (added since); `lamp-controls`, `lamp-health`,
  `lamp-policy`, `lamp-tasks` and `lamp-voice` differing by what merged
  since (`--touch`, `LAMP_PERSON_NAME`, `--progress --push-wait`, the
  processors' rule, the key's comment); and `lamp-cmedia-speaker.service`,
  the image's, not in the product. Exit 1, as it should be.

## What was not shown

- Harness at boot (step 9) was run by hand on lamp-75c9 (the home image) on
  2026-10-16, with a throwaway system user with no login shell in place of
  `lampos`, then undone (#811): Harness's own per-user installer, run as that
  user with its `HOME`, needed no root (the image has tmux) and put Harness
  0.4.16 under `~/.harness` and `~/.local/bin/harness`; `/usr/local/bin/harness`
  a link to that; then step 9's `loginctl enable-linger` and `harness service
  install` exactly as `install` runs them, which succeeded in 3.9 s. After a
  reboot, with that user never logged in, `harnessd` was running 11 s after
  boot. So where Harness lives on a home lamp has one working answer: its
  own installer as `lampos` with `HOME=/var/lib/lampos-tasks`, and that link.
  `install` does not run the installer (the Harness team's, from their CDN,
  self-updating). Not shown: signing in or pairing as `lampos`, or tasks
  handing work over through it. Note for a reset: `~/.harness` holds Harness
  itself (`cli/`, `runtime/`, `bin/`) beside the person's sign-in and pairings.

- `install` was run only in tests, on a lamp made of files with a channel
  and a machine of the test's own (`crates/update/tests/install.rs`): a
  blank lamp gets every part, link, unit and setting, and nothing is
  started; a second run changes nothing, byte for byte, and asks nothing of
  the machine; a file that does not match its SHA-256 leaves no link and no
  unit; a build without a part is refused before anything is written; a
  newer build moves each link and keeps the old folders. `useradd`,
  `systemctl` and `systemd-tmpfiles` themselves were not run.
- No build carries the voice's turn-end model and ONNX Runtime library, the
  eyes' four models (YuNet and SFace; Face Mesh V2 and Blendshape V2, which
  only `--mouth-model` and `--seems-blendshapes` read, off in the product,
  #1671) or the name's models. `install` fetches each that is
  missing (#811, from lamp-06c1) from `deploy/product/models.json`: where
  it goes, its URL, size and SHA-256, the runtime's library out of its
  pinned release archive with `tar`. Each is written under a temporary name
  and renamed into place only when it matches; the voice's two go beside
  its binary through `current/`, and a round carries them to the next
  build. Every model, the name part's own `name.onnx` among them
  (`1d238ca7…`, the model lamp-4ace and lamp-52e6 run), is in one release of
  ours, `models-2026-10-16`, with each model's licence beside it (the first
  builder on #811, 2026-10-16 13:58; the fifth made it). The voice's four
  recorded lines and their manifest are in `notices-2026-10-20`, placed in
  `/opt/lampOS/voice/notices` (#1844, [failure-notices.md](failure-notices.md)). It is a draft until
  #811 ships, so until it is published every fetch says the model could not
  be had. One that cannot be had is said and the install goes on. What
  install placed is recorded by SHA-256 as the settings are, so when
  `models.json` moves a pin, the earlier model it placed is replaced (its
  part checks the new pin and would refuse the old); one already there that
  install never placed is the lamp's and kept. Not run
  against the release from a lamp.
- Nothing here was run on a lamp. The units were read from the repository,
  not from the lamps, and lamp-4ace's `LAMP.md` and its unit files already
  disagree on builds (updates.md, "Most likely wrong").
- Two lamps, one board. A third lamp may show another line as its own.
- The camera's USB node rule (`99-lamp-device.rules`, #2126) gives `video`
  write access to the ZVCAI camera's USB node, so lamp-eyes can reset a
  stuck camera. An update reloads udev's rules but triggers nothing, so the
  rule applies at the next boot or replug of the camera: until then each
  reset says `not reset: Io(13)` and only the 15 s retry applies.
- `lamp-cmedia-speaker.service` (the speaker's level, from V1) is the
  image's, not the product's: `robots/lamp/rootfs/etc/systemd/system/`, at
  step 9 (−28 dB), with the `pro` override at step 6 (−31 dB). udev starts it
  when the C-Media card appears (`99-lamp-device.rules`), and again each time
  the card comes back after a USB drop (`RemainAfterExit=no`), since the card
  returns at its own louder default. It sets the card's
  hardware level and creates `/etc/asound.conf`'s softvol "Speaker"
  control. `install` sets no level of its own. The product's voice unit is
  ordered after it, without wanting it (El on #705, 17 Oct). On lamp-06c1
  it was byte for byte the image's before #1370. Not run on a lamp: when the
  card comes back, the unit's `aplay` holds the speaker for a moment, so a
  voice reopening the speaker after its `Broken pipe` in that window may be
  refused once (#1370); watch for it in the lamp run.
  A lamp with a local drop-in that resets `ExecStart` (as lamp-4ace's
  `30-local-gain.conf` does) needs the same `-` on its `aplay` line.
- The journal's size is the image's too:
  `robots/lamp/rootfs/etc/systemd/journald.conf.d/zz-lamp-journal.conf`
  (#1178).
  - **What it sets:** at most 20M of journal in files of 4M, with 10M of the
    `/var/log` RAM disk always left free. The same for a journal kept in
    `/run`. It's the base image's own `SystemMaxUse=20M` (its `journald.conf`
    on lamp-75c9 and lamp-cb94), pinned in a drop-in so an edit of
    `journald.conf` doesn't undo it.
  - **Why:** lamp-4ace's had been set to 1G by hand. On 17 Oct its journal
    filled the RAM disk, and the voice's lines were lost until a rotate by
    hand. Its 171M were also copied whole by `orangepi-ramlog` at 00:00, the
    night the voice ended at 00:00:06.
  - **The RAM disk's size isn't ours:** it's `SIZE` in
    `/etc/default/orangepi-ramlog`, 50M on the image as installed and 400M
    where set by hand (lamp-4ace, a 375M `zram1`). The cap is the same
    either way, so a bigger disk only leaves more free.
  - **How long 20M lasts:** about three hours at lamp-4ace's rate on 17 Oct.
    A quieter lamp keeps more, but no check should count on a whole day of
    journal (`--since yesterday`).
  - **Read last:** "zz-" puts it after any other drop-in, so its numbers win.
  - **Tests:** `lamp-update`'s `tests/it/image_journal.rs` checks the numbers
    against the disk. Where `systemd-analyze` exists, it checks that systemd
    reads them last beside a 100M `persistent.conf`. lamp-75c9's systemd 252
    reads it last after its own `journald.conf` too.
  - **Existing lamps:** only a newly imaged lamp has it, unless a build
    carries the image's files and `LAMP_UPDATE_IMAGE=on` (#1369,
    updates.md "The image's own files"); `install` doesn't place them. On a
    lamp already in use it is otherwise copied there by hand, and journald
    reads it when it next starts, at the next boot (an update never
    restarts journald either). `journalctl --vacuum-size=20M` shrinks the journal at
    once, with no restart of anything.
- The night's copy-back of the logs leaves the journal alone:
  `robots/lamp/rootfs/etc/systemd/system/logrotate.service.d/zz-lamp-ramlog.conf`
  runs `usr/local/libexec/lamp-ramlog-postrotate` in place of the base's
  `orangepi-ramlog postrotate` (#1178).
  - **Why:** at 00:00 the base's `logrotate.service` copies the `/var/log`
    RAM disk to `/var/log.hdd` (`orangepi-ramlog write`), runs logrotate,
    then writes every file of `/var/log.hdd` back over its `/var/log` twin
    with `cat file > dest`. The journal is among them, so journald's open
    `system.journal` was overwritten in place with the copy taken seconds
    before. On lamp-4ace at 00:00 on 19 Oct, every read of it said
    "truncated, ignoring file" until a `journalctl --rotate` at 00:11, and
    the voice's lines in between were lost. That's read from the base's
    scripts (on lamp-75c9), not yet reproduced.
  - **What changes:** the same copy-back, with `/var/log.hdd/journal` left
    out. The rotated text logs still come back to the RAM disk. Since
    #2602 `write` no longer copies the journal to the eMMC (below), so
    there is none there to copy back. Off when
    `/etc/default/orangepi-ramlog` says `ENABLED` isn't `true`, as the base.
    `orangepi-truncate-logs` (over 75% of `/var/log`) runs logrotate without
    the service, so it never copies back.
  - **Tests:** `lamp-update`'s `tests/it/image_ramlog.rs` runs the script on a
    copy shaped as the lamp's (the journal kept, the text logs copied,
    compressed and numbered ones left out); a copy-back that names the
    journal fails it.
  - **Existing lamps:** only a newly imaged lamp has it (#1369), as for the
    journal's size above. On a lamp in use the two files are copied by hand,
    then `sudo systemctl daemon-reload`.
- ramlog's own copies leave the journal out:
  `robots/lamp/rootfs/etc/default/orangepi-ramlog` (#2602).
  - **Why:** at boot `orangepi-ramlog` copies `/var/log.hdd` (the eMMC)
    into the new `/var/log` RAM disk, and journald starts after it (the
    base's `systemd-journald.service.d/override.conf`), so its flush already
    comes after the copy. But the copy brought back earlier boots' journal
    files, some still marked open: journald said "Failed to read journal
    file … Device or resource busy" at +11.05 s on lamp-06c1 (22 Oct). They
    filled the disk past the 75% at which `orangepi-truncate-logs` (cron,
    every 15 min) runs `write` and `journalctl --vacuum-size=5M`, and at
    19:00 that vacuum dropped this boot's oldest file. The journal then had
    nothing before +11.0 s, though dmesg had lines from 3.8 s.
  - **What changes:** the base's own settings file, with its options for
    extra rsync arguments set. The boot's copy (`XTRA_RSYNC_FROM`) leaves
    out `/var/log.hdd/journal`'s files, but not the folder, so journald
    still keeps its journal in `/var/log`. Each copy to the eMMC
    (`XTRA_RSYNC_TO`: `write` at 00:00, at shutdown and from
    `orangepi-truncate-logs`) hides the journal, so it is never written to
    the eMMC, and its `--delete` removes the old copy there once. The
    journal lives in the RAM disk only, one boot at a time, within
    `zz-lamp-journal.conf`'s 20M; the last two boots are kept apart (the
    next item). `ENABLED`, `SIZE=50M` and `USE_RSYNC` are the base's. The
    night's copy-back then finds no journal to write over, even on a lamp
    without `zz-lamp-ramlog.conf`.
  - **Tests:** `lamp-update`'s `tests/it/image_ramlog.rs` checks the
    settings, and where GNU rsync exists runs the base's two rsync copies
    with them on copies shaped as the lamp's.
  - **Existing lamps:** a build carries it with the image's files
    (`LAMP_UPDATE_IMAGE=on`). It replaces only the base's file as shipped
    (`deploy/image/shipped.json` lists that one's SHA-256 too), so a lamp
    whose file was edited by hand (lamp-4ace, `SIZE=400M`) keeps its own
    and never gets this fix from an update. By hand: copy it over
    `/etc/default/orangepi-ramlog`, and on such a lamp set its own `SIZE`
    again in the copy. The next copy to the eMMC uses it, and the boot's
    copy at the next boot.
- The last two boots' journal, kept on the eMMC at a clean shutdown or
  reboot: `robots/lamp/rootfs/etc/systemd/system/orangepi-ramlog.service.d/zz-lamp-journal-keep.conf`
  and `/usr/local/libexec/lamp-journal-keep` (#2602, the first builder's
  21:43 call). So `journalctl -b -1` works after an update or a reboot.
  - **What changes:** the drop-in clears the base unit's `ExecStop=` and
    runs `lamp-journal-keep keep`, then the base's own
    `orangepi-ramlog stop`, which unmounts the RAM disk. Only when systemd
    says `stopping` (a shutdown or reboot, never a `systemctl stop`):
    journald rotates, and this boot's closed files are copied, newest
    first, to `/var/lib/lamp-journal/last`, up to 5 MB, stopping at the
    first that doesn't fit, so a long boot keeps its end without a gap
    (lamp-06c1's archived files were 1.1 to 1.4 MB an hour on 22 Oct: about
    the last 3 hours). The boot before moves to `previous`, the one before
    that is removed: 2 boots, 10 MB at most, written once a shutdown. The
    boot before is set aside (`.old`) and removed only once this one is in
    `last`, so a move that fails (the eMMC full or read-only) keeps the
    kept boots and writes a `lamp-journal-keep:` line to the journal. At
    boot (`ExecStartPost=`, `link`) each kept file is linked into
    `/run/log/journal` as `lamp-kept-<last|previous>-….journal`.
    journalctl (systemd 252) reads journal files at the top of
    `/run/log/journal` as well as its machine's folders, symlinks
    included. A crash runs nothing, so its boot isn't kept (accepted).
  - **Why this way:** the kept files never come back to the RAM disk,
    which is what filled it past `orangepi-truncate-logs`' 75% (above). Not
    in `/var/log.hdd` either, whose copy has `--delete`. journald's
    vacuum is of its machine's folder only, and a `journalctl
    --vacuum-size` removes only archived files, by name ("@"), which a
    link's name isn't. The 15-minute and midnight copies still leave the
    journal out: the only new eMMC write is this one at shutdown. Both
    steps start with "-", so neither may fail ramlog.
  - **Tests:** `tests/it/image_ramlog.rs` pins the drop-in, and runs the
    script: nothing kept unless `stopping`, 5 MB a boot newest first, 2
    boots and 10 MB at most, a boot with nothing closed keeps the kept, a
    failed move keeps them and says so, and the links.
  - **Existing lamps:** a build carries both with the image's files
    (`LAMP_UPDATE_IMAGE=on`), then `systemctl daemon-reload`: the next
    shutdown keeps, and the boot after it links. By hand: copy both, then
    `sudo systemctl daemon-reload`.
- Which group may read the GPIO chips and the I²C bus on the base image was
  not checked.
- `lamp-surround` was not read beyond its unit, and `lamp-records` has two
  unit texts (lamp-4ace's names `/opt/lampOS/records/`, the crate's
  `/opt/lampOS/bin/`), while `deploy/units/README.md` says it has no unit on
  any lamp.
- `lamp-skills` (#811, from lamp-06c1): installed with every product skill
  (since 19 October) and the voice's say service (`LAMP_SAY=on`, its socket
  in the group `lampos`), Welcome back the lamp's one hello (the voice's
  own removed, #2825), its reminders and task news waiting for their
  skills (`LAMP_SCHEDULE_SPEAK=skill`, `LAMP_TASK_NEWS=skill`), and
  `LAMP_SKILLS_ASK=skip` until the Policy's gate (#939). Only in tests: no
  lamp has run the product unit with them yet, and Reminders and Task news
  have not run as skills on any lamp.
- Bedtime and Morning (#1672): an installed skill that is disabled.
  Host installation tests check that only eight skills are active, the new
  skill is absent from the live catalog, and the Skills and Actions units
  declare their private durable paths without enabling any new switch.
  The directories' creation by systemd and the routine's behavior on a lamp
  still need the comparison recorded in [bedtime-morning.md](bedtime-morning.md).

## Prepare-to-unplug core skill (#1674)

Install includes `skills/prepare-unplug/{SKILL.md,skill.toml}`. It stays hidden
and inactive unless `LAMP_UNPLUG=on` is set on Skills, Body, Expression and
Voice after the lamp checks in [unplug.md](unplug.md). The product does not set
that switch yet. This skill prepares the physical arm; it is not OS shutdown.
