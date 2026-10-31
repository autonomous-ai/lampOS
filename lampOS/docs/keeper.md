# The keeper: one lamp of many parts

The lamp is a dozen processes, each its own systemd unit
([brain-build.md](brain-build.md), "Components, processes and owners").
This page says how they are started, watched, paused and stopped as one
lamp: the units and their order, the slices, the restart limits, the
watchdog each part keeps, and the states each part says. The units are in
[`deploy/units/`](../deploy/units/README.md). The program that acts on
them, `lamp-keeper` (`crates/keeper`), is described with its crate.

## What exists and what we take

**systemd** is the process manager, and nothing here replaces it. A
*target* groups units; `Wants=` pulls a unit in, `After=` orders it, and
`PartOf=` carries a stop or restart of the target to the unit (not a
start). `Type=notify` lets a process say `READY=1` when it is ready rather
than when it has forked, and `STOPPING=1` when it begins to stop.
`WatchdogSec=` expects `WATCHDOG=1` at least that often; a process that
misses it is ended (SIGABRT) and, with `Restart=on-failure`, started again.
`StartLimitBurst=` starts in `StartLimitIntervalSec=` and the unit stays
failed (`start-limit-hit`) until someone resets it. *Slices* are the cgroup
tree: `CPUWeight=` shares the processor between slices, `Nice=` orders
processes within one. Taken: all of these, as they are. Not taken:
`StartLimitAction=reboot` (below), and `systemctl freeze`, which stops a
process where it is: a frozen body holds its servos at their last goal with
nothing watching their load or heat, so it is not a pause.

**ROS 2 managed (lifecycle) nodes** have named states (unconfigured,
inactive, active, finalized) and transitions driven by a manager; Nav2's
lifecycle manager also keeps a heartbeat (a "bond") with each node and
brings the set down when one is lost. Its **diagnostics aggregator** groups
every node's `/diagnostics` into one view with OK, WARN, ERROR and STALE.
Taken: a few named states each part says, and a pause ("inactive") that is
not a stop; the heartbeat, as systemd's watchdog. The aggregator was
already taken, as `lamp-health` ([health.md](health.md)). Not taken: the
configure and cleanup transitions and a manager that drives each node
through them. systemd starts a process; a part configures itself as it
starts, and a part that cannot is not ready.

**Android's init** reads `.rc` files of services, each in a *class*
(`core`, `main`, `late_start`) started and stopped together with
`class_start` and `class_stop`. A service is restarted when it exits, after
`restart_period` (5 s unless set). A `critical` service that exits more
than four times in four minutes, or before boot completes, reboots the
device into the bootloader; repeated crashes of the system server lead
Rescue Party to offer recovery. The system server's **watchdog** thread
checks that its main threads still turn over and kills the process when
one has been held for a minute, after dumping its stacks at half that.
Taken: the class, as `lampos.target`; restarts with a bound. Not taken:
rebooting on repeated failure. A lamp that reboots does not talk any more
than one with a part stopped, and what the servos do at power-up has not
been checked on any lamp (lamp-4ace runbook, "Do not"). A part that will
not stay up is stopped and reported.

