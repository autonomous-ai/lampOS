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

Read on 2026-10-15 at `lamp-voice` `462d04cee`.

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
  a folder, never adds a part, never enables a unit, and carries files
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
| actions | `lamp-actions` | orangepi, `audio` | `/run/lampos-actions/` (0755) | none | its folder; the bus |
| memory | `lamp-memory` | orangepi (`@MEMORY_USER@`: its own) | `/var/lib/lampos-memory/` (0700); `/run/lampos-memory/` | none | `memory.sock` |
| policy | `lamp-policy` | orangepi (template: `DynamicUser=yes`) | `/var/lib/lampos-policy/` (0700) | the judge's key, `/etc/lampOS/policy/jev.key`, as a systemd credential | the bus |
| health | `lamp-health serve` | orangepi | none | none: what it expects is options | the bus |
| controls | `lamp-controls serve` | orangepi | none | `controls.json` and `controls-pins.json`, inside the build's folder | the bus |
| ears | `lamp-ears` | orangepi, `audio` | none | none | the bus |
| schedule | `lamp-schedule serve` | orangepi | `/var/lib/lampos-schedule/` (0700); `/run/lampos-schedule/` (0700) | none | `call.sock` |
| air | `lamp-air` | orangepi, `/dev/i2c-0` | none | none | the bus |
| air's bus clock | `lamp-air limit-clock --bus-number 0` (a oneshot, before air and controls) | root | none | none | I2C bus 0's `device/freq` |
| tasks | `lamp-tasks serve` | orangepi | `/var/lib/lampos-tasks/` (0700); `/run/lampos-tasks/` (0700) | `targets.json` in its state folder; the Harness program in the user's home | `request.sock`, `voice.sock` |
| records | `lamp-records check` (a timer, hourly) | root | `/run/lamp-records/` | `/etc/lampOS/records.json`, which install places (`deploy/product/settings/records.json`; it does nothing without it) | `last.json` |
| selftest | `lamp-selftest --occasion first-boot` (a timer: 5 min after the lamp starts, then every 6 h; #812) | lampos, `CAP_SYSLOG` | `/var/lib/lamp-selftest/` (its own `StateDirectory`) | none | the bus (`lamp/selftest`), its runs' reports |
| update | `lamp-update serve` | root | `/var/lib/lampos-update/` (0700) | `/etc/lampOS/update/key.pub` | none |
| keeper | `lamp-keeper serve` | root | its service data folder | none | `lampos.target.d/keeper.conf`; the bus root |
| skills | `lamp-skills` | lampos (product unit, #811) | none | the skill folders, `/var/lib/lampOS/skills/` (Welcome back from install) | its tools, `/run/lampos-actions/skills/` |
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
| 3 | body, name, controls, policy; **ears, air** | Ears and air only publish on the bus, as name does |
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
  --playout-start-ms 350 --ring-channel-ceiling 24 --end-silence-ms 600
  --controls-stop --endpoint smart-turn-fast --call-wait-ms 2000`. On
  17 October `bounded` replaced `off` (lamp-cb94's voice ended on a held-up
  speaker with `off`; bounded recovery kept it up, #1298) and the call's
  wait came from lamp-4ace. The template's
  `@VOICE_OPTIONS@` calls them "the measured options of this lamp"; on two
  lamps they are one set.
- Every option of name, ears, air, actions, memory, schedule and controls
  (`lamp-name --microphone mic2 --least 0.9`, `lamp-ears --device
  plug:device_micro1 --rise-db 30`, `lamp-air --bus-number 0`,
  `--mixer-card allwinnerhdmi --mixer-control Speaker --volume-ceiling 100`,
  `--keep-days 30 --memory-days`, …), and `CPUAffinity=6 7` (the board's
  two fast cores; keeper.md).
- The body's speeds and limits: `--reach 150,15,1,250 --speed 120
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

The camera's USB path, `--usb 01da:5875@3-1.1.4`, is the same on both: a
property of the board and its cable, so the product's, as long as every
lamp's camera goes in the same port.

**Different: one lamp behind the other.** lamp-4ace has switches lamp-52e6
lacks: the voice's `LAMP_TASKS_DIR`, `LAMP_SCHEDULE_TOOLS`,
`LAMP_SCHEDULE_SPEAK`, `LAMP_SCHEDULE_DELIVERY_DIR`, `LAMP_MEMORY_DAYS`,
`LAMP_EYES_WHO_SOCKET`, `LAMP_SPEAK_FIRST`; the actions'
`LAMP_SCHEDULE_TOOLS`; health's `--expect policy`. By joining.md's rule
("The switch is for that first minute, not for life") a switch on for
lamp-4ace becomes the product's default, so the product takes lamp-4ace's
set, but for `LAMP_SPEAK_FIRST`: the product's hello is the Welcome back
skill (`lamp-skills`, #811), spoken through the voice's say service, so the
voice's own hello is off, as lamp-4ace now has it too.

**17 October: lamp-4ace's switches of that day became the product's** (joining.md's rule), taken from lamp-4ace's unit as it ran that day; most were switched on there on 16 or 17 October, several only as part of the groups of #1238's census, so their lamp record is that day's running, not a run of their own. They're the switch groups (lead-in to a tool, a waited answer kept, the stop told, the asker first only recorded, names checked, the staff note, task news at a pause, memory rules), the light requests (the voice's ring and the skills' half, `lamp-skills.service`), the quiet pace, starting offline, the asleep ring, a sorry in another language, the lamp's name in passing ignored, first words kept, tasks asked and who asked, a cut keeping the name, the self-test's speaker, the stall recorder, the strong ring eye, and short sounds judged in a quiet room after a check. Not taken: those that need the second voice's configuration (stop words, its down-line and wait cue, the voice link), which a product lamp doesn't have; the office's own (its Harness target, its say group); head touch, which needs Controls' and Expression's flags as well (#711); those that need another part's flag the product doesn't pass (the Policy holding actions needs `lamp-policy --act attention`, its hello and progress news need `--hello` and `--progress`, the full look needs the eyes' `--full-look`, a restored mute needs `--controls-mute`); and "that was weird", a builder's review tool whose marks (`/var/lib/lampOS/voice/marked/`) the reset doesn't wipe. Expression's catalogue differs too (`expression-alive-name-speech` on
lamp-4ace, `expression-alive` on lamp-52e6): the product takes lamp-4ace's.

**Neither: the person's.** The time zone (#820). Setup links `/etc/localtime`
to the zone the person's phone gives and names it in `/etc/timezone` (#806).
Memory, Expression and skills take `--zone system`, which follows that file,
across daylight saving and without a restart (skills since #1384). Schedule
takes `--zone ${LAMP_ZONE}` from `zone.env`, which install writes from the
image's zone and setup writes again with the person's. The voice
tells the time from `/etc/localtime` itself. The policy keeps none: only its
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
  build's folder, so an update changes the link and not the unit.
- **What a build does not carry**, never inside a build's folder: models in
  `/opt/lampOS/<part>/models/` (the eyes' two, the name's), beside
  `current`; the product's settings in `/etc/lampOS/<part>/` (decided by the
  first builder on #811, 2026-10-15 17:37): Expression's catalogue as
  `/etc/lampOS/expression/expression.json`, the controls' two files in
  `/etc/lampOS/controls/` (the pins name the button, the two head pads and
  the reset button, line 100; the unit serves `--touch`, the path whose
  reset event `watch-reset` reads. The product's voice and Expression do
  not read the head's `controls/touch` yet, so `legacy_head` keeps a pat
  the old engine's `Pet`, which wakes a lamp asleep at night; a head tap
  does not stop the voice on a product lamp, #1167). A pins file changed by
  hand is kept, and must pass the touch path's checks, which are stricter
  than the old reader's (chip 0, the button on line 99, head pads on 37,
  96, 97 or 98), or Controls does not start, and the button with it. `/etc/lampOS` is made 0755 and each of these
  folders 0755, files 0644: readable by `lampos`. A default found with install's own bytes but another mode is install's, and is given its mode (lamp-cb94's settings were there before any install at 0600, and Controls and Expression could not read them, #705); a lamp's own setting found closed is kept as it is, and said. lamp-4ace's `/etc/lampOS`
  is root's alone; `install` does not open one made closed (credentials may
  be below it) and says so. The voice's turn-end model and ONNX
  Runtime library stay beside the voice, where the voice looks for them,
  and lamp-update carries them from build to build as it does now.
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
  home; and `/var/lib/lampOS/voice/sessions` (root, 0700), which the voice's
  unit names and nothing else makes. Both were found on lamp-06c1 on
  2026-10-16: with the locks 0775 the body never opened the servo port, and
  without the sessions folder the voice could not start.
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

`install` puts lampOS on a lamp; it does not make the image. It expects to
find, from the third's list on #811 (2026-10-15 17:07) and from what it
reads itself:

- Debian bookworm (arm64) with systemd, `useradd`, `systemd-tmpfiles` and
  `date`; the groups `audio`, `video`, `dialout` (and `i2c`, `gpio` if the
  board has them).
- The update trust root, `/etc/lampOS/update/key.pub`, put there at the
  factory. Setup does not write it.
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
  enter setup.

## Setup at first boot, without the keeper

The keeper honours `--setup-marker` only when it runs as the old updater's
package (#816); a lamp installed by `install` has no keeper bring-up. So the
product does it with systemd alone (#811):

| Unit | Runs | Does |
|---|---|---|
| `lamp-setup.service` | at boot, `WantedBy=multi-user.target`, only while `/var/lib/lampos-setup/done` is missing and setup's two settings exist | the hotspot and V1's setup page until the home's Wi-Fi and the person's settings are saved; then writes the marker |
| `lampos-setup-done.path` | at boot, `WantedBy=multi-user.target` | starts `lampos.target` once the marker exists: the moment setup writes it, and at every later boot |
| `lamp-setup-reset.service` | in the target, after setup | the reset button held 5 s: stops the lamp, wipes, removes the marker, reboots into setup |

Setup and its reset start the same binary, `/opt/lampOS/setup/current/lamp-setup`.
`install` writes and enables all three and starts none.

## The install command

```text
lamp-update install --channel-url URL [--channel NAME] [--key-file PATH] [--setup on|off]
```

As root, on a lamp with the base image (`crates/update/src/install.rs`):

1. Reads the channel's newest build and checks it with the image's key
   (`/etc/lampOS/update/key.pub` unless `--key-file` says otherwise), as a
   round does. No key, no install.
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
   it.
5. Writes each product unit and `lampos.conf`, each only if it is not
   already exactly that. A unit, `lampos.conf` or setting found already
   holding install's exact bytes is given install's mode (0644; the voice's
   provider, its key and the Policy's key 0600, so a key is only ever
   narrowed), read and changed through one open of the file, never through
   a link: a link is left as it is (#1293). A lamp's own setting found
   closed to other users is kept as it is and said: `PATH is closed to
   other users (600): the parts run as lampos cannot read it`. The product settings (Expression's catalogue, the
   controls' two files, and the first skill, Welcome back, in
   `/var/lib/lampOS/skills/welcome-back/`: its `skill.toml`, `SKILL.md` and
   light, not its tests) are written only where one is missing or is still
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
   never used to put it back (said instead). Both files root's and 0600, as
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
   (`systemd_owed`) is recorded earlier still, before the first unit or
   `lampos.conf` is written (step 5), and after anything else changed.
8. If systemd is owed: `systemd-tmpfiles --create`, `systemctl
   daemon-reload`, and `systemctl enable` of every part, `lampos.target`,
   `lamp-update.service`, `lamp-records.timer` and `lamp-selftest.timer`
   (its service is started by the timer alone, as the records' check is);
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
   tasks unit's state folder), runs `loginctl enable-linger lampos` (a user
   manager for `lampos` from boot, with no login), then the Harness CLI's
   own `harness service install` as `lampos` with
   `HOME=/var/lib/lampos-tasks` and its `XDG_RUNTIME_DIR`. No unit of
   install's own: the daemon's runtime, entry point and platform variable
   are the Harness team's (docs/harness.md). A refusal stops the install,
   records nothing, so the next install asks again, and is said; it is taken to the Harness team, not worked around. How a
   home lamp's Harness signs in is theirs too.

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
```

- **Each product unit** is `same`, `missing on the lamp`, or `differs` with
  each differing line, `-` the product's and `+` the lamp's. Install writes
  every unit whole into `/etc/systemd/system/`, with no `@…@` placeholder
  (`install::tests::no_product_unit_names_a_lamp`), so units are compared
  byte for byte, comments included: a changed comment is a file that isn't
  the product's.
- **Every drop-in** (a file under a `.d/` folder) is said with its text: on a
  product lamp each one is a finding. After the office extras go back, the
  check must name `office.conf` and nothing else.
- **Every lamp unit the product has no file for** is said (`not in the
  product`), and a product unit read from anywhere but
  `/etc/systemd/system/` (`not where install puts it`).
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
  eyes' two models or the name's models. `install` fetches each that is
  missing (#811, from lamp-06c1) from `deploy/product/models.json`: where
  it goes, its URL, size and SHA-256, the runtime's library out of its
  pinned release archive with `tar`. Each is written under a temporary name
  and renamed into place only when it matches; the voice's two go beside
  its binary through `current/`, and a round carries them to the next
  build. Every file, the name part's own `name.onnx` among them
  (`1d238ca7…`, the model lamp-4ace and lamp-52e6 run), is in one release of
  ours, `models-2026-10-16`, with each model's licence beside it (the first
  builder on #811, 2026-10-16 13:58; the fifth made it). It is a draft until
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
- `lamp-cmedia-speaker.service` (the speaker's level, from V1) is the
  image's, not the product's: `robots/lamp/rootfs/etc/systemd/system/`, at
  step 9 (−28 dB), with the `pro` override at step 6 (−31 dB). udev starts it
  when the C-Media card appears (`99-lamp-device.rules`). It sets the card's
  hardware level and creates `/etc/asound.conf`'s softvol "Speaker"
  control. `install` sets no level of its own. The product's voice unit is
  ordered after it, without wanting it (El on #705, 17 Oct). On lamp-06c1
  it is byte for byte the image's.
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
  - **Tests:** `lamp-update`'s `tests/image_journal.rs` checks the numbers
    against the disk. Where `systemd-analyze` exists, it checks that systemd
    reads them last beside a 100M `persistent.conf`. lamp-75c9's systemd 252
    reads it last after its own `journald.conf` too.
  - **Existing lamps:** only a newly imaged lamp has it. `install` and
    updates don't place the image's files (#1369). On a lamp already in use
    it is copied there by hand, and journald reads it when it next starts,
    at the next boot. `journalctl --vacuum-size=20M` shrinks the journal at
    once, with no restart of anything.
- Which group may read the GPIO chips and the I²C bus on the base image was
  not checked.
- `lamp-surround` was not read beyond its unit, and `lamp-records` has two
  unit texts (lamp-4ace's names `/opt/lampOS/records/`, the crate's
  `/opt/lampOS/bin/`), while `deploy/units/README.md` says it has no unit on
  any lamp.
- `lamp-skills` (#811, from lamp-06c1): installed with Welcome back and the
  voice's say service (`LAMP_SAY=on`, its socket in the group `lampos`), the
  voice's own hello (`LAMP_SPEAK_FIRST`) off so the lamp greets once, and
  `LAMP_SKILLS_ASK=skip` until the Policy's gate (#939). Only in tests: no
  lamp has run the product unit yet.
