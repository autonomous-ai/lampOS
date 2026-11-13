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
| expression | `lamp-expression serve` | orangepi | none | a catalogue; the product's is `/etc/lampOS/expression/expression.json` (install's, then each build's settings) | the bus |
| actions | `lamp-actions` | orangepi, `audio` (product: lampos, `audio`) | `/run/lampos-actions/` (0755, kept across restarts); product: `/var/lib/lampos-output-caps/` (0700) | none | its folder; the bus |
| memory | `lamp-memory` | orangepi (`@MEMORY_USER@`: its own) | `/var/lib/lampos-memory/` (0700); `/run/lampos-memory/` | none | `memory.sock` |
| policy | `lamp-policy` | orangepi (template: `DynamicUser=yes`) | `/var/lib/lampos-policy/` (0700) | the judge's key, `/etc/lampOS/policy/jev.key`, as a systemd credential | the bus |
| health | `lamp-health serve` | orangepi | none | none: what it expects is options | the bus |
| controls | `lamp-controls serve` | orangepi | none | `controls.json` and `controls-pins.json`, in `/etc/lampOS/controls/` in the product (install's, then each build's settings) | the bus |
| ears | `lamp-ears` | orangepi, `audio` | none | none | the bus |
| schedule | `lamp-schedule serve` | orangepi | `/var/lib/lampos-schedule/` (0700); `/run/lampos-schedule/` (0700) | none | `call.sock` |
| air | `lamp-air` | orangepi, `/dev/i2c-0` | none | none | the bus |
| air's bus clock | `lamp-air limit-clock --bus-number 0` (a oneshot, before air and controls) | root | none | none | I2C bus 0's `device/freq` |
| tasks | `lamp-tasks serve` | orangepi | `/var/lib/lampos-tasks/` (0700); `/run/lampos-tasks/` (0700) | `targets.json` in its state folder (none at first: the product unit's `--set-up-later --targets-from-machines claude` writes it from the person's machines once Harness is signed in); the Harness program in the user's home | `request.sock`, `voice.sock` |
| mood | `lamp-mood serve --zone system` (off unless `LAMP_MOOD=on`, `mood.md`) | lampos | none | the lamp's zone, `/etc/localtime` | the bus (`mood/state`) |
| addons | `lamp-addons serve` (off unless `LAMP_ADDONS=on`, `addons.md`) | lampos | none | none | the bus (`addon/proposal` and the lamp's state in, `light/request` out) |
| services | `lamp-services --store /var/lib/lampos-services --run /run/lampos-services` (on in the product unit, `LAMP_SERVICES=on`, with writes, `LAMP_SERVICES_WRITE=on`, since 4 Nov; `services.md`) | root, group lampos, no capability | `/var/lib/lampos-services/` (0700, its `StateDirectory`; the reset plan wipes it); `/run/lampos-services/` (0750) | setup's `backend.json` and the voice's key, through `LoadCredential=`, for a token's refresh | `admin.sock` (`lamp-cloud` only), `tools/` with `tools.json` and `call.sock` (the voice only) |
| control | `lamp-control serve` (off unless `LAMP_CONTROL=on`, `control.md`) | lampos | `/var/lib/lampos-control/` (0700, its `StateDirectory`: certificate, key, paired computers); `/run/lampos-control/` (0700, the admin socket) | none | TCP 7731 (WebSocket over TLS, paired computers only); `admin.sock`; the body's `mood.sock`, the voice's `say.sock`, the bus |
| peek | `lamp-peek` (the owner's dev monitor, #5883: off unless `LAMP_PEEK=monitor`, which the app's `monitor.set` writes as a drop-in; outside `lampos.target`, `lamp-peek.service` is `WantedBy=multi-user.target` once setup's marker exists, `peek.md`) | lampos | `/run/lampos-peek/` (0700, the admin socket) | none | TCP 80 to the local network only, while on; `admin.sock`; reads the bus |
| cloud | `lamp-cloud --bus-root /run/lampos-bus` (on in the product, `LAMP_CLOUD=on` in its unit since #1815, `cloud.md`) | lampos | `/var/lib/lampos-cloud/` (0700, its `StateDirectory`) | setup's `backend.json`, the voice's key and update's `state.json`, through `LoadCredential=` | the company broker and the backend's ping; reads the bus |
| records | `lamp-records check` (a timer, hourly) | root | `/run/lamp-records/` | `/etc/lampOS/records.json`, which install places (`deploy/product/settings/records.json`; it does nothing without it) | `last.json` |
| selftest | `lamp-selftest --occasion first-boot` (a timer: 5 min after the lamp starts, then every 6 h; #812) | lampos, `CAP_SYSLOG` | `/var/lib/lamp-selftest/` (its own `StateDirectory`) | none | the bus (`lamp/selftest`), its runs' reports |
| update | `lamp-update serve` | root | `/var/lib/lampos-update/` (0700) | `/etc/lampOS/update/key.pub` | none |
| setup | `lamp-setup serve` (first boot only, until setup's marker) and `lamp-setup watch-reset` (the reset button, in the target); `docs/setup.md` | root | `/var/lib/lampos-setup/` (its `StateDirectory`; the marker `done`) | `/etc/lampOS/setup/serve.json` and `reset-plan.json` | setup's hotspot and page, where `serve.json`'s `listen` says; the bus (the reset's watch) |
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
[endpoint-detection.md](endpoint-detection.md)), the eyes' models, the
ears' YAMNet pair and the name's models, all from `deploy/product/models.json`, which install fetches
(a published build carries the same list as its manifest's models, which a
round fetches only where `LAMP_UPDATE_MODELS=on`; with it off, a lamp
whose install-placed file has another SHA-256 refuses the build,
[updates.md](updates.md)); the character file; and the voice's
private `provider.json` and `second.json`, which install writes and keeps
up to date while they are its own. Beside its binaries, a build's signed
manifest carries the rest of `deploy/product/` (the publish's `--deploy`):
the product units, the reset plan, and as settings Expression's
catalogue, the controls' two files, `records.json`, the tmpfiles conf
`/etc/tmpfiles.d/lampos.conf` and the Store's key
`/var/lib/lampOS/skills/store-key.pub`, which
`lamp-update-apply` writes at a round's switch (`LAMP_UPDATE_APPLY=on` in
the product; [updates.md](updates.md), "Settings and tmpfiles").

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
the keeper (which starts the target), update, surround, the records
timer, and the dev monitor's `lamp-peek.service` (#5883), which exits at
once unless the owner turned it on in the app. Stop is the reverse.

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
#4074). Their camera helper names `01da:5875` with no port too, which every
channel's helper takes since stable 8250 (#4077). The install test holds
both units to it.

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
equal, Laugh glance parked out of both): thirty-one folders are installed,
twenty-six enabled (Bedtime and Morning since #5753; Comfort break is off by its own file, the four of `PRODUCT_SKILLS_OFF` by install), one of them hidden unless `LAMP_UNPLUG=on`, which the product sets (#5747). Welcome back, Reminders, Task news, Onboarding, Stretch
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
enabled but stays hidden and inactive unless `LAMP_UNPLUG=on`, which the
product's Skills, Body, Expression and Voice units set since #5747.
Bedtime and Morning is included enabled since #5753, after its nights on
lamp-06c1 (#2803, #2804), with `LAMP_OUTPUT_CAPS=on` in the Actions, Skills
and voice units for its night ceilings; it adds no advertised tool. Watches
you leave (#1669) is on in the product since #2834, with
the eyes' `LAMP_ACTIVITY=place`, and Music sway (#1669) since #5770, with
the ears' sound classes (`LAMP_EARS_CLASSES=on` since #5748, YAMNet's files
placed by `models.json`, #6232). Watches you leave follows the sitter
by place only, never by a face print. With `room/activity` said, Stretch
break, already on, also waits while a head is down (reading, writing) and
lets that sitting's stretch go if they don't look up within its ten minutes.
Light show (#1679), a show of slow-changing colours a person asks for,
comes with the product since #5745, its `light_show` reached through
`use_skill`. Light scenes and Point the light (#1679) come with the
product since #5744, asked for by a person only and reached through
`use_skill` too. Four core skills waiting for their lamp checks are
installed disabled (`install::PRODUCT_SKILLS_OFF`, each with why): Light out
when you leave (#1677, [presence.md](presence.md), which must never take the
light of someone reading face down),
Smoky air and Dusty air
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
product lamp has 42 functions of its own: the voice's own 24 (with
`LAMP_REPORTS=on`, #2862, `reports_stop` and `reports_state` among them,
and `LAMP_MEMORY_ROLE=on`'s `set_role` and `end_role` since 9 Nov), the
actions part's 17 and `use_skill`, reaching Dance's `dance` and Pose's
`set_pose`, which comes with the product since #4135 (`PRODUCT_FUNCTIONS` in
`crates/update/tests/functions.rs`, which fails if one is left out or the
count changes). On the product's route a chat request takes 48
(`second_provider::MAX_TOOLS`), and each offers every one, with the
search's `web_search` and `fetch_page`
(`a_product_lamp_declares_every_function_it_has_within_the_bound`,
`a_product_chat_request_offers_dance_at_the_bound`). At the bound a core
skill's function is never the one cut (the owner's rule, on #2614): a chat
request leaves out the search's `fetch_page`, then `the_time` (asked only
for elsewhere), then the skills' functions that are not a core skill's,
the last first, then `web_search`; a Live session only the skills'
not a core skill's. Core skills say theirs in the skills' tools file's
`core` list (`lamp_skills::tools_file`). A Live
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
onboarding switch, Controls' privacy receipts (`LAMP_PRIVACY_CONTROLS=on`;
the product's voice and controls units set both switches), valid hardware profile and pending
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
fallback for one release; `LAMP_OUTPUT_CAPS=on` in the same units since
#5753, for Bedtime and Morning's night caps. Actions
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
  a step runs a tool that install does not put there. The voice's same-voice
  check `lamp-same-voice` ([same-voice.md](same-voice.md)) goes beside
  `lamp-voice` the same way, when the build carries it; it runs only when
  `LAMP_SAME_VOICE_MODEL` is set, which the product's `lamp-voice.service`
  sets since 4 November, for the linked services alone
  (`LAMP_SAME_VOICE_FOR=services`). A round fills
  a part's new folder from what is beside its binary now plus the build's
  files (`round::wants_of`), so a lamp whose voice folder has no
  `lamp-same-voice` yet gets it only from its next `lamp-update install`,
  not from a round.
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
- **The Store, prepared but off.** (The heading is kept for links: since
  2 Nov the product turns it on.) Install writes
  `/etc/lampOS/skills/store.env` with `LAMP_SKILLS_STORE=on`, read by both
  `lamp-cloud` and the skills manage helper. It also writes the helper's
  `/etc/systemd/system/lamp-skills-manage@.service.d/store.conf`, which reads
  that file and prepares IPv4 and IPv6 for fetching packages. These are
  defaults: a lamp's edits are kept on another install. They are install's
  `STORE_SETTINGS`. From updater 10 a round's settings carry `store.env`
  too, with the same rule: the build's is written over the one install
  wrote, and an edited one is kept (docs/updates.md, "The Store's switch
  file"). The drop-in is never carried by a round.
  The product cloud unit sets the Store on too; both parts require a valid
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
  lamp-ears (#1722 item 1). The local agent's socket folder `/run/lampos-agent`
  (0755, `lampos`'s) is made here too; the install step creates the group
  `lamp-agents` and the user `lampagent` in it when the lamp has none (a
  round carries the tmpfiles line but never makes a user or a group, so a
  lamp already in a home gets them from `lamp-update install` run again),
  and the owner's switch in the app only turns the door on (setup.md,
  `local_agent`). Both were found on lamp-06c1 on
  2026-10-16: with the locks 0775 the body never opened the servo port, and
  without the sessions folder the voice could not start.
  It also makes `/run/lampos-photos` (`lampos`'s, 0700), the app's photos
  for the eyes when `LAMP_PHOTO_FACES` is on (cloud.md), and
  `/run/lampos-eyes-kept` (`lampos`'s, 0700), where the eyes keep a sitting
  across a short restart (eyes.md, "A sitting kept across a restart"). And it keeps
  PulseAudio out of the board user's session (#4332): it masks
  `pulseaudio.socket` and `pulseaudio.service` for every user session
  (`/etc/systemd/user/`, links to `/dev/null`) and turns libpulse's
  `autospawn` off (`/etc/pulse/client.conf.d/90-lampos-no-autospawn.conf`).
  On lamp-06c1 on 30 Oct, the autologin's `pulseaudio` took the speaker
  while the voice restarted, and the voice's open failed busy. An update's
  restart that fails that way puts the build back. The lamp plays and
  records through ALSA only. A round carries this file
  (updates.md, "Settings and tmpfiles"). The masks take effect from the
  next boot, so a `pulseaudio` already running keeps going until then.
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

An install writes every product unit byte for byte; a round deliberately
keeps some installed text (#5061). While planning a new product build, it
names `lamp-update.service` as not carried by rounds: its product manifest
has no candidate text for that unit, so this is a policy notice, not a claim
that the file is stale. It also names a carried unit whose text differs only
in comments, blank lines or trailing whitespace when the plan leaves that
unit alone. These notices contain no unit contents, require no extra fetch
or systemd call, and do not repeat on polls of an already installed build.
`lamp-update check-units` remains the way to compare all installed unit text
with a specified product build.

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
- For setup (#806): nginx installed, not enabled and with no site, for
  setup's `free_port` to stop (setup serves its own page,
  `fixtures/serve-own-page.json`); V1's radio scripts `device-ap-mode` and
  `device-sta-mode` with hostapd and dnsmasq, as
  `scripts/imager/build-orangepi-v2.sh` writes them; no
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
starts setup again in place, unset, with no reboot (setup.md, the reset's
step 5; it reboots only if that fails). The stop waits because `systemctl stop
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
with `lamp-controls` stopped, and writes the lines that each gave two
touches or one long hold, two to four of them, with their polarity (pull-up for active-low, pull-down for
active-high, so a line with nothing on it reads as not touched), the rest of
the pins file kept; fewer than two such lines writes nothing. On an MPR121
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
| `lamp-setup.service` | at boot, `WantedBy=multi-user.target`, only while `/var/lib/lampos-setup/done` is missing and setup's two settings exist | the hotspot and setup's own page until the home's Wi-Fi and the person's settings are saved; then writes the marker, and starts each part `lampos.target` wants that is inactive, when the target already runs (setup.md, "The parts a target already up leaves behind") |
| `lampos-setup-done.path` | at boot, `WantedBy=multi-user.target` | starts `lampos.target` once the marker exists: the moment setup writes it, and at every later boot. A target already active pulls nothing in again; setup itself starts what it left behind (above) |
| `lamp-setup-reset.service` | in the target, after setup | the reset button held 5 s: stops the lamp, wipes, removes the marker, and starts setup again in place with no reboot (#4500; it reboots if that fails, setup.md). Sandboxed (#3710): it writes only its subscriber's socket under `/run/lampos-bus` (`ProtectSystem=strict`, `ProtectHome`, `PrivateTmp`, `NoNewPrivileges`), and hands the reset to `lamp-setup-wipe` through `systemd-run` (the plan's `detach`), which runs outside it. So it needs a plan with `detach`, as every plan a build ships has (the install test checks it): a plan kept by hand without one, which only a keeper-installed lamp can have (`crates/keeper/src/profile.rs`, a hand-changed plan is kept), would reset inside this read-only unit, fail on its journal and restart without wiping; give such a plan the shipped `detach` |
| `lamp-waking.service` | early at boot, `WantedBy=sysinit.target`, only once the marker exists, and with every start of the voice (`lamp-voice.service` wants it and starts after it, #2848) | the waking face until the voice asks for the ring (#1681, [ring-choreography.md](ring-choreography.md), "The waking face"); off unless `LAMP_WAKING_FACE=on`, and then it exits at once. Sandboxed (#3710): `ProtectSystem=strict` with nothing written but its runtime directory and the ring's `/dev/spidev3.0`, `ProtectHome`, `NoNewPrivileges`; no `PrivateTmp`, which would order it after the /tmp mounts and delay first light |

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
   the same: lamp-update's fallback, which no round writes itself
   (updates.md, "lamp-update switching itself", #1680). After install, only
   the writer's one-shot unit replaces it, from a confirmed lamp-update
   (updates.md, "The settings writer refreshed", #4178).
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
   **Name and time servers: China's on a lamp placed in China only**
   (#6256, #6414; `install::region_servers`, also `lamp-update region`). The
   region is the one `lamp-cloud` keeps from the ping's reply in
   `/var/lib/lampos-cloud/region` (`CN` or empty, [cloud.md](cloud.md)).
   - **`CN`.** In `/etc/resolvconf.conf` the first `name_servers*` line
     becomes `name_servers_append="223.5.5.5 119.29.29.29 1.1.1.1 8.8.8.8"`,
     any other goes, every other line is kept (one is added to a file with
     none), then `resolvconf -u`. openresolv puts `name_servers=` before the
     router's, and glibc asks the first of its three first and waits 5 s on
     one that is dropped, as 1.1.1.1 is in China; appended, the router's
     comes first, then AliDNS and DNSPod; in the hotspot, these three. And
     `/etc/systemd/timesyncd.conf.d/lampos.conf` is written, `[Time]` with
     `FallbackNTP=0.debian.pool.ntp.org 1.debian.pool.ntp.org ntp.aliyun.com
     ntp.tencent.com cn.pool.ntp.org`, then `systemctl try-restart
     systemd-timesyncd`: fallbacks only, Debian's pool first, so a lamp in
     China whose pool is dropped still sets its clock.
   - **Anything else, no file, or one that cannot be read.** The lamp's
     fallback stays the image's, `name_servers="1.1.1.1 8.8.8.8"`
     (`scripts/imager/build-orangepi-v2.sh`, which names no Chinese server;
     a test holds it so), and timesyncd's own. Nothing is written or run,
     except on a lamp no longer placed in China: the `CN` line goes back to
     the image's and the drop-in is removed, each followed by its refresh.
     A lamp outside China never sends a lookup its router misses, or a time
     query, to Alibaba or Tencent (the Reviewer-lead's privacy HOLD on #6386).

   A file already so is left and nothing run; a failed `resolvconf -u` or
   `try-restart` is said (the next lease lists the servers); a write that
   fails is said and the install goes on. `try-restart` leaves a timesyncd
   that is not running so, as on a lamp whose clock is chrony's. Install
   does this once; after it, `lamp-region.service` (root, oneshot, enabled)
   does it at every boot and, through `lamp-region.path` (`PathChanged=` on
   the region file), each time `lamp-cloud` writes a new region.
   `lamp-cloud` itself cannot: it runs as `lampos` with no capability and a
   read-only `/etc`. The one-shot runs with `ProtectSystem=strict`, `/etc`
   and `/run/resolvconf` its only writable paths (the files are written
   whole, a new one renamed into place, and `resolvconf -u` rewrites
   `/etc/resolv.conf`), `ProtectHome`, `PrivateTmp` and `NoNewPrivileges`.
   A round that brings the two units enables them; they run from the next
   boot.
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
   `lamp-update.service`, the four setup services that start outside the
   target (`lamp-setup.service`, `lamp-waking.service`,
   `lamp-setup-wifi-restore.service` and `lamp-setup-recover.service`,
   [setup.md](setup.md)), `lamp-records.timer`, `lamp-selftest.timer`,
   `lamp-update-back.timer`, `lampos-setup-done.path`, `lamp-region.service`
   and `lamp-region.path` (#6256, below), and the helpers'
   sockets `lamp-skills-manage.socket`, `lamp-setup-change.socket`,
   `lamp-setup-wifi.socket` and `lamp-eyes-camera.socket` (`install::enabled`;
   a test holds this list to it; #2853; each template is started by its socket,
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
   - **Only that manager:** it runs nothing but Harness. The 17 other
     product units that run as `lampos`, all but tasks, which sets its own, keep the `HOME` systemd gives them
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
     and Harness's service steps 120 s (`id -u` too), Harness's installer,
     which fetches, 15 min, and `harness version` 30 s, its `id -u` with it. One that runs past its
     bound is an error naming it and the bound. A `harness version` past
     30 s is not known rather than missing: the Harness there is kept as it
     is, never installed over, and it is asked again on the next run
     (`harness_on_keeps_a_harness_whose_version_cannot_be_read_in_time`).
   - **A failing step stops there,** before the link, so a lamp is never
     left with a link to nothing. Nothing is recorded, and the next run
     starts again.
   - **No targets yet is not a failure:** the product's `lamp-tasks.service`
     runs with `--set-up-later --targets-from-machines claude`, so the part
     starts with no `/var/lib/lampos-tasks/targets.json` (hand-overs off, its
     `tasks` diagnostic saying why) and, once Harness is signed in, writes
     that file from the person's machines with Claude Code as each target's
     engine (`docs/tasks.md`, "Before the lamp is set up for hand-overs").
     A `targets.json` written by hand is left as it is. Before this, on
     lamp-c0a4 on 3 Nov, the part failed at every start after `harness-on`
     (`No such file or directory`) until systemd stopped restarting it.
   - **Signing in is the person's,** not done here: by a spoken code since
     Harness 0.4.23 (openharness#1410), with `lamp-tasks serve
     --sign-in-by-code` and the voice's `LAMP_TASKS_SIGN_IN`, or from the
     owner's app (`--app-socket`, `LAMP_TASKS_SIGN_IN_APP` in the voice and
     `lamp-cloud`, #5198): all on in the product's units since 5 Nov.
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

   **Harness before the sign-in** (#5197; off unless
   `LAMP_HARNESS_AT_ONBOARDING=on` in `lamp-update.service`'s environment,
   `crates/update/src/harness_install.rs`; on a product lamp with it unset,
   on by the build's own default since 5 Nov, below). Put there with no hand, at the
   first onboarding or on a later update of a lamp without it:
   - **When:** after every `lamp-update serve --apply on` round whose
     channel was reached (not `unreachable`, nor a switch of lamp-update's
     own), so the first one once the onboarding has the lamp on Wi-Fi,
     and every later one, with or without a lampOS update due. Only while
     nothing is at `/usr/local/bin/harness`, its link followed: a link whose
     target is gone counts as missing and is put there again; one already
     there is recorded `installed` and nothing runs, unless it is older than
     the build's pin ("A newer pin moves an older Harness", below).
   - **How:** a round's own sandbox can't write `/usr/local/bin` or
     Harness's home, so, as with the settings writer, the round writes the
     fixed one-shot `lamp-update-harness.service` in `/etc/systemd/system`,
     checks what systemd loaded is that file alone, and starts it with
     `--no-block`. Neither the round nor the onboarding waits for Harness's
     download. The unit (`ProtectSystem=full`, `ReadWritePaths=/usr/local/bin
     -/usr/local/lib/lampos` and `TimeoutStartSec=3600` since #5978, which
     runs Hermes after Harness, never enabled) runs `lamp-update
     harness-install`: `harness-on`'s install steps above, then `harness
     version` and `systemctl restart lamp-tasks.service`. One install at a
     time (`harness.lock` in the records).
   - **Installed, not running:** no Harness at boot, no `harness service
     install`, no daemon. lamp-tasks starts the daemon after the person's
     sign-in by a spoken code, as it does today.
   - **Pinned:** with
     `LAMP_HARNESS_MANIFEST_URL` (https) and `LAMP_HARNESS_MANIFEST_SHA256`
     both set, the manifest is fetched (`curl`, 1 MB at most) and its
     sha256 compared first; another sha256 installs nothing
     (`pin_mismatch`). A match runs the installer with
     `HARNESS_METADATA_URL` at the checked bytes, served on 127.0.0.1
     (next), so a server that changes the manifest after that read isn't
     read again. The installer and its four other manifests are the
     build's own copies (next), with this pin too. With neither set the
     install is `harness-on`'s, said
     `pinned: false`, except on a product lamp (next); one alone is
     `pin_unreadable`.
   - **The build's own pin** (#5229, `harness_install/product.rs`): a round
     never rewrites `lamp-update.service`, so a pin or switch written there
     would reach only a lamp installed after it. Instead the pinned Harness
     manifest itself is compiled into lamp-update (`product::MANIFEST`, from
     `deploy/harness/cli-0.6.52.metadata.json`, a byte-for-byte copy of
     Harness's CDN manifest `https://storage.googleapis.com/s3-autonomous-upgrade-3/harness/cli/metadata.json`
     when it named 0.6.52; 0.5.98 or newer is needed for Hermes on the lamp, openharness#3807, #5978). The signed manifest names lamp-update by its
     SHA-256, so the build's Ed25519 signature covers the pin (#3693's
     decision (a): a sha256 pin in our signed manifest is enough, as Harness
     signs nothing). Nothing is fetched or hosted for it: the one-shot checks
     the bytes against `product::MANIFEST_SHA256` (`pin_mismatch` otherwise)
     and runs the installer with `HARNESS_METADATA_URL` at a listener of its
     own on `127.0.0.1` that serves those bytes while the installer runs
     (`harness_install/serve.rs`). The installer reads its manifest with
     Node's `fetch`, so it takes http but not `file://`. The installer checks
     each file the manifest names (`cli.js`, `notify.mjs`, at per-version
     CDN paths that are never republished) against its sha256. A fetched pin
     (`LAMP_HARNESS_MANIFEST_*`) is served the same way, so the installer
     never reads it again from its server.
     After the install, `harness
     version` is compared with the manifest's `cli.version` and journaled:
     `Harness 0.6.52 is the pinned version`, or `Harness says version …, not
     the pinned …`. Another version, or none, is recorded `failed` with
     `version`, not `installed`, so the app can tell it apart. Harness is
     still there and lamp-tasks is still started again. While Harness is
     there, later rounds keep that record and install nothing.
     - **The installer and its other manifests are pinned too.** Harness's
       installer reads four more manifests from its CDN, each with its own
       variable: the Node runtime (`HARNESS_RUNTIME_METADATA_URL`), tmux
       (`HARNESS_TMUX_METADATA_URL`), grid (`HARNESS_GRID_METADATA_URL`) and
       hn (`HARNESS_TUI_MANIFEST_URL`). Byte-for-byte copies of them and of
       the installer itself (`https://cdn.autonomous.ai/harness/cli/install.sh`)
       are under `deploy/harness/`, copied on 13 Nov 2026, and compiled in
       (`product::INSTALLER`, `product::MANIFESTS`), each with its
       SHA-256. Every pinned install, the build's own pin or the env pair,
       checks all five against their SHA-256s before anything runs
       (`pin_mismatch` otherwise). The four manifests are served from the
       same `127.0.0.1` listener, each at its own path, and each variable
       is set to its loopback URL. The installer is written as
       `harness-install.sh` in a new folder under the one-shot's private
       `/tmp`, owned by root (folder 0755, file 0644, so `lampos` can read it
       but not change it), checked by its SHA-256 again, run as `lampos` with
       `sh <file>`, and removed after. No pinned install fetches a script or
       a manifest from a server, apart from an env pin's own manifest. The
       installer's 15-min bound (`INSTALLER_WITHIN`) holds for this run too.
     - **OpenCode is never fetched unattended.** The installer downloads
       OpenCode (`curl https://opencode.ai/install | bash`, not pinned) only
       when its output is a terminal or `HARNESS_INSTALL_ATTENDED=1`. The
       one-shot captures the output, and a pinned run starts the installer
       with `env -u HARNESS_INSTALL_ATTENDED`, so that step never runs.
     - **A listener or file not made** (no listener on `127.0.0.1`, or the
       installer's file not written) is `serve`, not `installer`. It is
       tried again at the next round, with no 30-min wait.
     - **Without a pin** (a builder's lamp, `harness-on`), the installer is
       still the CDN's, piped to `sh`, as before.
     - **Which lamp:** a product lamp is one whose `lamp-update.service` is a
       regular file starting `/opt/lampOS/update/current/lamp-update`, that
       link naming a build. That is the same test a round makes before
       writing the product's settings. A builder's lamp on `deploy/units/`
       (lamp-update from a build's own folder) is not one, and keeps
       #5197's behaviour.
     - **What a product lamp does:** with no `LAMP_HARNESS_MANIFEST_*` set it
       uses the build's pin, so a home lamp never installs an unpinned
       Harness. The env pair still overrides it, for a builder's test. With
       `LAMP_HARNESS_AT_ONBOARDING` unset, it takes the build's
       `PRODUCT_DEFAULT`, journaled as `Harness before the sign-in: on by
       this build's default on a product lamp`. That default is on since 5
       Nov (#5197: the PM's call, once the app's sign-in screen is in a
       TestFlight build); the build that carried #5229 had it off. It
       reaches a lamp already in a home with this build's own lamp-update
       (`LAMP_UPDATE_SELF=on`), not with a unit. The environment, a drop-in
       included, always wins: `LAMP_HARNESS_AT_ONBOARDING=off` (or any value
       but `on`) turns it off, journaled `off` with no "by this build's
       default" line
       (`this_builds_default_is_on_for_a_product_lamp_and_the_environment_still_turns_it_off`).
     - **A new Harness version** is a PR that copies the CLI manifest, the
       installer and the four other manifests again under
       `deploy/harness/`, and updates `product.rs`'s `include_bytes!` paths
       and constants: `MANIFEST_SHA256`, `VERSION`, `INSTALLER_SHA256` and
       each `MANIFESTS` entry's `sha256`. Tests hold every constant to its
       file. CI opens that PR by itself (#5957 part 1,
       `.github/workflows/lampos-harness-pin.yml`): once a day (01:00 UTC,
       or by hand) it reads `product::ORIGIN`, and when its `cli.version` is
       a newer plain version than `product::VERSION` it copies the six
       files, checks every linux-arm64 file they name against its SHA-256
       (and size), updates `product.rs` and this section's version and date,
       runs `cargo test -p lamp-update --lib harness_install`, and installs
       that pin in an ARM64 container from the copies on `127.0.0.1`
       (`harness version` says the new version, `harness --help` runs). It
       then pushes the branch and opens one PR onto lamp-voice with a token
       of the pin GitHub App (secrets `LAMPOS_PIN_APP_ID` and
       `LAMPOS_PIN_APP_KEY`), so the PR's own CI starts by itself (a push or
       PR made with the workflow's own `GITHUB_TOKEN` starts none), and
       closes an older pin PR it opened. It never merges: a reader reads it,
       and it merges only after its row "Each Harness pin move" in
       `lamp-runs.md` passes (#5957 (b)). A dispatch with `proof: true`
       moves no pin: it only opens a draft PR from `ci/pin-proof-<run id>`
       with the same token, to show its checks start by themselves; a person
       closes it after.
     - **A newer pin moves an older Harness** (#5957; no switch of its own,
       it follows `LAMP_HARNESS_AT_ONBOARDING` and the build's default).
       After a reached round, Harness there and recorded `installed` with a
       `version` older than the pin's `cli.version` is installed again by
       the same one-shot, the same loopback files and the same hold, as
       `lampos`, with Harness's home (and so its sign-in) kept. Versions are
       compared as dotted numbers (`0.5.100` is newer than `0.5.39`); a
       version missing or not plain digits and dots is left alone, and an
       equal or newer one too. With the build's own pin the round compares
       the versions itself. An env pin's version is known only once fetched,
       so the round asks the one-shot when that pin differs from the one in
       `harness-ask.json`, and the one-shot installs only if the fetched
       `cli.version` is newer, else records `installed` as it was.
       - **Not while Harness is in use:** a round waits, saying one line
         (`Harness X is to be moved to the pinned Y: at a later round,
         …`), never an error, while `tasks/status`'s kept value lists a
         task in `tasks` or `unread` (one that can't be read counts as in
         use), or a Harness `login` (a sign-in by code), `new` or `reply`
         (a hand-over or follow-up being sent) is among the lamp's
         processes (`harness_install/busy.rs`; options before the verb,
         as in `harness --json login`, are passed over). lamp-tasks' store is
         private and never opened. The next idle round moves it.
       - **A task waits 6 hours at most (#6656):** a task left on a machine
         that is offline stays `unknown` and never ends, so a wait for
         `tasks/status` (a task worked on, or a status that can't be read)
         is bounded by `DEFER_CAP`, 6 h from the first round it held the
         move back. That time is kept in lamp-update's records as
         `harness-deferred.json` (`{"version":1,"since_unix_ms":N}`,
         0600). Past it, the round says once `Harness's move has waited N h
         for …, past its 6 h bound: it is moved now (#6656)` and asks the
         move. A sign-in, hand-over or follow-up running now still waits
         every round, past the bound too. The kept time goes once a round
         finds nothing in use, once the one-shot stops Harness for the
         move, or once a round finds Harness `installed` with no move due
         (the one-shot found it already at the pin and left it), so a later
         task starts its 6 h afresh. A move asked and still waited for
         (`installing`, or `failed` within its retry wait) keeps it. Before, a task on an
         offline laptop held a lamp on Harness 0.5.39 all evening, and
         neither the app nor the voice could clear it. Tests:
         `issue_6656_a_task_left_unknown_holds_the_move_back_six_hours_at_most`,
         `issue_6656_the_one_shot_moves_past_the_bound_and_lets_the_time_go`,
         `issue_6656_no_move_due_lets_a_kept_hold_go`.
       - **The ask:** `harness-ask.json` gains `upgrade_from` (the
         recorded version), so the one-shot moves the Harness that is there
         instead of saying "already at …, nothing installed". The record
         is `installing` with `from_version` and the words "Updating
         Harness on the lamp.". An ask or record without the new fields
         reads as before. The one-shot runs Harness's part whenever the
         ask is there, also when a round started it for Hermes (#6275:
         it ran Hermes's part alone, and the record stayed `installing`).
         Every end of the move records `installed` or `failed`, and then
         spends the ask: `upgrade_from` is taken out, the pin kept, so a
         later one-shot for Hermes moves nothing and a round asks again
         when due. Only another install holding `harness.lock` leaves
         `installing`, for that install to answer or for a round to ask
         again past 30 min.
       - **The move:** the one-shot reads `harness version` again (none:
         left alone, `installed`). That version, not the record's (Harness
         may have moved itself), is the `from_version` of the `installing`
         record and of any `failed` `upgrade`. It checks the pinned files as
         for a first install, then looks again whether Harness is in use,
         with the same 6 h bound for a task: if it is, it records `installed` at that version and a later round
         moves it. Then it stops `lamp-tasks.service` and runs `harness
         stop` (lamp-tasks starts the new daemon when it starts again).
         Either stop failing, or `id -u` failing first, is "not now" in the
         same way, with `systemctl start lamp-tasks.service` (a running one
         is left as it is). A bus that can't be opened counts as in use. Then it keeps the Harness that is there: as
         `lampos`, the folder of the `cli.js` its launcher runs, the
         launcher, `~/.harness/runtime/current-{node,tmux,grid}` and the
         `~/.local/bin/tmux` link are copied to
         `~/.harness/lampos-upgrade-from`. Runtimes sit in folders named by
         their version that the installer never removes, so these are what
         the older Harness needs. The CLI folder is checked by its real path
         (`pwd -P`, so `/./` or `//` change nothing): Harness's home,
         `.harness`, `.local`, `.local/bin`, the kept copy, or a folder
         above one of them is refused, here and again before a put-back, so
         neither can remove the sign-in. A kept copy already there (a
         put-back that failed) is the only good one: it is never removed,
         and the move is refused (`upgrade`, nothing stopped or installed,
         journaled `a kept copy waits at …`) until it is cleared by hand;
         this is looked at before anything is stopped. The new copy is made at `<kept>.new` and
         renamed only once whole. Once kept, `~/.harness/runtime/current-node`
         is taken away (#6302): Harness's installer reuses the Node it names,
         so a moved lamp would stay on its older Node (22) while a fresh
         install gets the pin's (24, which OpenClaw needs). Without it, the
         installer takes the Node the pinned runtime manifest names, served
         from loopback and checked by its sha256; a put-back restores the
         kept `current-node`. Then the installer runs.
       - **Put back on failure:** the installer failing, or `harness
         version` not the pinned one after it, puts the kept copy back (as
         `lampos`) and records `failed` with `upgrade` and `from_version`,
         "Harness couldn't be updated, so the lamp keeps the version it
         had."; it is journaled whether `harness version` says the older
         one again, and the kept copy is removed only when it does. A copy
         not kept installs nothing (also `upgrade`). Either
         way the launcher is pinned again (with the hold on) and lamp-tasks
         started again. A failed move is tried again by a round with
         another pin at once, or with the same pin after 24 h
         (`UPGRADE_RETRY_AFTER`). If the copy cannot be put back and
         Harness is then missing, the next round installs it as on a lamp
         without it.
       - **Shown in tests, and on lamp-c0a4 on 10 Nov** (the move to a newer pin, signed in and
         handing work over after: 0.5.62 to 0.5.95, and 0.5.95 to 0.6.2 with #5978's row).
     - **Held there:** Harness's own self-update is held by lamp-tasks'
       `LAMP_HARNESS_HOLD_UPDATES` (tasks.md, "Harness held at its pinned
       version"). A Team account ignores it. With that switch on in
       lamp-tasks' unit, `harness-install` also writes Harness's launcher
       in Harness's own pinned shape right after it installs, before it
       starts lamp-tasks again, so a Harness command run by hand holds the
       updates too (#5493; tasks.md, "The pinned launcher"). It runs as
       root in a folder Harness's user owns, so it writes and reads the
       launcher without following links, and leaves alone one with a
       second hard link or owned by another uid than its `bin` folder's
       owner (one line said; #5624), or over 64 KiB (Harness's is a few
       hundred bytes; not read past the cap, one line said; #5647).
     - **Not held on a dev or beta lamp (#6300, (B)):** a lamp whose
       `lamp-update.service` follows the lamp-voice feed (`--channel-url`
       exactly `…/releases/download/lampos-lamp-voice/`) is the canary for
       Harness's own releases: with the switch on, `harness-install` takes
       the hold's two lines out of the launcher instead of putting them in
       (`Harness's launcher unpinned: this lamp follows the lamp-voice
       feed, …`), and lamp-tasks writes no hold (tasks.md, "Dev and beta
       lamps: the hold lifted"). A stable lamp (`lampos-stable`, every
       product lamp), any other feed, and a lamp whose feed `systemctl show
       lamp-update.service --property=ExecStart` can't give keep the hold
       exactly as above. The pin itself is unchanged: a canary lamp still
       installs the build's pin, and a move to a newer pin moves only a
       Harness older than it (a self-updated one is left).
   - **Said to the app:** `harness-install.json` in the records,
     `{"install", "revision", "at_unix_ms", "version"?, "pinned"?,
     "error"?, "from_version"?, "message"}` (`lamp_msgs::HarnessInstall`), on
     `update/status` as `harness` and in lamp-cloud's `update.status`
     (`docs/cloud.md`). `install` is `absent` (switch on, not tried yet),
     `installing`, `installed` or `failed`; `error` a fixed code
     (`pin_unreadable`, `pin_fetch`, `pin_mismatch`, `timeout`,
     `installer`, `no_user`, `not_started`, `serve`, `version`,
     `upgrade`); `from_version` only with an `installing` that moves an
     older Harness, or a `failed` `upgrade` (#5957), absent otherwise;
     `message` fixed words for
     them; `revision` one more at each change. Said within 2 s of a
     change, with the switch on. Switch off, `serve` removes the record, so
     no `harness`, and the status thread wakes once a minute, as before.
   - **Tried again** after the next reached round when `failed`, or
     `installing` for over 30 min; the app's `update.check` starts that
     round at once. A failure the same steps would only repeat,
     `installer` or `no_user`, is tried again only by a round 30 min or
     more after it (`RETRY_AFTER`), so a broken installer isn't run after
     every round; an `update.check` within those 30 min starts no install.
     `serve` and the rest are tried at the next round. `version` is not
     held either, but nothing is tried while Harness is there. `upgrade`
     waits 24 h for the same pin, none for another.
   - **Shown only in tests** (`harness_install::tests`, the status and
     `app_contract` tests). Not run on a lamp: the one-shot's sandbox with
     the real installer, and a first onboarding with the switch on.
     `lamp-update.service` is never switched by a round, so the switch
     reaches a lamp by a drop-in or a new image.

   **Hermes on the lamp** (#5978;
   `crates/update/src/hermes_install.rs`; `LAMP_HERMES_ON_LAMP` in
   `lamp-update.service`'s environment: set, on only when `on` exactly, so
   any other value is off and `off` turns a product lamp off; unset, on on
   a lamp installed as the product, the build's default `PRODUCT_DEFAULT`
   being on (the product owner's decision, 10 Nov, under the lead's
   schedule below, V2 15:44 on #5978: it installs in the first round
   reached but for the setup walk, gives way while someone speaks and
   stops if the privacy reading is lost), and off on any other lamp; no product unit sets it). Hermes (NousResearch
   hermes-agent) is a coding agent Harness already knows as an engine.
   Harness finds an engine only by `command -v` in its user's login shell,
   so a `hermes` on `lampos`'s login `PATH` is all it needs: the wrapper
   `/usr/local/bin/hermes`. Nothing is done by hand on the lamp: a fresh
   install, an update from an older build and a try after a failure all
   take the same steps.
   - **Rides on Harness's install:** read only by a `serve` that acts with
     Harness before the sign-in on too. With that off, Hermes is off as
     well, journaled `Hermes on the lamp: off, as Harness before the
     sign-in is off`.
   - **When:** after each reached round, right after Harness's own step,
     while Hermes is not there as this build puts it: `hermes` in its
     folder, its checkout at the pinned commit (read from the checkout's
     `.git/HEAD`), and the wrapper and the namespace script holding this
     build's text. One of them missing or different is Hermes missing. The
     round writes `hermes-ask.json` in the records. With Harness there, it
     records `installing` and starts `lamp-update-harness.service` itself,
     without waiting. With Harness missing it starts nothing: the one-shot
     Harness's step started installs Hermes right after Harness, in the
     same run, and the record stays `absent` until then. Only in its
     window, below; outside it the round writes no ask, takes back an
     install ask an earlier round left (an ask to remove is kept), and
     journals `Hermes 0.22.2 is not set up yet: it waits, <why>`.
   - **The window** (`crates/update/src/hermes_schedule.rs`, the lead's
     rule, V2 15:44 on #5978, as V1 did): any reached round starts it (or
     asks Harness's one-shot for it), the first round after lamp-update
     started too, with no quiet stretch, no night window and no wait for
     the walk to end, whatever the voice is doing; Hermes's install rides
     after Harness's in Harness's one-shot, at the build's Harness pin.
     The one exception is the setup walk itself: while it is under way
     (the voice's onboarding file, as the round's own walk hold reads it,
     `crates/update/src/walk.rs`) the round waits.
     Harness's own install is not held by it. **The app's tap** (#6240,
     `LAMP_APP_ENGINES=on` in lamp-update's and lamp-cloud's units,
     `crates/update/src/engines.rs`; docs/cloud.md, "The lamp's agents"):
     the app shows Hermes's state (`off`, `not_installed`, `waiting`,
     `installing`, `frozen`, `installed`, `failed`) and, for one missing
     or failed, `engines.install` puts one `update/engine_install` on the
     bus. `serve` marks it asked and starts its next round; that round's
     step opens `Window::Asked`, which still waits out the setup walk, and
     a recent failure does not hold it. Everything below (the floors, the
     freeze, the privacy stop) is the same. lamp-update's sandbox reads
     all of these: `ProtectSystem=strict` keeps `/var/lib` and `/etc`
     readable, and `/run/lampos-bus` is among its `ReadWritePaths`. The
     voice is heard by a thread of `serve` that listens from its start
     (subscribed as `update_hermes`), by the round's own quiet gate
     (`voice/state`, `voice/heard`, `voice/said`, `voice/mute`,
     `voice/privacy`, `crates/update/src/quiet.rs`).
   - **Gives way while it runs** (the same thread, every 100 ms, while the
     record says `installing` and systemd says the one-shot is
     `activating`):
     - the voice not quiet (someone heard or answered, the voice not
       resting, or not seen): the unit's own `cgroup.freeze` (systemd 252 refuses `systemctl freeze` for a oneshot still activating, "Unit has a pending job")
       journaled `Hermes's
       install is frozen while the voice is not quiet: <why>`; thawed with
       `cgroup.freeze` back to 0 once the voice has rested 5 s, nobody
       heard (`RESTED`, the quiet gate's rest between turns), with no
       quiet stretch to rebuild. Frozen 40 min in all (`MOST_FROZEN`,
       raised from 30 min so a busy office still finishes), over one
       freeze or several, it is stopped as `interrupted` and tried again
       in the next round, so with a stage's own 15-min bound it ends
       inside the unit's hour (`TimeoutStartSec` counts on while
       frozen). A freeze or thaw systemd refuses stops it too, so the
       one-shot is never left frozen; and a `serve` that starts thaws it
       first, in case one ended mid-freeze;
     - the privacy reading lost: once a fresh lease with the slide read
       (or a lamp with no slide) was seen on `voice/privacy`, its newest
       lease older than 250 ms (`LOST_AFTER`, its life being 50 ms), one
       that no longer reads the slide, or one from another of the voice's
       boots (it ended and started again): the one-shot is thawed and
       stopped at once (`systemctl stop`), and the record written `failed`
       with `privacy_lost`. A lamp whose voice never said a reading is not
       judged by it. Looked at every 100 ms, a lapse shorter than that
       between two looks can be missed; a restart of the voice by an
       update round during the install stops it too. When no such lease
       was seen in the install's first second (`NO_LEASE_SAID_AFTER`),
       `serve` says once "Hermes's install runs with no privacy watch", so
       a bus permission or a renamed topic is not silent. The stop takes
       the install away, but does not by itself give the person the
       microphones back: a voice that falls into its restart guard after
       losing the reading stays muted until #6124's fix lands.
     A stopped install's ask is taken away with it (the one-shot is killed
     before it could); what it left in `/var/lib/lampos-hermes` is taken up
     by the next try, as a cut-off try's is, and the entry points are
     written only as an install ends.
   - **The one-shot:** `lamp-update harness-install` runs Harness's install
     whenever `harness-ask.json` is there (#6275): a missing Harness is
     installed, one there is moved when the ask carries `upgrade_from`,
     else recorded `installed` with its version (a `failed` `version` or
     `upgrade` kept); with no Harness ask (its switch off), Harness's
     record is left as it is. Then Hermes's when the ask is there, whatever
     Harness's install did. The unit's result is Harness's alone: a Hermes
     failure is journaled and recorded, never the unit's, and never stops
     Harness or the round. The ask is removed once answered. One Hermes
     install at a time (`hermes.lock`).
   - **As `lampos`, Harness's own user,** so Harness's hooks in Hermes's
     `config.yaml` and its reading of Hermes's sessions work. No other
     user, no group, no sudo. Hermes's folder is `/var/lib/lampos-hermes`,
     made `lampos:lampos` 0700 by `install -d` before the installer runs;
     Hermes's own home (`HERMES_HOME`) is `/var/lib/lampos-hermes/.hermes`,
     the folder lamp-tasks and Harness's hold file name. Every step of the
     installer runs as `runuser -u lampos -- env HOME=/var/lib/lampos-hermes
     HERMES_HOME=/var/lib/lampos-hermes/.hermes PATH=/usr/bin:/bin
     SHELL=/bin/sh`, so every file it writes, `config.yaml` among them, is
     `lampos`'s. Its checkout (`.hermes/hermes-agent`), its `uv`, its
     Python and its dependencies are all under the folder. The lamp has no
     Python: the installer's `venv` stage fetches its own pinned `uv`
     (checked by its SHA-256 in the checkout's `pm/lock.json`) and a
     uv-managed Python, and its package manager installs the dependencies
     from the checkout's hash-locked `uv.lock`. `hermes` is published at
     `/var/lib/lampos-hermes/.local/bin/hermes`, on no `PATH`.
     The build's own **lampos SDK** is written beside it, once for every
     agent (#6507): `sdk_tree` embeds `sdk/python/src/lampos` and the
     pure-Python `websockets` (`deploy/sdk-runtime/`, 15.0.1) in
     lamp-update, and the same one-shot that writes Hermes's entry points
     writes them to `/usr/local/lib/lampos/sdk-python` (root's, files 0644:
     the namespace leaves `/usr` as it is, so a run reads it and cannot change
     it). The folder holding anything but exactly this build's files counts
     as Hermes missing, so a newer build has it written again; a skill's run
     has it on `PYTHONPATH` (`docs/tasks.md`). No fetch at run time. No unit, no
     service, no gateway, nothing at boot: Harness starts `hermes` in a
     pane only when a task is sent to it.
   - **In a namespace that shows Hermes only its own folders.** Hermes
     runs shell commands the model chooses. As plain `lampos` they could
     reach every lamp service run as `lampos`: lamp-cloud's relay key (its
     systemd credential under `/run/credentials`, and lamp-cloud's state
     folder), the eyes' `/run/lampos-eyes/permit.sock` (it turns the
     camera on) and `/var/lib/lampos-eyes/people.json` (the face prints),
     the body's motors under `/run/lampos-body`, the person's asks in
     `/var/lib/lampos-tasks/store`, and, through `lampos`'s groups
     (`video`, `audio`, `dialout`, `i2c`), the camera, mic and serial
     ports themselves. So Hermes is never run but through two root-owned
     scripts, 0755, each written whole beside and renamed in, never
     through a link:
     - the wrapper `/usr/local/bin/hermes` reads lamp-cloud's local
       forwarder token, `/var/lib/lampos-cloud/hermes-token` (lamp-cloud
       makes it, 0600, with `LAMP_HERMES_ON_LAMP=on` in its unit). Missing
       or empty, it says so and ends with 1, running nothing. Else it
       exports `LAMP_HERMES_TOKEN` (the variable Hermes's `providers.lamp`
       reads its key from, as lamp-tasks writes it) and `HERMES_HOME`, and
       runs `unshare --map-root-user --mount --pid --kill-child
       --mount-proc --propagation private /usr/local/lib/lampos/hermes-ns`:
       a PID namespace of its own with its own `/proc`, the wrapper its
       parent (`--kill-child` forks, and killing the wrapper kills Hermes).
       Hermes shares `lampos`'s uid with lamp-cloud, so with the lamp's
       `/proc` it could open lamp-cloud's `/proc/<pid>/root` (lamp-cloud's
       own mount view, the credentials there), `environ` and `fd`; in its
       own PID namespace Hermes sees only its own processes, and no lamp
       process is there to open;
     - the namespace script `/usr/local/lib/lampos/hermes-ns` refuses to
       run in the lamp's own user namespace (its `uid_map` the whole
       range). Then, by an allow-list (`ALLOWED`), as root of that first
       namespace:
       1. an empty writable tmpfs of Hermes's own on `/tmp` (256 MB),
          `/var/tmp` and `/dev/shm` (64 MB each), which are shared on the
          lamp;
       2. what stays visible staged on that new `/tmp`: Hermes's folder
          `/var/lib/lampos-hermes`, its work folder
          `/var/lib/lampos-tasks/harnesses/lamp-tasks` and the skills
          `/var/lib/lampOS/skills` bound there (the last two only when
          there), and the file `/etc/resolv.conf` resolves to when it is
          under `/run` (such as `/run/systemd/resolve/stub-resolv.conf`),
          so names still resolve for the web and the forwarder stays on
          TCP `127.0.0.1:7741`. The work folder is Harness's project
          `~/harnesses/lamp-tasks` in its home, not only its `lamp`
          subfolder (where lamp-tasks puts `AGENTS.md` and Harness starts
          Hermes), as Harness names the project and may start a task in
          another folder of it; nothing else of lamp-tasks' state folder
          (its `store`, Harness's own files) is shown;
       3. an empty tmpfs over `/run`, `/var/lib` and `/etc/lampOS` (when
          there), the mount points made in it and the staged mounts moved
          back; the skills and `resolv.conf` remounted read-only (with the
          flags they had, as a user namespace may only add flags); then
          the three overlays remounted read-only;
       4. `/dev/null` bound over every character device matching
          `/dev/video*`, `/dev/media*`, `/dev/v4l-subdev*`, `/dev/snd/*`,
          `/dev/ttyACM*`, `/dev/ttyUSB*`, `/dev/ttyS*`, `/dev/ttyAMA*`,
          `/dev/i2c-*`, `/dev/spidev*`, `/dev/gpiochip*`, `/dev/hidraw*`,
          `/dev/fb*` and `/dev/dri/*` (a missing one is fine): under
          `--map-root-user` `setgroups` is denied, so `lampos`'s groups
          can't be dropped, and the devices are hidden instead;
       5. back into the folder it was started in through the new mounts
          (the old one would reach the hidden folders by `..`); a folder
          not shown starts it in Hermes's own folder instead, with one
          line on stderr (`hermes-ns: <folder> is not visible to Hermes;
          it starts in <Hermes's folder>`), and only when that too fails
          does it end, with no raw `cd` error. A task started from such a
          folder (a lamp-tasks hand-over included) then works in Hermes's
          own folder, not the one it was started from: that stderr line,
          in the pane or the journal, says so;
       6. `HOME` set to `/var/lib/lampos-hermes` and `unshare --user
          --map-user=<caller's uid> --map-group=<caller's gid>
          /var/lib/lampos-hermes/.local/bin/hermes`.
       In that nested user namespace the mounts are locked: Hermes cannot
       take them off. Any step failing ends it before Hermes runs.
     So Hermes sees its own folder, its work folder and the skills
     (read-only), `resolv.conf`, its own `/tmp` and `/proc`, and the
     lamp's programs (`/usr`, `/opt/lampOS`); no lamp service's `/run` or
     `/var/lib` folder (no socket, no state, no credential), no
     `/etc/lampOS`, no camera, mic, serial, I2C, SPI or GPIO device and no
     lamp process. A test (`no_lampos_service_folder_is_visible_to_hermes`)
     reads every `RuntimeDirectory=`, `StateDirectory=` and
     `ReadWritePaths=` under `/run` or `/var/lib` of the product units run
     as `lampos`, and the tmpfiles folders made `lampos`'s, and fails if
     any of them is not hidden, so a new service is hidden with no change.
     With `LAMP_HERMES_SKILLS` and `LAMP_SKILL_TASKS` off nothing under
     `/run` is shown to Hermes; with `LAMP_HERMES_SKILLS` on (the product's
     default), lamp-skills' socket folder; with `LAMP_SKILL_TASKS` on,
     lamp-control's control socket file alone (both next).
     Shown on lamp-c0a4 by hand (10 Nov, the earlier deny-list scripts):
     the key hidden; `umount` refused, nested too. As plain `lampos`, `ls
     /proc/<lamp-cloud pid>/root/run/credentials/lamp-cloud.service/`
     lists `account build key`; inside the wrapper's namespaces without
     the PID namespace it is `Permission denied`, but `/proc/<pid>/fd` is
     still listable; with `--pid --fork --mount-proc` the lamp-cloud pid
     does not exist there and only 3 processes are visible. The allow-list
     scripts are still to be shown there. The relay key never goes into
     Hermes's environment or its folder; only the forwarder's token does
     (Hermes keeps snapshots of its shell's environment under its home).
   - **Harness's hooks inside the agent namespace** (#6530; the shared
     agent namespace piece, `crates/update/src/agent_ns.rs`, #6510's "one
     namespace for every engine", used today by Hermes's and OpenClaw's
     namespaces). Harness writes an engine's hooks as bare commands
     (`harness hook hermes session-start`, `harness hook hermes tool`),
     found by `PATH`. Inside the namespace `/var/lib` is an empty tmpfs, so
     `/usr/local/bin/harness` (a link into `/var/lib/lampos-tasks/.local/bin`)
     leads nowhere, and Harness's state is never shown there. Until Harness
     ships a hook mode for a sandbox (openharness#4194), lamp-update writes a
     stand-in, `/usr/local/lib/lampos/ns-bin/harness` (root's, 0755, in its
     own root's 0755 folder), with each engine's namespace script and takes
     it out with the last of them. It answers `harness hook …` with nothing
     and exit 0 and refuses anything else with one line; it reads nothing,
     so it shows no Harness state. The namespace scripts (`hermes-ns`,
     `openclaw-ns`, and OpenClaw's wrapper before it joins the running
     gateway's namespaces) put `/usr/local/lib/lampos/ns-bin` first on the
     `PATH` right before the line that runs the engine. Each start of an
     engine's wrapper writes one journal line, outside the namespace, with
     the engine's tag: `Harness hooks aren't available inside the lamp's
     agent namespace yet (openharness#4194)`; no `logger`, or one failing,
     never fails the start. A missing or changed stand-in is the engine
     missing, written again. The stand-in is removed once a sandbox hook
     mode ships in the pinned Harness.
   - **The lamp's skills, behind their own switch** (#6128;
     `LAMP_HERMES_SKILLS` in `lamp-update.service`'s environment, read as
     `LAMP_HERMES_ON_LAMP` is: set, on only when `on` exactly, so `off`
     turns it off; unset, on on every product lamp, `SKILLS_PRODUCT_DEFAULT`
     on, the product owner's decision (11 Nov), as a person has no way to
     turn it on; off on any other lamp; no product unit names it). The
     caller check below is what makes on by default safe.
     On, the namespace script is written with four more steps
     (`hermes_install::ns_text(true, _)`): lamp-skills' tools folder
     `/run/lampos-skills` (`tools.json` and `call.sock`, the voice's
     `LAMP_SKILLS_DIR`) staged on the new `/tmp` with the rest before
     `/run` is hidden (skipped when the folder is not there), moved back
     and remounted read-only (a unix socket still connects on a read-only
     mount), and its staging folder removed; any of them failing ends it
     before Hermes runs. Then `lamp-tasks skill call NAME '{…}'` reaches the
     lamp as the voice's model does, through lamp-skills, which applies its
     cooldowns, pause and policy, and which tells its caller apart
     (`docs/skills.md`, "The call socket tells its caller apart"): only lamp-voice's process may give
     a requested-audio context or authorize and finish requested audio,
     and a call from any other peer is refused while the lamp is private.
     Nothing else under `/run` is shown. The switch is carried in the
     script's own text, which root writes and `lampos` cannot change; the
     script reads no flag. A round passes it to the one-shot in its ask
     (`"skills": true`; an ask without it is off), and the script with the
     other text is Hermes not there as this build puts it, so turning the
     switch on or off has the next reached round write the script again
     (no installer runs; Hermes at the pin gets its config and both
     scripts again). Tests: the script run by `sh` with the switch on and
     off (each new step failing stops it; off, the folder there is never
     named), `no_lampos_service_folder_is_visible_to_hermes` (off, no
     folder under `/run` is named; on, exactly `/run/lampos-skills`), and
     the switch turned on and off over an installed Hermes.
   - **lamp-control's socket for a skill's run, behind `LAMP_SKILL_TASKS`**
     (#6306; in `lamp-update.service`'s environment, the same name
     lamp-tasks reads to serve a skill's task, so one name turns skill
     tasks on; on only when `on` exactly, on any lamp; the product's unit
     sets it). A skill's run under Hermes is told
     `LAMP_CONTROL_SOCKET=/run/lampos-control/control.sock`
     (`lamp_tasks::skill_task::CONTROL_SOCKET`), which the empty `/run`
     would hide. On, the namespace script is written with these lines too
     (`hermes_install::ns_text(_, true)`, `TASKS_EDITS`, each found once in
     the script with the skills switch on or off):

     ```sh
     … L=/run/lampos-control/control.sock S=/tmp/.lampos-ns
     if [ -S "$L" ]; then touch "$S/l" && mount --bind "$L" "$S/l" || fail "no $L"; else L=; fi
     if [ -n "$L" ]; then mkdir -p "${L%/*}" && touch "$L" && mount --move "$S/l" "$L" && ro "$L" || fail "$L not shown"; fi
     rm -f "$S/r" "$S/l" && …
     ```

     The socket file alone is staged on the new `/tmp` before `/run` is
     hidden (skipped when no socket is there), moved back into an
     otherwise empty `/run/lampos-control` after it, and remounted
     read-only (a unix socket still connects on a read-only mount, as
     `/run/lampos-skills` does); its staging file is removed; any of these
     failing ends it before Hermes runs. The folder is never shown, so
     lamp-control's owner socket `admin.sock` beside it stays hidden, and
     no line names it. What the run can do there is what its one-run
     token grants. As the skills switch, it is carried in the script's
     text alone; a round passes it in its ask (`"tasks": true`; an ask
     without it is off), and turning it on or off has the next reached
     round write the script again, with no install. OpenClaw's namespace
     never shows it. Tests: the script run by `sh` with it on, alone and
     with the skills switch (each new step failing stops it; `admin.sock`
     beside the socket never named; no socket, skipped), the text
     unchanged with it off, both switches' edits together, and the switch
     turned on and off over an installed Hermes.
   - **A skill's run gets its own network namespace, with two ways out**
     (#6306, the first builder's design on #6443; behind `LAMP_SKILL_TASKS`
     as the control socket is). A skill's run has `LAMP_NS_NET=run`: set
     by the wrapper itself for a run Harness starts in the run's own folder
     (#6510 Part 1 step 2, `skill_task::HarnessRunner`, next), or by
     lamp-tasks for a run it starts itself (`skill_task::HermesRunner`, kept
     for #6643's skill make). It is the only opt-in, and it only takes
     access away. Harness's own panes never set it, so Hermes started there
     in any other folder keeps its network as before. With the switch on,
     the wrapper is written with these lines first
     (`hermes_install::RUN_LINES`, the same in every engine's wrapper),
     before the forwarder token is read:

     ```sh
     P=$(pwd -P 2>/dev/null) || P=
     R=
     case $P in /var/lib/lampos-tasks/harnesses/lamp-tasks/skill-?*) R=${P#/var/lib/lampos-tasks/harnesses/lamp-tasks/skill-} ;; esac
     case $R in *[!abcdefghijklmnopqrstuvwxyz0123456789-]*) R= ;; esac
     [ ${#R} -le 64 ] || R=
     unset HERMES_YOLO_MODE
     if [ -n "$R" ]; then
       F=/run/lampos-tasks/runs/$R
       C=
       if [ -f "$F" ] && [ ! -L "$F" ]; then C=$(cat -- "$F") || C=; fi
       rm -f -- "$F" || { echo "${0##*/}: the token file of run $R is not deleted" >&2; exit 1; }
       [ -n "$C" ] || { echo "${0##*/}: no token for run $R" >&2; exit 1; }
       LAMP_CONTROL_TOKEN=$C LAMP_CONTROL_SOCKET=/run/lampos-control/control.sock PYTHONPATH=/usr/local/lib/lampos/sdk-python LAMP_NS_NET=run
       unset C
       export LAMP_CONTROL_TOKEN LAMP_CONTROL_SOCKET PYTHONPATH LAMP_NS_NET
       # Nobody answers a skill's run: Hermes asks no approval in it, its hardline list still holds (#6684).
       HERMES_YOLO_MODE=1
       export HERMES_YOLO_MODE
     fi
     ```

     Harness passes an engine no variable of lamp-tasks', so a run is
     named by its folder: only when `pwd -P` is exactly lamp-tasks' Harness
     project `/var/lib/lampos-tasks/harnesses/lamp-tasks` plus
     `skill-<run-id>`, with a plain run id (`a-z`, `0-9`, `-`, at most 64),
     is it a run. Then lamp-tasks' token file for it (0600 in `lampos`'s
     0700 tmpfs folder) is read once if it is a regular file (a link is
     never read through), deleted whatever it is, and no token ends the
     run before anything else runs, so a second start in the same folder
     (Harness relaunching the session, a Restart, `harness resume`) gets
     none. The token goes on in the environment only, never an argument,
     with lamp-control's socket and the SDK's `PYTHONPATH`, as
     `HermesRunner` gives them. Any other folder: these lines change
     nothing. `HERMES_YOLO_MODE` is unset for every start and set to 1
     only in a run (#6684): nobody is there to answer Hermes's approval
     prompt, and its hardline list still holds. With the switch off, the
     wrapper has no run lines, and its own `unset T HERMES_YOLO_MODE`
     drops the variable for every start. With `LAMP_SKILL_DEFAULT_AGENT=on`
     as well, `/usr/local/bin/hermes` is not this wrapper but the one run
     path's, with Hermes's data, and `hermes-ns` is taken out ("One agent
     namespace for every engine", "The one run path"). Inside the namespace `/run` is a fresh empty tmpfs, so no
     namespaced session can read another run's token file; a process
     outside a namespace running as `lampos` still could (a known limit).
     Then:

     ```sh
     if [ "${LAMP_NS_NET-}" = run ]; then N=--net; else N=; unset LAMP_NS_NET; fi
     exec /usr/bin/unshare --map-root-user $N --mount --pid --kill-child --mount-proc --propagation private /usr/local/lib/lampos/hermes-ns "$@"
     ```

     `run` exactly adds `--net` to the first `unshare`, in the same call
     as `--map-root-user`, so the new network namespace is owned by the new
     user namespace and its mapped root may bring `lo` up there; any other
     value is unset, so the namespace script never sees `run` without
     `--net`. The namespace has only `lo`: no address outside it resolves
     or connects (`resolv.conf` is still shown, which is harmless: its
     server is not reachable), and the lamp's abstract unix sockets, which
     belong to a network namespace, are gone too. The namespace script
     (`TASKS_EDITS`) then, only with `LAMP_NS_NET=run`:

     ```sh
     … L=/run/lampos-control/control.sock B=/run/lampos-tasks/model.sock P=/var/lib/lampOS/skills-dev S=/tmp/.lampos-ns
     if [ "${LAMP_NS_NET-}" = run ]; then [ -S "$B" ] && touch "$S/b" && mount --bind "$B" "$S/b" || fail "no $B"; fi
     if [ "${LAMP_NS_NET-}" = run ] && [ -d "$P" ]; then mkdir -m 0700 "$S/p" && mount --bind "$P" "$S/p" || fail "no $P"; else P=; fi
     if [ "${LAMP_NS_NET-}" = run ]; then mkdir -p "${B%/*}" && touch "$B" && mount --move "$S/b" "$B" && ro "$B" || fail "$B not shown"; fi
     if [ -n "$P" ]; then mkdir -p "$P" && mount --move "$S/p" "$P" && ro "$P" || fail "$P not shown"; fi
     rm -f "$S/r" "$S/l" "$S/b" && { [ -z "$P" ] || rmdir "$S/p"; } && …
     …
     if [ "${LAMP_NS_NET-}" = run ]; then
       I=; for p in /usr/sbin/ip /sbin/ip; do if [ -x "$p" ]; then I=$p; break; fi; done
       [ -n "$I" ] || fail "no ip: lo not up, the run has no way out"
       "$I" link set lo up || fail "lo not up"
       [ -z "$("$I" -o link show | cut -d: -f2 | tr -d ' ' | grep -vx lo | grep -v '@NONE$')" ] || fail "a link besides lo: the run is not held off the network"
       M=$(/usr/bin/unshare --user --map-user="$U" --map-group="$G" /opt/lampOS/tasks/current/lamp-tasks net-relay --listen 127.0.0.1:7741 --to "$B" &)
       [ "$M" = ready ] || fail "no model relay on 127.0.0.1:7741"
     fi
     unset LAMP_NS_NET
     ```

     It shows the skills the owner's computer pushed,
     `/var/lib/lampOS/skills-dev`, read-only as the skills are (skipped when
     the folder is not there), so a pushed skill's script is there for its
     run; Hermes started any other way never sees them. It shows lamp-tasks'
     bridge socket `/run/lampos-tasks/model.sock`, the
     file alone, read-only, as the control socket is (missing, the run
     ends: the bridge is served only with the switch on); brings `lo` up
     (no `ip`, the run ends) and checks `lo` is its only link with a way out: a link besides `lo` ends
     the run (it is not held off the network), except the kernel's own
     fallback tunnels a new namespace gets when their modules are loaded
     (`sit0@NONE`, `tunl0@NONE`, `ip6tnl0@NONE`: no device under them, so no
     way out; Heron's P3 on #6443); and starts `lamp-tasks net-relay` (lamp-tasks'
     own program, under `/opt`, which is not hidden) in the background,
     waiting for its `ready` line (none, the run ends). The relay runs as
     the caller's own ids in a nested user namespace of its own, as Hermes
     does, so it holds no capability over the namespace's mounts that
     Hermes could borrow. It listens on `127.0.0.1:7741`, where Hermes's
     model calls go (`http://127.0.0.1:7741/v1/deepseek`), and pipes each
     connection to the bridge; outside, lamp-tasks pipes each bridge
     connection to lamp-cloud's forwarder on TCP `127.0.0.1:7741`
     (docs/tasks.md, "A skill's task on the lamp's Hermes"). The PID
     namespace ends the relay with Hermes. So a skill's run has exactly two
     ways out: `control.sock` (what its one-run token grants) and the
     model forwarder (what its forwarder token allows). Off, the wrapper
     and the script are unchanged; OpenClaw's never get any of it. Tests:
     the wrapper run by `sh` with `LAMP_NS_NET` `run`, unset and other
     values (`--net` and the variable only with `run`), the wrapper run by
     `sh` in a run's folder (`hermes_finds_a_skill_run_by_its_folder_and_reads_its_token_once`:
     the token file read once and deleted, the token, socket, SDK and
     `--net` given, a second start refused "no token for run", a link
     refused, any other folder as before), the script run by
     `sh` with a net run (each new step failing stops it, `ip` missing or
     the bridge missing too; with the switch off, the variable changes
     nothing), the text with every step behind `run`, and OpenClaw's texts
     with none of it. What only a lamp shows: `--net` with
     `--map-root-user` as `lampos`, `ip link set lo up` there, and a run's
     model call answered while `curl https://example.com` fails.
   - **User namespaces checked first:** before anything is made, `lampos`
     must be able to make that namespace: `runuser -u lampos --
     /usr/bin/unshare --map-root-user --mount --pid --kill-child
     --mount-proc --propagation private /usr/bin/unshare --user --map-user=<uid> --map-group=<gid> true`. It
     failing is `no_userns`, said with the reason, and nothing is
     installed: Hermes never runs where it could read the key. lamp-c0a4
     had kernel 5.15, `user.max_user_namespaces` 15294 and
     `/usr/bin/unshare`.
   - **Hermes's own update and lingering turned off:** after the install,
     as `lampos`, `hermes config set` (Hermes's own writer, which changes
     that key alone and keeps every other line, Harness's `hooks:` block
     among them) sets `updates.check false` (no update check of its own,
     so nothing offers a Hermes past the pin; Hermes applies no update
     unless `hermes update` is run by hand), `plugins.auto_update_check_hours
     0`, `lsp.enabled false` (no language servers, which Hermes otherwise
     starts after an edit and keeps 600 s idle) and
     `terminal.oneshot_completion_wait_seconds 0` (a `chat -q` run ends
     with its turn, not up to 600 s later). Its messaging gateway is never
     installed or started: the installer's `gateway` stage is not run, and
     nothing runs `hermes gateway`. One not set is `config`. Then
     `hermes --version` is run as `lampos`, and its first line journaled;
     failing, `installer`.
   - **Another Hermes moved aside:** a Hermes in `/var/lib/lampos-hermes`
     that is not this build's (its checkout at another commit or on a
     branch, a `hermes` with no checkout at a commit, or the folder
     another user's) is moved whole to `/var/lib/lampos-hermes.old` before
     the install, after the free-space check, and removed once the new
     Hermes has run. A failed install keeps it there; a later try that
     finds another one in the folder with one aside already removes that
     one instead. A link at the folder is removed, never followed. A
     checkout at the pinned commit left by a try that was cut off is kept:
     the installer repairs it in place.
   - **Then, in this order:** the namespace script, then the wrapper (so
     `lampos` never finds a wrapper before what it runs); any not written,
     or no `/usr/local/lib/lampos` folder, is `entry`. Then the old Hermes
     removed, and, after an install that ran the installer,
     `systemctl try-restart lamp-tasks.service`: lamp-tasks merges Hermes's
     settings at its start (the forwarder as its provider, the lamp's
     skills), and is started only if it was running. Hermes there at the
     pin with an older build's wrapper gets its config and both scripts
     again, with no installer and no restart.
   - **The one-shot's sandbox:** `ReadWritePaths=/usr/local/bin
     -/usr/local/lib/lampos` (since #5978; `-`, so a lamp without that
     folder still installs Harness), `ProtectSystem=full` (so `/var` is
     writable for Hermes's folder), `TimeoutStartSec=3600`, for Harness's
     install as well. `/etc` is not writable. It runs at `Nice=10`,
     `IOSchedulingClass=idle` and `CPUWeight=20`, as lamp-update does, and
     is capped: `CPUQuota=100%`, writes 8 MB/s and 200 IOPS, reads 16 MB/s
     and 400 IOPS on `/var/lib`'s card, and `MemoryHigh=600M`: past it
     the kernel reclaims the one-shot's own memory and throttles it, never
     kills it (no `MemoryMax`). At `CPUQuota=150%` and `MemoryHigh=1200M`
     the `products` stage took `MemAvailable` down to about 786 MB on
     lamp-c0a4 and the voice lost its privacy reading (2 of 2 runs); at
     100% and 600M it stayed above 1.4 GB, the privacy reading held, and
     the whole install took 20 min (`products` 531 s) on 11 Nov. On lamp-c0a4 on 10 Nov a Hermes
     install at the default priority stalled lamp-controls' loop 93 ms past
     the privacy reading's 50 ms, and the voice kept the microphones closed
     until it was restarted. The lower priority alone did not stop it: the
     card's scheduler is `none`, so the idle I/O class has no effect, and the
     install's 2.4 GB of writes (the Python packages above all) took the card.
   - **Taken out:** a failed install takes the wrapper and the namespace
     script out, the wrapper first, so `lampos` finds no `hermes`. With the
     switch off, a reached round that finds either writes the ask with
     `"remove": true` and starts the one-shot, which takes them out with
     the record and runs nothing else. Hermes's folder stays.
   - **Pinned:** the installer of hermes-agent's tag `v0.22.2`
     (`scripts/install.sh` at the pinned commit) is compiled in, a
     byte-for-byte copy at `deploy/hermes/install-v0.22.2-f4cdb0e2.sh`,
     with its SHA-256 (`INSTALLER_SHA256`, `f4cdb0e2…`). It is checked
     before anything runs, written as `hermes-install.sh` in a new folder
     under the one-shot's private `/tmp` (root's, folder 0755, file 0644),
     checked again, run with `bash`, and removed after; other bytes are
     `pin_mismatch` and nothing runs. The commit is the tag `v0.22.2`,
     `d923a5eb487a17c0a68801bba9ef0f56c81040a8` on `main` (`COMMIT`),
     passed as the installer's own pin, `--branch main --commit <COMMIT>`:
     the installer checks the commit is on `main`, checks it out detached,
     and looks up no release. After it, the checkout's commit is read;
     another is `failed` with `version`. Why 0.22.2: Harness takes no
     session id of Hermes 0.22.3 or later until openharness main's fix is
     in a Harness release. A new Hermes is a PR that copies the installer
     again and updates `COMMIT`, `VERSION` and `INSTALLER_SHA256`; a test
     holds them to the file. Harness's Hermes binding needs Harness 0.5.98
     or later; this build's Harness pin is still 0.5.39
     (`harness_install::product::VERSION`), so the pin must move in the
     same release (#5957).
   - **The stages:** `prerequisites repository venv python-deps config
     products`, each run alone (`--stage`) with `--non-interactive
     --skip-browser --skip-computer-use`, each stage's time journaled.
     Version 1's list (`runtimes/hermes/install.sh` in the autonomous
     repo) skipped `node-deps`, which hung on ARM; this installer has no
     such stage: its `products` stage publishes the command, and Node's
     tools come through its package manager only as a build asks for them.
     `setup` and `gateway` need a person and are not run; `complete` writes
     only a marker. Each stage has the installer's 15-min bound
     and the 40 min it may be kept frozen while someone speaks
     (`HERMES_STAGE_WITHIN`, 55 min: a stage's clock goes on while frozen,
     and on c0a4 a products stage frozen 21 min was taken for timed out
     the moment it thawed); the unit's is an hour for Harness and Hermes
     together.
   - **Memory first:** before anything is moved or made, and before the
     disk is looked at, `MemAvailable` in `/proc/meminfo` must be at least
     1 GiB (`NEEDS_AVAILABLE_KB`, 1,048,576 kB); less, or it not read (no
     file, no such line, not a number in kB), is `no_memory` and nothing
     is changed. The floor is from lamp-c0a4 on 10 Nov, which had about
     2.1 GB available at rest with the voice running; the install's own
     peak is still to be measured. pip's `python-deps` stage is the heavy
     one.
   - **Free space next:** before anything is moved or made, `df -Pk
     /var/lib` must show at least 4 GiB available (`NEEDS_FREE_KB`); less,
     or `df` not read, is `no_space` and nothing is changed. Hermes v0.22.2
     took 3.0 GB on lamp-c0a4. The check reads the file system's free
     space alone: a partial `/var/lib/lampos-hermes` or a
     `/var/lib/lampos-hermes.old` left by a failed try is not counted as
     room the install gets back, so their space is taken off the 4 GiB
     like any other file's. Neither check runs when Hermes is already
     there at the pin in a `lampos` folder (the try only repairs the
     entry points and config then).
   - **Disk on a 16 GB card:** the lampOS image plus Harness and Hermes
     use about 7.0 GB (Hermes's folder about 2.4 GB of it), leaving
     roughly 7–8 GB free. Measured on lamp-c0a4 (10 Nov): 13.1 GB used on
     its 58 GB card, of which 6.1 GB were test copies, so about 7.0 GB
     for the product.
   - **What the lamp must have:** `git`, `curl` and `bash`, looked for as
     `lampos` with the install's `PATH`, and `/usr/bin/unshare`, before the
     installer runs (`no_tools` otherwise). Nothing is added to the system
     for them.
   - **The record:** `hermes-install.json` in the records, next to
     Harness's, `{"install", "revision", "at_unix_ms", "version"?,
     "commit"?, "error"?}`, lamp-update's own (not on `update/status`
     yet). `install` is `absent`, `installing`, `installed` or `failed`;
     `error` one of `pin_mismatch`, `no_user`, `no_space`, `no_memory`,
     `no_tools`,
     `no_userns`, `serve` (one of lamp-update's own steps: the installer's
     file, moving the old Hermes aside, making the folder), `timeout`,
     `installer`, `version`, `config`, `entry`, `not_started`,
     `privacy_lost` and `interrupted` (stopped by the watch above); `version`
     and `commit` with `installed`. Switch off, `serve` removes the record
     and the ask at its start; the entry points go as above.
   - **Tried again** by the first reached round 24 h after a failure
     (`RETRY_AFTER`), or after `installing` for over 70 min; `serve`,
     `not_started`, `privacy_lost` and `interrupted` at the next round
     whose window is open (`RETRY_IN_NEXT_WINDOW`), that is the next
     round reached outside the setup walk. Every retry waits for its
     window. A failed install takes out the
     wrapper and the namespace script, but keeps its partial
     `/var/lib/lampos-hermes` (up to about 2.4 GB) until that retry 24 h
     later, which continues it when its checkout is at the pin or not made
     yet, and moves it aside (or removes it) otherwise; a `/var/lib/lampos-hermes.old` set aside
     before the failure stays until a later install's new Hermes runs.
   - **The parts' units** (`deploy/product`): unchanged; none sets
     `LAMP_HERMES_ON_LAMP` in this change, and none names
     `LAMP_HERMES_SKILLS` (on by lamp-update's own default): lamp-update installs the
     product's Hermes by its own default (on since the product owner's
     decision, 10 Nov, with the window above), and
     `LAMP_HERMES_ON_LAMP=off` in its unit turns that off. lamp-cloud's
     forwarder (`LAMP_HERMES_ON_LAMP=on` in `lamp-cloud.service`, which
     writes only its own state folder, so it needs no other path and no
     group) and lamp-tasks' side (`lamp-tasks.service`) are turned on by
     the units' own change. lamp-tasks' `--engines-from-machines` (without
     it the lamp's own Hermes is never looked for, and the lamp is no
     target) and `--reply-named`, `LAMP_TASKS_REPLY_NAMED` (lamp-voice) and
     `LAMP_TASKS_RESUME_CLOSED` (lamp-tasks) come with the routing that
     reads them (#6094).
   - **Shown only in tests** (`hermes_install::tests`,
     `hermes_schedule::tests`, the wrapper and the namespace script run by
     `sh` there too). Not run on a lamp yet: the whole install by
     lamp-update, its window, a freeze and thaw, and a stop on a lost
     privacy reading. Measured by hand on lamp-c0a4 (10 Nov,
     the same installer as `lampos`, the same six stages): 557 s in all,
     +3.0 GB, inside each stage's 15-min bound and the unit's hour.

   **OpenClaw on the lamp** (#6226; `LAMP_OPENCLAW_ON_LAMP` in
   `lamp-update.service`'s environment, read as `LAMP_HERMES_ON_LAMP` is:
   set, on only when `on` exactly; unset, on on a lamp installed as the
   product, off on any other lamp; no product unit sets it). OpenClaw is
   the lamp's second engine under Harness (no memory floor: the owner,
   #6226, every lamp has 6 GB; Harness's engine id `openclaw`; #6226 made
   it the default, #6498, the lead's decision, made Hermes the default
   again), installed beside Hermes in exactly Hermes's role and under the
   same rules: the
   same schedule (window, freeze while the voice is not quiet, stop on a
   lost privacy reading, the one-shot's bounds), the same user `lampos` in
   its own user and mount namespace, its DeepSeek through lamp-cloud's
   forwarder, and the lamp's skills. Hermes is the lamp's target while it
   is installed, OpenClaw when it is not or when chosen
   (`LAMP_LAMP_ENGINE` in lamp-tasks, docs/tasks.md). The code is
   `crates/update/src/openclaw_install.rs`.
   - **What lamp-update puts in:** a Node 24 runtime in
     `/var/lib/lampos-openclaw/node` (OpenClaw needs Node 24, and Harness
     reads OpenClaw only on Node 24, while the lamp's Harness stays pinned
     on Node 22), the tarball checked by its SHA-256; the npm package
     `openclaw` 2026.11.2 and its whole tree in
     `/var/lib/lampos-openclaw/npm`; the wrapper `/usr/local/bin/openclaw`,
     so Harness finds `openclaw` on `lampos`'s login `PATH`; the namespace
     script `/usr/local/lib/lampos/openclaw-ns`; OpenClaw's state in
     `/var/lib/lampos-openclaw/state` (`OPENCLAW_STATE_DIR`), its tools
     working in the lamp's own work folder
     (`/var/lib/lampos-tasks/harnesses/lamp-tasks/lamp`, set by lamp-tasks
     as `agents.defaults.workspace`); and an initial
     `<state>/openclaw.json` with only its `gateway` block
     (`mode: "local"`, `bind: "loopback"`, token auth). lamp-tasks merges
     the rest in at its start (docs/tasks.md, "OpenClaw's settings on the
     lamp").
   - **From a pinned lockfile, inside the namespace:** the tree is never
     resolved freely. The build carries `crates/update/openclaw/package.json`
     (`openclaw` at 2026.11.2 exactly) and its `package-lock.json` (made
     once with `npm install --package-lock-only --ignore-scripts`: every
     package with its registry tarball and sha512 integrity). Before
     anything is fetched, lamp-update checks the lockfile's SHA-256, that
     its `openclaw` is the registry tarball with the pinned integrity, and
     that every other package carries a registry tarball and a sha512
     integrity or comes bundled inside one that does; any other is
     `pin_mismatch`. It writes both files in `/var/lib/lampos-openclaw/npm`
     (root's, 0644), writes `openclaw-ns` first, then runs `npm ci` as
     `lampos` through `unshare --map-root-user --mount --propagation
     private /usr/local/lib/lampos/openclaw-ns` with
     `LAMP_OPENCLAW_NPM_CI=on`, the script's one other mode: behind the
     same mounts, it runs only the pinned Node's `npm ci --omit=dev
     --ignore-scripts --no-audit --no-fund --no-update-notifier
     --loglevel=http` in that folder. npm checks each tarball against the
     lockfile's integrity, and no package's install script runs (OpenClaw's
     own preinstall only warns; its postinstall step is finished by
     OpenClaw itself at its first run, which is the install's `openclaw
     --version`, in the namespace too). npm never runs as `lampos` outside
     the namespace. Checked in Docker (Node 24.22.2, linux-x64, 11 Nov):
     `npm ci --ignore-scripts` from this lockfile added 364 packages in
     about a minute, `openclaw --version` said `OpenClaw 2026.11.2` and
     cleared its pending package step, and the lockfile was left as it was.
     A lamp that had the earlier `npm install --global` layout finds no
     package at the new path, so its next install sets the old one aside
     and installs from the lockfile.
   - **The namespace and the PID namespace:** one PID namespace for the
     Gateway and all its panes. OpenClaw 2026.11.2 refuses to open its
     state database from a PID namespace other than its Gateway's
     (`GatewayLockNamespaceError: cannot verify Gateway ownership from this
     process (different PID namespace)`), so with one per call, as Hermes
     has, every Harness pane exited at once on lamp-c0a4 and no task was
     delivered (`dispatch uncertain stopped_before_message`). So:
     - The gateway's unit sets `LAMP_OPENCLAW_GATEWAY=on`, and with it the
       wrapper runs `unshare --map-root-user --mount --pid --fork
       --kill-child --mount-proc --propagation private
       /usr/local/lib/lampos/openclaw-ns`: a user, mount and PID namespace
       of its own with its own `/proc`, as Hermes's.
     - Any other call (a Harness pane) takes the gateway's main process
       from systemd (`systemctl show -p MainPID`, never a file OpenClaw
       could write), checks it is `lampos`'s, in another user namespace and
       still in the unit's cgroup (so a pid reused after the gateway
       stopped is never taken for it),
       and joins its namespaces: `nsenter --user --mount
       --pid=/proc/<pid>/ns/pid_for_children --preserve-credentials`, then
       the caller's own ids in a nested user namespace, as `openclaw-ns`
       ends. The Gateway lock then sees one namespace.
     - With no gateway running (or its main process not in a namespace of
       its own), a call fails closed: `openclaw: its gateway
       (lamp-openclaw-gateway.service) is not running in its own
       namespaces`. It never runs in a namespace of its own nor in the
       lamp's. A gateway restart ends its panes with its namespace.
       `LAMP_OPENCLAW_GATEWAY=on` set by any other `lampos` caller only gives
       that caller namespaces of its own, never a way into the lamp's.
     - The wrapper sets `OPENCLAW_NO_AUTO_UPDATE=1`, so OpenClaw never
       checks for or offers an update of its own: lamp-update's pinned
       lockfile is the only way it changes, as Hermes's `updates.check
       false`.

     So OpenClaw sees only OpenClaw's processes and, though it runs as
     `lampos` as every lamp service does, can signal none of them. Checked
     on lamp-c0a4 (11 Nov, 18:2x) from inside a pane's namespaces: `/proc`
     listed 7 processes (the Gateway as pid 1, its workers and the check
     itself), and `kill -0` on lamp-tasks, lamp-voice, lamp-cloud and
     Harness's daemon each said `No such process`. The install's own
     steps (`npm ci`, `openclaw --version`) run with the gateway's flags,
     in a PID namespace of their own. `openclaw-ns` itself reads only
     `/proc/self/uid_map` and `findmnt` (`/proc/self/mountinfo`), which are
     the calling process's own.
   - **The Gateway:** OpenClaw's TUI panes need a running Gateway, and
     Harness never starts or configures one.
     `lamp-openclaw-gateway.service` runs `openclaw gateway run` through
     the wrapper as `lampos`, on loopback `ws://127.0.0.1:18789`, with the
     token in `/var/lib/lampos-openclaw/gateway-token` (0600 `lampos`),
     made once by lamp-update at the install. One Gateway serves every
     pane.
   - **The forwarder token:** the wrapper exports `LAMP_HERMES_TOKEN` from
     lamp-cloud's `/var/lib/lampos-cloud/hermes-token`, as Hermes's does,
     for the `lampos` provider's `apiKey` (`${LAMP_HERMES_TOKEN}`).
   - **The lamp's skills** behind `LAMP_OPENCLAW_SKILLS` (unset, on on every
     product lamp, read only with `LAMP_OPENCLAW_ON_LAMP` on), as
     `LAMP_HERMES_SKILLS` does for Hermes: on, `openclaw-ns` shows
     `/run/lampos-skills` read-only; off, nothing under `/run`.
   - **No skill's run on OpenClaw yet** (#6685): its one gateway has no
     wrapper started per run, so it has no part in the one run path ("One
     agent namespace for every engine"), and `openclaw-ns` never shows
     lamp-control's socket or lamp-tasks' runs' token folder. With
     `LAMP_SKILL_DEFAULT_AGENT=on` and OpenClaw the lamp's default,
     lamp-tasks refuses a skill's run in words (docs/tasks.md, "The lamp's
     default agent"); OpenClaw's own fenced path per run is a later step.
   - **Records:** `openclaw-install.json` (the record), `openclaw-ask.json`
     (the ask) and `openclaw.lock` (one install at a time), beside
     Hermes's in lamp-update's records.
   - **Not shown on a lamp.**

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

## One agent namespace for every engine (#6510)

#6510 Part 1, step 1 (`crates/update/src/agent_ns/wrapper.rs`, beside the
shared hook stand-in in `agent_ns.rs`): one wrapper and one namespace
script for any engine Harness reports on the lamp, with no engine named in
the code. Written by a round only with `LAMP_AGENT_NS=on` (below); not
yet on a lamp.

- **Engine names** are data and must be plain: `a-z`, `0-9` and `-`, 1 to
  32 bytes (`engine_ok`). Anything else is refused before a path or a text
  is made.
- **The wrapper**, `/usr/local/bin/<engine>` (root's, 0755), carries
  `Written by lamp-update.` in its first 512 bytes, the mark lamp-tasks'
  `LAMP_WRAPPER_MARK` (#6506) looks for. It:
  - finds the engine's own program on the caller's `PATH` (`lampos`'s, as
    Harness starts it): the first `<engine>` there that is not the wrapper
    itself (by `readlink -f`) and carries no mark (so never another
    lamp-update wrapper or the hook stand-in); only full folders of `PATH`
    count. None, and it ends. No path to the program is written in any
    text;
  - for a skill's run, found by its folder alone (#6510 Part 1 step 2:
    `pwd -P` exactly `/var/lib/lampos-tasks/harnesses/lamp-tasks/skill-<run-id>`
    with a plain run id; Hermes's run lines, `hermes_install::RUN_LINES`,
    above), reads `/run/lampos-tasks/runs/<run-id>` (lamp-tasks' token
    file, 0600 in `lampos`'s 0700 folder; a link there is never read
    through) into a shell variable once, deletes the file whatever it
    held, and ends if there was no token. The token goes on as
    `LAMP_CONTROL_TOKEN` in the environment only, never in an argument,
    with `LAMP_CONTROL_SOCKET` and `PYTHONPATH`; the run gets `--net` and
    `LAMP_NS_NET=run`, as Hermes's skill runs do. The provisional
    `LAMP_RUN_ID` is gone. Not in a run's folder, a `LAMP_CONTROL_TOKEN` or
    `LAMP_NS_NET` from the caller is unset;
  - writes the hook stand-in's journal line with the engine as its tag;
  - runs `/usr/local/lib/lampos/agent-ns` through `unshare
    --map-root-user [--net] --mount --pid --kill-child --mount-proc
    --propagation private`, with the engine's name and the program's path
    in `LAMP_NS_ENGINE` and `LAMP_NS_ENGINE_BIN`.
- **The namespace script**, `/usr/local/lib/lampos/agent-ns` (root's,
  0755), is Hermes's (`hermes-ns`, "Hermes on the lamp": the same
  allow-list, hidden `/run`, `/var/lib` and `/etc/lampOS`, private `/tmp`,
  hidden devices, the skills and `LAMP_SKILL_TASKS` switches, and a skill
  run's network) with the engine made data. It checks the name again and
  that the program is a full path to a file, sets `HOME` to the engine's
  own folder `/var/lib/lampos-agents/<engine>` (`lampos`'s, 0700; the
  folder above it root's, 0755), binds the program back read-only at its
  own path, puts the hook stand-in first on `PATH`, and runs the program as
  the caller's own ids in a nested user namespace. The lampos SDK (#6507,
  `/usr/local/lib/lampos/sdk-python`) is under `/usr`, which the namespace
  leaves as it is. A program that needs more than its own file (a Node or
  Python tree under a hidden folder) is not shown more yet.
- **Writing** (`write_engine`): the homes folder and the engine's home,
  the namespace script, the hook stand-in, then the wrapper last, each
  written whole beside and renamed in. A file at the wrapper's place
  without the mark (the engine's own program, or anyone else's) or a link
  there is refused, never replaced. `present` says the wrapper and the
  script hold this build's text, the home is there, and the wrapper is the
  `<engine>` `lampos`'s login shell finds (`login_finds`: `timeout 5
  runuser -l lampos -c 'command -v <engine>'`, as Harness looks, nothing
  found past 5 s: its installer puts
  `~/.local/bin` before `/usr/local/bin`, so an engine's own program there
  would run with no namespace); anything else is the engine missing, and
  one found before the wrapper is not offered.
- **Names** are `a-z`, `0-9` and `-`, never starting with `-`. A run's
  token file is read and deleted before the program is looked for, so a
  wrapper that finds no program leaves none behind.
- **Hermes and OpenClaw** keep their own wrappers and namespace scripts
  ("Hermes on the lamp", "OpenClaw on the lamp") with the one run path
  off. With it on (below), Hermes's wrapper is the one run path's;
  OpenClaw's stays its own and takes no skill's run.
- **Written by a round** (`crates/update/src/agent_ns/rounds.rs`), only
  with `LAMP_AGENT_NS=on` exactly in lamp-update's own unit (no product
  unit sets it; any other value or none is off), by a `serve` with
  `--apply on`, Harness's install on, and `LAMP_APP_ENGINES=on`, whose
  look at Harness (`harness machines engines <machine> --json`, every five
  minutes, #6240) is the list of engines. Either missing is said once at
  start, and nothing is written. After each round that reached the
  channel, after Hermes's and OpenClaw's own steps, every engine that look
  last said `installed` with a plain name, except `hermes` and `openclaw`,
  is written with `write_engine` unless its files already hold this
  build's text (`present` as if the login shell found the wrapper). Then
  `login_finds` is asked: an engine whose own program the login shell
  finds first, or none, is said, not written again; one
  `not_installed` is passed over, as a wrapper on `lampos`'s `PATH` would
  have Harness find it installed. `lampos`'s ids are looked up once, only
  when an engine is to be written. Each engine written is one line; one
  already there, none. A line that stays true round after round (a
  refusal, a program found before the wrapper or none, no ids for
  `lampos`) is said once (`rounds::Said`, kept by `serve` between rounds,
  as the look at Harness says a failed look once), and again only when it
  changes, or after it stopped being true. The namespace shows what
  Hermes's does (`LAMP_HERMES_SKILLS`, `LAMP_SKILL_TASKS`), so turning
  either changes the script and every engine is written again.
- **Taken out by a round**, before writing, with the same switch on: a
  wrapper lamp-update wrote (`wrapper::ours`: a regular file holding
  exactly this build's wrapper text for that name; the engine's own
  program, a link, an edited wrapper or an older build's text are never
  touched) is removed (`remove_ours`), one line each, when (a) Harness's
  last good look (its machine known; none before the first look or after
  a failed one) no longer says its engine is `installed`
  (`engines_to_remove`), or (b) no program of the engine's own is left on
  `lampos`'s login `PATH`. Harness may find an engine by `command -v`, so
  the wrapper alone would keep it `installed` forever; (b) is the real
  signal. It is `login_programs`: `timeout 5 runuser -l lampos -c 'E=<engine>;
  …'`, a fixed text with only the plain name put in, that walks the full
  folders of `$PATH` and prints each executable `<dir>/<engine>`, skipping
  `/usr/local/bin/<engine>` and any file with the lamp-update mark (as the
  wrapper itself skips them). Only a look that ends 0 and lists nothing
  takes a wrapper out; one that fails, runs past 5 s, or cannot be run (no
  `timeout` or `runuser`) keeps it, said once. It is asked at most once an
  engine a round. An engine with no program of its own is not written
  either (said once), so a removed wrapper is not written back the next
  round; it is written again once a program is there. `hermes` and
  `openclaw` are never taken out here. The engine's home (it may hold the
  engine's own data) and the shared `agent-ns` script stay. Off, nothing
  is written or taken out: what an earlier round wrote stays. The hook stand-in stays
  while `agent-ns` is there (it is among the namespace scripts Hermes's
  and OpenClaw's removal look for).
- **The one run path** (#6685, `LAMP_SKILL_DEFAULT_AGENT=on` in
  lamp-update's own unit, read only with `LAMP_SKILL_TASKS` on; no product
  unit sets it; lamp-tasks reads the same switch). One way to run a skill
  for every agent: every agent's wrapper is one text
  (`wrapper::one_path_text`), Hermes's included, and no agent is named in
  its run path. It is, in order:
  - `#!/bin/sh` and the mark line `# lampOS: an agent on the lamp, by the
    one run path (#6510, #6685). Written by lamp-update.`, then a header
    naming no agent;
  - **the agent's data** (`wrapper::Agent`), the only lines that differ
    from one agent to another: `E=<engine>`; `H=<home>`, its home,
    `/var/lib/lampos-agents/<engine>` or its engine's own home from an
    installer (`OWN_HOMES`, `(engine, home)` pairs: Hermes's
    `/var/lib/lampos-hermes`, for Hermes only, shown at that path since its
    checkout's scripts and Python name it; another agent naming it is
    refused); `X=<program>`, its
    program, or empty to find it on `PATH` as above; `agent_env() { … }`,
    lines run at every start; `agent_run_env() { … }`, lines run in a
    skill's run only. Hermes's (`hermes_install::agent`): its program
    `/var/lib/lampos-hermes/.local/bin/hermes`, at every start its
    forwarder token read from lamp-cloud's file (none ends it) and given
    as `LAMP_HERMES_TOKEN` with `HERMES_HOME=/var/lib/lampos-hermes/.hermes`
    and `HERMES_YOLO_MODE` unset, and in a run `HERMES_YOLO_MODE=1`
    (#6684). A plain agent's are empty;
  - **the run path**, after `# The run path, the same for every agent:`,
    the same bytes for every agent (a test holds it for Hermes, Claude
    Code and Codex, and pins Hermes's and Claude Code's whole wrappers in
    `crates/update/src/agent_ns/wrapper/one-path-*.sh`, which lamp-tasks'
    check is tested against): `unset LAMP_CONTROL_TOKEN LAMP_NS_NET`; the
    run lines (`hermes_install::ONE_RUN_LINES`: the lines above without
    Hermes's approvals); `agent_env`, then `agent_run_env` in a run; `N=`
    and `[ "${LAMP_NS_NET-}" != run ] || N=--net`; the program (its own,
    resolved by `readlink -f`, or the first on `PATH`); the hook stand-in's
    journal line tagged `$E`; and `exec /usr/bin/unshare --map-root-user
    $N --mount --pid --kill-child --mount-proc --propagation private
    /usr/local/lib/lampos/agent-ns "$@"`, with `LAMP_NS_ENGINE`,
    `LAMP_NS_ENGINE_BIN` and `LAMP_NS_HOME`.

  `agent-ns` then takes the home from `LAMP_NS_HOME` instead of
  `/var/lib/lampos-agents/$E` (`ONE_PATH_NS_EDITS`), and before anything
  is mounted checks it is `/var/lib/lampos-agents/<the same engine>` or
  that engine's own home from an installer, matched as `<engine>:<home>`
  (`case $E:$H in "$E":/var/lib/lampos-agents/"$E"|hermes:/var/lib/lampos-hermes)`,
  so Hermes's home is never another agent's), a folder and not a link
  (`one_path_home_check`; the list is root's text, `lampos` can change
  neither), and shows it read-write at its own path; the rest (the skills,
  lamp-skills' and lamp-control's sockets, a run's own network with the
  model bridge, the pushed skills) is as above. With the switch on, a
  round writes every other engine's wrapper as the one run path's
  (`wrapper::text_for`), and Hermes's install writes `/usr/local/bin/hermes`
  as the one run path's with Hermes's data and `agent-ns`, and takes
  `hermes-ns` out (`hermes_install::wrapper_for`, `ns_for`; its ask
  carries `"one_path": true`). Off, every wrapper and namespace script is
  as before, byte for byte. A wrapper of either text is lamp-update's
  (`ours`), so turning the switch writes it again. A round still writes a
  wrapper that the login shell does not find first (and says so); lamp-tasks
  therefore asks again, live, at each skill run's start, whether
  `command -v <engine>` in `lampos`'s login shell is the wrapper, and
  refuses the run otherwise (docs/tasks.md, "The lamp's default agent, by
  the one run path").
Tests (`agent_ns::wrapper::tests`, on any computer): plain names only; the
mark in the first 512 bytes; no path to the program in the wrapper; the
token only assigned and exported; the wrapper run with a stand-in
`unshare`, started in a run's folder: the token file read once and
deleted, the token, socket and SDK in the environment and never in argv, a
second start in the same folder refused, a link refused; a run-like folder
whose id is not plain, the project, the lamp's folder or one under a run's
folder: no token or `--net` (even with the caller's own, or the old
`LAMP_RUN_ID`), the program found past the wrapper and another marked wrapper; the
namespace script naming no engine, for every switch, with `HOME` per
engine; `sh -n` on every text; the writer's modes and its refusal to
replace a file that is not lamp-update's; no token file left when no
program is found; an engine found before the wrapper (or none found) not
present; the login lookup's argv; the programs look's argv (bounded,
fixed text, plain names only), run with a stand-in `PATH` listing only
the engine's own program, and its output read strictly.
`agent_ns::rounds::tests`: the switch
on only for `on`; only installed, plain engines other than Hermes and
OpenClaw written, a made-up one included; a second round writing nothing
and asking no ids, a changed wrapper or another switch written again; an
engine whose own program the login shell finds first said, not written
again; the engine's own program at the wrapper's place left and the rest
written; no ids, nothing written; the hook stand-in kept past Hermes's
removal; a line that stays the same said once, a changed one again, and a
refusal and no ids said once; a wrapper whose engine Harness no longer
says installed taken out once, its home and `agent-ns` kept; one with no
program of its own left taken out while Harness still says installed, and
not written back until one is there; one with a program in
`~/.local/bin` kept; a failed look keeping it, said once; only exactly
lamp-update's wrappers taken out (not the engine's own program, an edited
wrapper, a link, Hermes's or OpenClaw's), none before Harness was reached;
off, nothing taken out.

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
`enabled = false`, as Comfort break) that the lamp has turned on
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
  `--mouth-model` and `--seems-blendshapes` read, both on in the product's
  `lamp-eyes.service` since #2799) or the name's models. `install` fetches each that is
  missing (#811, from lamp-06c1) from `deploy/product/models.json`: where
  it goes, its URL, size and SHA-256, the runtime's library out of its
  pinned release archive with `tar`. Each is written under a temporary name
  and renamed into place only when it matches; the voice's two go beside
  its binary through `current/`, and a round carries them to the next
  build. Every model, the name part's own `name.onnx` among them
  (`1d238ca7…`, the model lamp-4ace and lamp-52e6 run), is in one release of
  ours, `models-2026-10-16`, with each model's licence beside it (the first
  builder on #811, 2026-10-16 13:58; the fifth made it). The same-voice
  check's CAM++ (`3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx`,
  `357a834f…`, 29,596,978 bytes, Apache-2.0) is in `models-2026-11-04`
  with its licence beside it, placed beside the voice through `current/`;
  the voice reads it only where `LAMP_SAME_VOICE_MODEL` names it, as the
  product's `lamp-voice.service` does at that path since 4 November, for
  the linked services alone ([same-voice.md](same-voice.md)). The voice's five
  recorded lines and their manifest are in `notices-2026-11-03`, placed in
  `/opt/lampOS/voice/notices` (#1844, [failure-notices.md](failure-notices.md)). The ears'
  YAMNet pair, `yamnet.onnx` (`c7e50179…`, 29,489 bytes) and `yamnet.data`
  (`26771b49…`, 14,917,924 bytes), our own conversion of Google's
  `yamnet.h5` (#6232, Apache-2.0, [sound-events.md](sound-events.md)), is in
  `models-2026-11-11`, placed in `/opt/lampOS/ears/models`, where the
  product's `lamp-ears.service` names it. Those three earlier
  releases are published (`models-2026-10-16` since 26 Oct); `models-2026-11-11`
  must be before a build listing it is promoted (lampos-promote refuses a
  model whose release or asset is absent). A model that cannot be
  had is said and the install goes on. What
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
    journal (`--since yesterday`). A first-time setup's is kept apart on
    the eMMC (#5144, "A first-time setup's journal" below), so the cap stays
    as it is: the RAM disk is 50M, and `orangepi-truncate-logs` vacuums the
    journal to 5M past 75% of it.
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
  - **How long they are kept (privacy, the PM's ruling on #5358, the first
    builder's P2 on #5342):** their lines can hold what a person said, so
    they follow the setup journal's rules (below). No file is kept past 72
    hours from when it was closed: the shutdown's `keep` copies none older,
    and `link` at boot and every `lamp-journal-setup` run (every 3 minutes)
    delete any older, with their links, each with a `lamp-journal-keep: a
    kept boot's file is deleted after 72 hours` line; nothing is deleted on
    a clock not yet set. A reset wipes them: both reset plans wipe
    `/var/lib/lamp-journal` whole, and `lamp-setup`'s reset then leaves
    `.forgotten-boot` with this boot's id there, so a reset that ends in a
    reboot keeps nothing of the old owner's boot at that shutdown (Twelve's
    P2 on #5365; a reset started in place empties the journal itself). The
    owner's own "forget everything about
    me" removes them with the setup's (`lamp-voice`'s
    `setup_journal::forget_with_everything`), with their links, and leaves
    `.forgotten-boot` holding this boot's id
    (`/proc/sys/kernel/random/boot_id`), so this boot's shutdown keeps
    nothing of it, even with nothing kept yet; at the next boot's shutdown the mark is spent and the
    keep goes on as before. Tests: `image_ramlog.rs`'s
    `a_boot_the_owner_forgot_is_not_kept_at_its_shutdown` and
    `the_kept_boots_are_kept_72_hours_at_most`; `lamp-voice`'s
    `setup_journal` tests; `lamp-setup`'s reset tests list both folders,
    and `a_reset_marks_this_boot_so_its_shutdown_keeps_no_journal`. The
    script tests run with a `journalctl`, `systemctl` and `pkill` that fail
    them if called, and with a `date` before 2026 that deletes nothing.
  - **Existing lamps:** a build carries both with the image's files
    (`LAMP_UPDATE_IMAGE=on`), then `systemctl daemon-reload`: the next
    shutdown keeps, and the boot after it links. By hand: copy both, then
    `sudo systemctl daemon-reload`.
- A first-time setup's journal, kept on the eMMC past the 20M cap:
  `robots/lamp/rootfs/etc/systemd/system/lamp-journal-setup.timer` and
  `lamp-journal-setup.service`, running `lamp-journal-keep setup` (#5144).
  So a setup step that failed can be read back hours or days later.
  - **Why:** a tester's setup on lamp-0c4e (3 Nov) was gone from the 20M
    journal within 4 hours, so the face step that failed couldn't be read
    back. A larger cap isn't the answer: the journal lives in the 50M
    `/var/log` RAM disk (zram) with the other logs, and past 75% of it
    `orangepi-truncate-logs` vacuums the journal to 5M anyway. Nor is a
    journald restart to change it, which broke every part's log stream on
    15 Oct.
  - **What it does:** every 3 minutes from a minute after boot, while the
    setup walk is under way and for 20 runs (an hour) after it ends, each
    of this boot's closed journal files (4M at most, an archived name with
    "@") is copied once to `/var/lib/lamp-journal/setup` on the eMMC, oldest
    first. "Under way" is read as `lamp-update`'s `walk.rs` reads it: setup
    done (`/var/lib/lampos-setup/done`), and the voice's onboarding file
    (`/var/lib/lampOS/voice/onboarding/progress.json`) not yet written or
    without an outcome for one of its 9 steps. Files closed before the walk
    was first seen hold nothing of it and are left. After the hour, the
    first file closed after it, which holds the walk's end, is copied, and
    nothing more. Every part's lines are in them, the voice's and the eyes'
    among them.
  - **How it is read:** each copy is linked into `/run/log/journal` as
    `lamp-kept-setup-….journal` once its original has left the journal
    (the cap's vacuum), and at every boot by `link`, so `journalctl -u
    lamp-voice -u lamp-eyes` reads the setup in the same boot and after a
    reboot, and never reads a line twice.
  - **How long it is kept (privacy, the PM's call on #5342):** its lines can
    hold what the person said at setup, so it is deleted, links and all,
    72 hours after the walk was first seen, at a reset (both reset plans
    wipe `/var/lib/lamp-journal`, #5358), or when a walk begins again (a
    redo of the walk or of a step), which replaces it, whichever comes
    first; the keep's run says "deleted after 72 hours". A walk first seen
    before the lamp's clock was set (before 2026) is dated anew once it is.
    The owner's own "forget everything about me", once their memory is
    emptied (the same mark and rule as their place and voice print),
    deletes it at once: `lamp-voice`'s `setup_journal::forget_with_everything`
    removes the copies and their links, with the last two boots' (#5358,
    above), the answer says `kept_logs: "forgotten too"` (or `everything:
    false` with the reason when one can't be removed), and it leaves
    `.forgotten`, so nothing more of that walk is copied, even a file being
    copied then; a redo after that walk ends starts anew.
  - **Its bound:** 24 MB on the eMMC, each file written once, stopping at
    the first that doesn't fit, so what is kept is the setup's start
    without a gap, with a `lamp-journal-setup:` line saying it is full.
    That is about 3.5 hours at lamp-4ace's busy rate on 17 Oct, and more
    than 17 hours at lamp-06c1's 1.1 to 1.4 MB an hour (22 Oct). Beside the
    last 2 boots' 10 MB, 34 MB of the eMMC at most. Nothing is added to the
    RAM disk; the links are in `/run`, a few bytes each. Idle I/O and
    `Nice=10`, so a copy never competes with the voice.
  - **Never journald:** the keep never rotates, restarts or signals journald,
    and the timer needs no enable: `orangepi-ramlog.service`, enabled on the
    base image, wants it (`zz-lamp-journal-keep.conf`, `[Unit]
    Wants=lamp-journal-setup.timer`). A lamp whose walk ended before the
    timer first ran keeps nothing.
  - **Tests:** `lamp-update`'s `tests/it/image_journal_setup.rs` runs the
    script on folders shaped as the lamp's, with a `journalctl`,
    `systemctl` and `pkill` that fail the test if called: nothing kept
    without a walk under way, each closed file kept once and linked once its
    original is gone, 24 MB at most, the hour after and the walk's end, a
    redo replacing it, the links at boot, deleted after 72 hours (and a
    start before the clock was set dated anew), nothing more of a forgotten
    walk; the script's steps are
    `lamp_msgs::ONBOARDING_STEPS`; and the units pinned, read by
    `systemd-analyze verify` where it exists. `lamp-voice`'s
    `setup_journal` tests: only the owner's emptied forget removes the
    copies and links (never #2602's), and one that can't be removed is
    said. `lamp-setup`'s reset tests: both plans wipe the folder.
  - **Existing lamps:** a newly imaged lamp has it. A build carries the
    three files with the image's files (`LAMP_UPDATE_IMAGE=on`, then
    `systemctl daemon-reload`), and the timer starts at the next boot; by
    hand, copy them and `sudo systemctl daemon-reload`, then reboot or
    `sudo systemctl start lamp-journal-setup.timer`. Not shown on a lamp
    yet: the lamp check in the pull request (a fresh flash, setup, 24
    hours) is owed.
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
- Bedtime and Morning (#1672): an installed skill, enabled since #5753.
  Host installation tests check that it is live with no function of its
  own, that the Skills and Actions units declare their private durable
  paths, and that `LAMP_OUTPUT_CAPS=on` is in the Actions, Skills and voice
  units (`the_product_runs_bedtime_and_morning_with_its_night_caps`). Its
  nights on lamp-06c1 are #2803 and #2804; a played row on the product
  build is still owed (#5753).

## Prepare-to-unplug core skill (#1674)

Install includes `skills/prepare-unplug/{SKILL.md,skill.toml}`. It stays hidden
and inactive unless `LAMP_UNPLUG=on` is set on Skills, Body, Expression and
Voice after the lamp checks in [unplug.md](unplug.md). The product sets it in
all four units since #5747 (lamp-52e6's row 1 on #1699, 20 Oct). This skill prepares the physical arm; it is not OS shutdown.