**launchd** keeps a job running with `KeepAlive` (always, or on conditions:
it crashed, a path exists, the network is up) and will not start a job
again within `ThrottleInterval` (10 s) of the last start. It has no order
between jobs: launchd creates each job's socket itself and a client waits
on the socket, not on the job. Taken: the throttle, as `RestartSec=`, and
the lesson that order is a convenience, not a dependency. Every lampOS part
already does without a neighbour that is not there (brain-build.md, "No
component waits on another"); the order below only keeps early messages
from being lost. Not taken: conditional keep-alive.

## The parts and their order

`lampos.target` is the lamp. Every part is `PartOf=lampos.target` and
`WantedBy=lampos.target`, so one command starts or stops all of them:

    systemctl start lampos.target
    systemctl stop lampos.target

and `lamp-keeper start lamp` does the same, and `lamp-keeper stop lamp`
with a pause first (below). The target is active when every part it wants has
started; for a `Type=notify` part that means it has said it is ready, so
the target's start time is the lamp's "power-up to every part ready"
once every part notifies.

Start, in this order (each line after the one above it; parts on one line
together):

| Step | Parts | Why |
|---|---|---|
| 1 | memory, actions, controls | The voice reads what is kept before its first session and calls the actions for its functions. It starts after Controls, so at a shutdown it stops before Controls (#3697) |
| 2 | voice | The lamp's reason to be: listening as early as it can. The ring is the voice's own, so "the light" comes with it |
| 3 | body, name, policy | The body's mood comes from the voice; name and policy read and write the voice's topics |
| 4 | eyes, Expression | The eyes send each look to the body's gaze socket; Expression's requests go to the body and the ring's owner |

Health is ordered against none of them: it starts and stops with the
target, at no fixed point in either order.

Stop is the reverse: eyes and Expression first, so nothing is sent to a
body on its way down; then the body, which folds before it ends (its
`TimeoutStopSec=25s`), with name and policy; then the voice; then
memory, actions and controls.

What is known and what is not:

- **Body before eyes**: checked in the code. `lamp-eyes` sends each look as
  a datagram to `--body-socket` and ignores a failure to send
  (`crates/eyes/src/main.rs`), so eyes without a body work and their looks
  are dropped. With the body `Type=notify`, the eyes start once the body is
  ready, and its gaze socket is there for the first look.
- **Voice before body**: as on lamp-4ace since 2026-10-13. Nothing needs it:
  the voice sends moods without waiting. It is chosen so that the lamp can
  talk before it can move, and a slow body start does not delay listening.
- **Memory before voice**: from memory.md (what is kept is read before the
  first session). While memory is `Type=simple` this orders only the
  forks; once memory notifies, the voice waits for it, and how long that
  delays the first listening is something only a lamp shows.
- Only `Type=notify` parts make an `After=` wait for anything. Today that is
  the body, so the only real wait in the chain is the eyes and Expression
  on the body's start checks. How long those take at power-up, and whether
  the eyes should wait for them, only a lamp shows.
- Surround is not part of the target: an update stops and starts the lamp,
  and what asked for the update is not stopped with it. The keeper's own
  unit is outside the target for the same reason.

## Slices and priorities

| Slice | Parts | Weight | Why |
|---|---|---|---|
| `lampos-voice.slice` | voice | `CPUWeight=1000`, `IOWeight=1000` | A late word is worse than anything else late. The voice's microphone and speaker threads are also round-robin real-time at 20 (lamp-4ace, 2026-10-13: with every core busy the voice ended four times at ordinary priority) |
| `lampos-parts.slice` | every other part | `CPUWeight=100` | Together no more than one ordinary service. Within it: eyes `Nice=10` (looking is never worth a late word); name, Expression, policy, health `Nice=5`; body and memory ordinary; controls `Nice=5` in these units, and in the product unit (`deploy/product/lamp-controls.service`) its main loop and strip child round-robin real-time at 30, above the voice, so the switch and hand can be read while speech is busy. Each loop rests for at least its thread CPU work; reset-on-fork stays unset (#2602, [controls.md](controls.md)) |

The name also keeps to the board's fast processors (`CPUAffinity=`), as on
lamp-4ace.

## Restart limits

| Part | Restarted after | Limit | Then |
|---|---|---|---|
| voice | 4 s | 10 starts in 10 min | stays failed |
| body | 15 s | 3 starts in 10 min (as on lamp-4ace) | stays failed until someone has looked: a body that stopped itself three times has found something wrong with the lamp |
| every other part | 5 s | 5 starts in 10 min | stays failed |

A part over its limit is `failed` with `start-limit-hit`. `lamp-keeper
status` shows it, and it is the keeper's to report on the bus. `systemctl
reset-failed UNIT` and a start try it again. lamp-4ace's voice unit has no
limit (`StartLimitIntervalSec=0`); the template's ten in ten minutes is a
choice not yet tried on a lamp.

## The watchdog each part keeps

Through the keeper's helper (`crates/keeper`), each part:

1. says `READY=1` when it really is ready, not when it has started: the
   body once all five joints answer, its start checks have passed and its
   loop runs; the voice, when it can hear; the eyes, when the camera can be
   opened on a permit.
2. says `WATCHDOG=1` from its main loop, once per turn of it, and never from
   a timer thread of its own: a timer goes on while the loop it should
   vouch for is stuck. At least twice per `WatchdogSec`.
3. says `STOPPING=1` when it begins to stop (the body, as it begins to fold).

Without `NOTIFY_SOCKET` (a part run by hand) the helper does nothing.
A part's unit becomes `Type=notify` with a `WatchdogSec=` only once the
part calls the helper; a `Type=notify` unit whose process never says ready
fails its start after `TimeoutStartSec` (90 s). Today only the body does:
`WatchdogSec=5`. A body whose loop is silent for five seconds is ended and
started again after 15 s, within its limit. Its servos hold their last goal
meanwhile, with torque on; it does not fold.

## Named states and the pause

Each part says one of four states on the bus:

| State | systemd | Means |
|---|---|---|
| starting | activating | Begun; not yet able to do its work |
| ready | `READY=1` said | Doing its work |
| paused | (none) | Running, ready, and doing no new work |
| stopping | `STOPPING=1` said | Ending; what it holds is being put down |

systemd has no pause. Each part honours one itself, when the keeper asks:

| Part | Paused |
|---|---|
| voice | Takes no new turn. A turn under way is finished, the person is not cut off. The microphone stays as the privacy switch has it |
| body | Goes to rest (folds, if it stands), sends no new goal, and holds there |
| Expression | Holds still: no new requests; those already sent run out |
| skills | A run under way sends no further step: no next clip, a show stops before its next frame, a search ends |
| addons | The judge drops every suggestion: `the lamp is paused` |
| eyes, name, memory, actions, policy, controls, health | Nothing: none of them moves or speaks by itself |

Resume undoes it. A stop by the keeper is: pause, wait until the voice has
been quiet (task 12's 45 s, [surround.md](surround.md), "Startup and first
setup"), then `systemctl stop lampos.target`. `systemctl stop` alone
does not pause: the body still folds on its way down, but the voice may be
ended mid-word.

**How it is asked.** `lamp-keeper pause` says `{"paused": true}` on
`lamp/pause`, a latched topic ([msgs.md](msgs.md)); `lamp-keeper resume`
says false. The value is kept on the bus until the next, so a part reads it
whenever it looks, and a part started while the lamp is paused starts
paused. `/run` is not kept, so a power-up is never paused. The factory's steps end a passed self-test with `lamp-keeper pause`, so a
lamp leaves them folded and resting until it is next started (install.md,
#2818). `lamp-keeper stop
lamp` pauses, waits up to 45 s for `voice/state` to say resting, stops
`lampos.target`, and then clears the pause, so that the lamp is not paused
when it is next started. Setup's shutdown says `lamp-keeper pause` too,
before it stops the target ([shutdown.md](shutdown.md)); a lamp with no
keeper is said so, and its body rests as it stops.

**Who honours it now.** The body: it reads the kept value at most once a
second; paused, every mood it is told is rest, and gestures, looks,
Expression's requests and calls are read and dropped, so nothing from
before the pause is acted on after it. A standing body folds once its
awake time has run out (`--awake-seconds`, 90 s on lamp-52e6), as it would
with nobody there; it is not made to fold sooner. Its report on
`diagnostics` says `state` `paused` instead of `ready`.

The voice and Expression read it the same way, with the body's own reader
(`lamp_body::pause_in`): the kept value at most once a second, a file read
and no more; nothing kept, or a bus it cannot reach, is not paused. The
voice asks once a turn of its loop. Paused, a new sound takes no turn,
whether or not one is under way (`input_not_taken_paused`, once a pause,
in its session record), and it begins none itself: no hello, no news, no
reminder, no
look it said it would take and no second look at a request made by name.
Those wait, as they wait while the lamp is in a turn, and each keeps its
own expiry. A turn under way, and its answer, are finished, and someone
still speaking after an end sent early goes on with the same request.
`voice/state` then says resting as for any finished turn, which is what
`stop lamp` waits for. A change is a line in the journal and `keeper_pause`
in the session record. Expression, paused, publishes nothing on
`expression/body` or `expression/light`; what it sent runs out with its
200 ms lease, and the next request after resume is the planner's own.
With nothing said on `lamp/pause` both do exactly what they did before.

Skills and addons read `lamp/pause` too. A skill's run looks at it as it goes
(`body_stopped` in `lamp-skills`): paused, it sends no next clip and no step
after it, a light show stops before its next frame and a search ends, each
saying why, `the lamp was paused` ([skills.md](skills.md)). The addons
judge drops a suggestion while the lamp is paused, as while it is private or
asleep ([addons.md](addons.md)).

## A restart asked from the app (#2193)

`lamp-cloud` no longer asks here: a product lamp has no keeper, so the
app's `system.reboot` and `system.shutdown` go to setup's change helper
([cloud.md](cloud.md), "A restart from the app"; setup.md, "Changed after
setup"), which answers with this helper's code (`lamp_keeper::restart`:
the requests, the answers and the update lock, one copy). This helper is
kept for a lamp that runs the keeper and asks it by hand.

It was reached through `lamp-cloud`, which runs as `lampos` with no
capabilities and so cannot restart anything itself.
`lamp-keeper-restart.socket` (root's, group `lampos`, mode 0660, one
connection at a time) starts `lamp-keeper restart-asked` as root for each
connection (`lamp-keeper-restart@.service`), with the connection as its
standard input and output. It reads one line, which must be
`{"restart":"lamp"}`, and:

- **refuses while an update installs**: it takes lamp-update's own lock
  (`/var/lib/lampos-update/lock`, `Records::lock`); held by a lamp-update
  at work, it answers `{"refused":"an update is being installed: try again
  once it is done"}`. A lock it cannot check is refused too, with why;
- else answers `{"done":{"restarting":true}}`, waits 3 s so the asker's
  reply leaves the lamp, and then restarts as `stop lamp` stops: pause,
  wait up to 45 s for the voice to rest, `systemctl restart lampos.target`,
  clear the pause. It holds the update's lock to the end, so no update
  starts under the restart.

Never the machine: the keeper does not reboot (above, "Not taken"). The
socket is wanted by `lampos.target` but not part of it, as the keeper's own
unit, so the restart it starts does not stop it.

**Who may ask:** the switch, `LAMP_APP_RESTART=on`, is `lamp-cloud`'s;
the helper reads none. The socket trusts the `lampos` group, the same
trust as `lamp-setup-change.socket`, and doesn't check which part asks:
once the units are installed, any part running as `lampos` can ask for a
restart, one at a time. The two units are in `deploy/product/` but not
yet in `lamp-update`'s install list. Whether the app's restart should be a
machine reboot, as V1's is, waits on the first builder's answer on #2193.

## Switches

Every action the keeper takes on its own is behind a switch, off unless
set: without it the keeper reports and does nothing. A restart asked on
`lamp-keeper-restart.socket` has none of the keeper's: its switch is
`lamp-cloud`'s (above, "A restart asked from the app"). Copying the units in starts
nothing; each part is in the lamp once enabled, and the lamp starts at
power-up once `lampos.target` is enabled. Nothing restarts the voice while
someone speaks to the lamp: the keeper's restarts of the voice, and every
update and reset, wait for the quiet above. systemd's own restart of the
voice comes only after its process has ended, when nobody is being heard.
Once the voice keeps a watchdog, a voice whose loop has hung is ended
whether or not someone is speaking; a hung voice hears nobody either.

## One service for the lamp

To the rest of the machine the lamp is one service. The old runtime's
updater (the parent repository's `docs/bootstrap-ota.md`, explicit service
replacement) installs a package that carries `bin/service` and `VERSION`,
runs `bin/service serve` under a unit of its own, stops `hal` and
`os-server`, and asks `bin/service health` for
`{"ready","version","pid"}`. For lampOS that package is the keeper:
`bin/service` is `lamp-keeper`, and the parts' units travel in the
package's `units/` folder.

Started that way (`AUTONOMOUS_SERVICE_NAME` set), `lamp-keeper serve`:

1. writes an answer that is not ready, so that one left by the run before
   is never taken for this one;
2. clears what the old runtime leaves behind (below);
3. links every unit in the release's `units/` (`systemctl link --force`),
   writes `lampos.target.d/keeper.conf` (`BindsTo=` and `After=` its own
   unit), reloads, and enables each part, which puts it under the target;
4. starts `lampos.target`, and from then on writes after every look whether
   every part the release carries is running, with its own process id,
   which `health` prints.
5. points `/usr/local/bin/lamp-keeper` at its own `bin/service` (a new
   link under a temporary name, renamed over the old one), so that the
   setup's reset can run `/usr/local/bin/lamp-keeper stop lamp` by one path
   whatever the version.

### Prepare: the device key before the old runtime stops (#701)

The `lamp-os` package the updater installs is built in the parent
repository (`scripts/release/lamp_os_package.py`), and its `bin/service` is
that repository's adapter, which runs `bin/lamp-keeper serve` as a child
without `AUTONOMOUS_SERVICE_NAME`; the adapter's own `cleanup` removes what
it made (its units live under `/run`). The updater refuses an initial
`lamp-os` migration without `SERVICE_PREFLIGHT=1` in the package and a
`bin/service prepare`; neither is in the parent's package yet. What lampOS
gives that prepare is one command, `lamp-keeper prepare`.

`lamp-keeper prepare` runs while the old runtime still owns the lamp. It
stops nothing and touches no hardware and no unit. It provisions the device
key with `lamp-update install`'s own code (`install::provision_device_key`):
V1's `llm_api_key` from `/root/config/config.json` into
`/etc/lampOS/voice/credential` and `/etc/lampOS/policy/jev.key`, 0600 (set,
not left to the umask or to a file left beside), written whole beside and
renamed, root's as the updater runs it, and only where the file is missing:
one already there is never replaced, and with both there the config is not
read. It records what it wrote in lamp-update's records
(`/var/lib/lampos-update/state.json`), as install does, and follows
install's rule (#1102; the key is the user's and a reset wipes it, #806): a
key written once, by install or by prepare, and now gone is not put back
from V1's config, which may still hold the previous owner's; it says so and
leaves the key to setup. It reads the records as install does
(`Records::state`, #1094's review): a corrupt `state.json` is set aside as
`state.json.corrupt-<ms>` and said, and since what was written before is
then not known, V1's key is not copied; every key path, missing or there,
is recorded as once written (`install::SET_ASIDE`, marked by
`Records::state` as it sets the file aside, so a round that finds the
damage first saves the mark too), so neither the next
prepare or install, nor one after a later reset, copies it (Third and the
reviewer on #1263). If that mark cannot be saved (records damaged, then
unwritable), prepare exits 1 and that update is refused; the next run,
finding no `state.json` but a `state.json.corrupt-*` beside it, takes the
records as set aside again, so it copies nothing either. One that cannot be read
is said and left alone, with nothing written and exit 0. A file missing and no usable key in V1's config:
nothing written, it says so and exits 0, as install does, and setup takes
the user's key. Main's updater runs `bin/service prepare` before every
`lamp-os` update, so a refusal here would refuse every update, not only
the migration. It prints the paths it wrote, never
the key, not even its length. Checking the release (unit templates filled,
programs present) is the adapter's, which renders the units.

Then prepare writes what a V1 lamp lacks for the generic package (#2163,
`profile::prepare`), each only where it is missing, so a lamp that has it
keeps its own:

- `/etc/lampOS/package-profile.json` (0644), which the adapter fills the
  units from. The hardware is found by reading only: the one CH340 servo
  adapter under `/dev/serial/by-id`, the one USB camera's index-0 video node
  and its `vendor:product@usb-path`, the microphone and speaker cards in
  `/proc/asound/cards` (the speaker's `Speaker` control asked with `amixer
  sget`, a read), the CPUs with the highest top frequency, and the `cpu` and
  `skin` thermal zones. A lamp with `/etc/lampOS/body/lamp.env` (this lamp's
  own, from lamp-body-measure) has its `BODY_POSES` there too, for older
  packages' body units, which took the poses in the unit; without a measured
  `--gaze` the model's `755,555` is used there, and the profile says
  `"gaze": "default, not measured"`. A lamp without one is not refused: the
  generic body unit reads its poses from the files, and
  `lamp-body-measure.service` reads them seated, at rest, after cutover
  (#2163, `deploy/units/README.md`). That profile has no `BODY_POSES` and
  says `"gaze": "not measured"`.
- The settings the package carries in `provision/`: the voice's
  `provider.json` (0600), and the controls' `controls.json` and
  `controls-pins.json` (0644), the pins without `reset` and `legacy_head`,
  which only `--touch` takes, and without `privacy`, which only
  `LAMP_PRIVACY_CONTROLS=on` takes (#2485): the generic controls refuse
  either and would not start. A lamp moved before #2485 keeps a pins file
  with `privacy`, since prepare writes only a missing file. lamp-a63e was the
  only one, and was fixed by hand on 21 Oct. **A moved lamp never gets
  `reset`**, whatever its own V1 wiring says (#2937, the first builder on
  #2975 after El's P1): V1's wired head pad idles high and goes low on a
  touch (`hal/drivers/ttp223.py` in the parent repository), as a pulled-up
  button does, so no idle read tells a pad on line 100 from the reset
  button, and a hand on the head must never wipe a lamp. Its way back to a
  new Wi-Fi is setup's hotspot ([setup.md](setup.md), "When the saved
  network is gone"), until a real V1 pad on line 100 has been measured.
- Setup's page and reset plan (#2937), `/etc/lampOS/setup/serve.json` and
  `reset-plan.json` (0644): the pair `lamp-update install --setup on`
  writes (`serve-own-page.json`, `reset-plan-product.json`), carried in the
  keeper itself, not in `provision/`. With the plan, setup's marker
  (`/var/lib/lampos-setup/done`, empty, 0600, in a 0700 folder): a moved lamp
  got the package over its home's Wi-Fi, so it is set up, and the move never
  opens a hotspot. A lamp with the plan and no marker was put back into setup,
  and prepare writes no marker for it ([setup.md](setup.md), "A lamp moved
  from V1"). A plan that is byte for byte one an earlier build shipped
  (`lamp_update::install::RESET_PLAN_SHIPPED`, passed to `prepare_with`) is
  written over with this build's, so a store added since is wiped too (El
  on #2975); one changed by hand is kept.
- The runtime directories under `/var/lib` (`lampOS`, the memory's, the
  actions', and the motor and camera locks 0700, the last four the parts'
  account's, `orangepi` on a V1 image).
- The voice's recorded failure lines (#2943), last, in
  `/var/lib/lampOS/voice/notices`, the folder the generic voice unit
  names: the five files `deploy/product/models.json` pins under
  `/opt/lampOS/voice/notices`, fetched from their release and checked by
  size and SHA-256 with `lamp-update install`'s own code
  (`install::place_moved_notices`). Only a missing one is fetched; one there
  that is not the pinned one is kept and said. Nothing records what prepare
  wrote, so when `models.json` re-pins a line, a moved lamp keeps the older one
  (it still plays) until it is removed and prepare runs again, which the said
  line tells. This is prepare's only
  network use. A line that cannot be fetched (no network, the release out of
  reach) is said and never refuses the update: the voice sighs in its place
  (failure-notices.md) and the next prepare tries again. Not under
  `/opt/lampOS`, which the package's packager takes for its own paths, and
  the package carries no notices.

A missing or doubled device, a `lamp.env` without `--home`, a setting the package
lacks or a missing account stops it, exit 1, before any of these is written
(the device key above may already be), and the old runtime keeps the lamp.
The account is looked up only when a directory it owns is missing, so a
lamp that has its profile, settings and directories is refused for none of
them: no V2 update is refused here. Its first run on a V1 lamp writes them
all, and a second run writes nothing. Each file is linked into place from a
temporary beside it, so one that appeared meanwhile is never replaced.

The settings prepare writes are not in `lamp-update install`'s records
(`state.settings`). A lamp moved this way stays on the package's settings:
install would take each as the lamp's own and keep it, so a newer default
would never reach it, and the pins file (without `reset`) would stay on a
product's `--touch` controls. Moving such a lamp to `install` needs those
three files removed, or recorded, first.

The key stays after a failed migration or a rollback: it is written outside
the updater's data folder, so neither the updater's staging cleanup nor its
snapshots remove it. A later `lamp-update install` finds it equal to the key
and takes it as its own.

Not shown: run by an updater, or on a lamp; only on a temporary root.

### Not set up for a home yet (#806)

With `--setup-marker PATH` (off unless given; the third's setup uses
`/var/lib/lampos-setup/done`), step 4 looks for the marker first. Present:
as above, call for call (a test compares the calls with and without the
switch). Absent: only `lamp-setup.service` is started, not
`lampos.target`, so nothing of lampOS listens before the lamp is set up;
the answer is not ready, `waiting_for: ["setup"]`. The serve loop looks for
the marker once a second and, once it exists, starts `lampos.target`, once.
The setup's reset removes the marker first, then runs `lamp-keeper stop
lamp`, wipes and reboots, so a lamp cut off mid-reset comes back into setup.

### What the old runtime leaves behind

A lamp that still has the old software has two things in the way, both
seen on lamp-52e6 on 2026-10-14; lamp-4ace has neither.

- **Its units.** `lamp-voice` refuses to start while `hal.service`,
  `os-server.service`, `led-boot.service` or `led-shutdown.service` is
  active (`refuse_legacy_owners`, in the voice and in its camera check).
  The updater stops `hal` and `os-server`, but `led-shutdown.service`, a
  static unit, becomes active whenever `hal` stops. Before it starts the
  lamp the keeper masks each of the four that is active and then stops it
  (masked first, so that stopping one cannot start another), says each in
  its journal, and lists them in `health` as `legacy_stopped`. It masks
  because its unit is the updater's replacement for the old runtime; it
  deletes nothing, and one that is not active is not touched. The parent's
  updater leaves the old runtime's mask state as it found it and unmasks
  nothing, so a lamp brought up this way keeps these masks after a rollback
  until someone runs `systemctl unmask`. Under the parent's package adapter
  the keeper runs without `AUTONOMOUS_SERVICE_NAME` and masks nothing.
- **The bus root.** Made by whichever part comes first, `/run/lampos-bus`
  was root's on lamp-52e6 (by the voice), and `lamp-health` and
  `lamp-expression`, run as `orangepi`, failed on it with "Permission
  denied". The keeper makes it as lamp-4ace has it ([bus.md](bus.md), "On
  the lamp"): owner root, group the first `Group=` other than root among
  the release's units, mode 2770, the set-group-ID bit giving everything
  made below it the group. Made if missing, put right if there, every
  time; `LAMP_BUS_ROOT` moves it. What is already below the root is left
  as it is.

systemd still runs every part; the keeper only brings them up together and
watches. The target itself is not enabled: at power-up it is the keeper's
unit that starts, and the lamp with it. Stopping the keeper's unit stops
the target, and the parts in reverse order.

On lamp-52e6, 2026-10-14, with a package laid out as the updater lays it
(`/opt/lampOS/lamp-os/<version>`, the updater's own unit text, the voice,
body and eyes of `deploy/lamp-52e6/` in `units/`), the updater itself not
run:

| What | Measured |
|---|---|
| `systemctl start lamp-os` to `health` saying ready, the `pid` equal to the unit's main process | 2.56 s, 2.53 s; once 5.8 s while the body was still folding down from the stop before |
| `systemctl stop lamp-os` to every part stopped | about 12 s: the eyes at once, the body after folding down, the voice last |
| The body killed (`kill -9`) to running again | 15.3 s (its `RestartSec=15`) |
| The body hung (`kill -STOP`) to running again | 19.6 s: the watchdog after 5 s, `SIGABRT`, then 15 s |

The journal of that window is in
[`verification/keeper-lamp-52e6-20261014/`](verification/keeper-lamp-52e6-20261014/README.md).

The body is the only part that says ready and alive (`Type=notify`,
`WatchdogSec=5`); for the others "running" is all systemd knows, and the
keeper's `health` says no more than that.

## What was not shown

- The templates in `deploy/units/` have not run on a lamp as a whole: they
  were checked with `systemd-analyze verify` (systemd 252, Debian bookworm,
  in a container) with every placeholder filled, no complaint but the
  missing binaries. lamp-52e6 ran its own three (voice, body, eyes) under
  the target, above.
- The updater installing the package: the layout and the unit were copied
  from it by hand; its activation, its own health check and a rollback
  through it were not run.
- A power cut with the keeper's unit enabled.
- Clearing the old runtime's units and making the bus root, on any lamp:
  checked only with stand-ins for systemd and `chown`. Whether masking
  `led-shutdown.service` holds while the updater's own steps run, and a
  rollback unmasking it, were not run. Topics a root-owned bus made before
  the keeper are not put right; on lamp-52e6 they go at the next power-up
  (`/run` is not kept).
- `lamp-led-shutdown.service`, also seen on lamp-52e6: no part checks it,
  so the keeper leaves it.
- The order at power-up and how long each step takes; whether the eyes
  should wait for the body's start checks; whether the voice should wait
  for the memory once it can.
- The voice in `lampos-voice.slice`. lamp-52e6's kernel (5.15.147) has no
  `CONFIG_RT_GROUP_SCHED`, so its real-time threads should start in any
  slice; lamp-52e6 still runs it in `system.slice`, as lamp-4ace does.
- The voice's limit of ten starts in ten minutes.
- The pause: first shown with host tests (the kept value read
  and cleared, the body's report saying `paused`; the voice's real service
  loop with fixture workers refusing a question while paused, finishing
  the answer under way, and answering again once resumed; Expression
  asking nothing while paused). `lamp-keeper pause` in the middle of a
  conversation was then shown on lamp-52e6 on 2026-10-15
  ([readings](verification/keeper-pause-voice-expression-lamp-52e6-20261015/)):
  the answer under way went on to its end, a question while paused was not
  taken and `voice/state` said `resting`, and after `resume` the same
  question was answered. Not shown there: Expression holding back a request
  (none came up), news, a hello or a reminder arriving while paused, and the
  keeper's own `stop lamp`.
- The update choreography is `lamp-update`'s (#737, second builder):
  quiet judged from the bus, each part switched and checked, rolled back
  if unwell. The reset of task 12 waits on setup, which the owner has not
  decided. The keeper takes no command from the bus but the pause: starting
  and stopping parts stays on its command line.
- lamp-4ace's units are unchanged here: they have no target, no slices and
  no watchdog.
