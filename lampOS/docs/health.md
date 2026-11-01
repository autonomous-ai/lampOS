# Health: how the lamp is

`lamp-health` (`crates/health`) answers one question: how is the lamp? Each
part of the lamp says how it is on [the bus](bus.md). This program puts
what they say together, adds the board's own numbers, and shows both to a
person with one command.

## What it is for

ROS gives every robot a `/diagnostics` stream, to which each part reports
on itself, and one aggregated view of all of them. The owner asked for the
same defaults. Here that is three things:

1. Every part's newest report, put together into `lamp/health`
   (`lamp_msgs/Health`): the worst level, one sentence, every part with its
   level, message and age, and the parts that should report and never have.
2. The board's own numbers, read from the system and published as
   `board/state` (`lamp_msgs/BoardState`). The board is also one more part:
   it reports on itself as the others do, under the name `board`.
3. One command, `lamp-health`, that prints how the lamp is and ends with a
   status a script can use.

It is its own process. With it gone nothing else on the lamp changes, and
a part reports whether or not anybody listens. The skill Not feeling well
(#1681, [skills.md](skills.md)) reads `lamp/health`'s `level`: `error` for
30 s, and the lamp droops once and dims a shade until it ends, or until
`lamp/health` has said nothing for its 15 s. It comes with the product
since its lamp check, row 8 on lamp-52e6 (20 Oct; `install::PRODUCT_SKILLS`).

The voice can say which part is unwell, when asked (#1681). Behind
`LAMP_HEALTH_FACTS=on` in the voice's unit (off unless set;
`crates/voice/src/health_told.rs`), the provider worker reads
`lamp/health`'s kept value every 5 s, outside any turn, and tells the
model one short line beside what a person is saying to the lamp:
"(For you to know, not to answer: not working right in you now: camera,
arm and head. Say so only if someone asks how you are or what's wrong,
and then name each of them; never bring it up yourself.)" "and then name
each of them" is there only with more than one part: told two, "how are
you?" named only the first. The parts are named by `show::known_as`
(below), and those the self-test found failed by its own names
(`parts::part_known_as`, "the SD card" said as "SD card", a possessive
name keeping its "the": `voice_route` is "the voice's connection"); at
most four, then "and N more". A part counts when it has been `error` for 30 s, or
`stale` for 5 minutes while `quiet_ms` is 5 minutes or more. It is told
once a session for each set of parts; when the set empties after it was
told, the session is told "all your parts are working again". Nothing is
told when all is well, and nothing when no `lamp/health` newer than 15 s
is kept (not knowing is not well): the 30 s starts again. A turn never
waits for it: the note comes from the last read, and is queued with the
turn's input. On the second route (`LAMP_VOICE_SECOND_CONFIG`), whose
turns each start with nothing, a thread of its own reads `lamp/health`
every 5 s (`health_told::watch`) and the same line, without "all well
again", is in every turn's system message while a part counts; the turn
reads only what that thread keeps. Whether the model keeps quiet unasked
has not been measured.

## The commands

    lamp-health serve [--bus-root ABSOLUTE_DIR] [--expect NAME]... [--stale-ms N] [--judge-zone BEGINNING]... [--proc DIR] [--sys DIR] [--run DIR] [--disk PATH] [--clock timesync|kernel] [--online on|off] [--reach-probe on|off] [--reach-idle on|off] [--needs NAME=PATH]...
    lamp-health [show] [--bus-root ABSOLUTE_DIR] [--json]
    lamp-health --help

`serve` is the program that runs all the time.

| Option | What it is | If left out |
|---|---|---|
| `--bus-root` | The bus's directory | `LAMP_BUS_ROOT`, or `/run/lampos-bus` |
| `--expect` | A part that should be reporting. May be given up to 64 times, each name once | none |
| `--needs` | `NAME=PATH`: the part runs only where the absolute file `PATH` is. While it is not there, the part is listed as ok with "off: no <file> on this lamp (PATH)", never as missing, and the lamp's health is not lowered by it; checked at every working out. On a home lamp, `tasks=/usr/local/bin/harness`: the tasks part waits for Harness by its unit's condition (#811) | none |
| `--stale-ms` | After how long without a report a part is stale, from 100 to 600000 | 5000 |
| `--judge-zone` (repeatable, at most 16) | How the name of a thermal zone begins whose own limits are judged. A board has zones for parts the lamp does not use: on lamp-4ace the graphics zone names a first limit of 60 °C and reads 64 at rest. Every zone's temperature is published whether it is judged or not | every zone is judged |
| `--proc` | Where load, memory, uptime and the Wi-Fi signal are read | `/proc` |
| `--sys` | Where the temperatures are read | `/sys` |
| `--run` | Where the clock's state is read | `/run` |
| `--disk` | Any path on the disk whose free room is watched | `/` |
| `--clock` | Who says whether the clock is set: `timesync`, systemd-timesyncd's marker under `--run`; or `kernel`, the kernel's own answer (`adjtimex`, as `timedatectl`'s `NTPSynchronized`), which any time client sets, with the marker still believed first (#830, [offline.md](offline.md)) | `timesync` |
| `--online` | `on` to publish `lamp/online` too, once each report's time: whether the lamp can reach the internet, from the voice's `link` fact on `diagnostics` (`ready` and `connecting` are online, `down` is not, a voice not reporting or stale, or saying `closed` while it waits with its microphones off (#1371), is unknown), with its `down_for_s` (#830, [offline.md](offline.md)) | `off` |
| `--reach-probe` | `on`, with `--online on` only: while the voice's link is down, ask the internet itself (a TCP connection to 1.1.1.1:443 or 8.8.8.8:443, never port 53, at most every 30 s, each try bounded at 2 s, on its own thread). Its verdict goes in `lamp/online`, from `reach`: online if it answers (a relay down with the internet up is the service's trouble, not the home's), offline if not, and unknown while no probe has ended in the outage (#830, [offline.md](offline.md), "The relay is not the internet") | `off` |
| `--reach-idle` | `on`, with `--reach-probe on` only: the probe also asks with the voice's link up, every 30 s, and every 5 s once the internet doesn't answer; not answering is offline in `lamp/online`, from `reach`, with the link still ready. On the second route as the only voice the link goes down only when a turn fails on the network, so with nobody speaking the lamp said online through a Wi-Fi cut (lamp-06c1, 20 Oct: 38 s and 13 s), and Expression's offline look and the light couldn't follow (#1844, [offline.md](offline.md)) | `off` |

Every path is absolute. A wrong option prints the usage and exits with 2.
A bus that cannot be subscribed to exits with 1 before anything is listened
to. After that nothing ends it but being killed. It cannot catch a signal
(the standard library has no way and the crate takes in nothing else), so
SIGTERM ends it where it stands. Nothing is half done by that: each value
on the bus is put in place whole. Its socket is left for the next report
to remove, and what it kept on the bus means nothing fifteen seconds later.

`show`, which is also what `lamp-health` alone does, is for a person or a
script. It takes `--bus-root` as above, and `--json`.

| It ends with | When |
|---|---|
| 0 | The lamp is ok |
| 1 | The worst of it is a warning |
| 2 | The worst of it is an error |
| 3 | A part is stale or missing, or nobody is reporting the lamp's health at all |
| 64 | It was called wrongly: an unknown option or command, or a bus root that is not an absolute path. Not 1 or 2, so that a mistake cannot be taken for a level |

`--help` prints the options and these statuses, and exits with 0.

## What it reads and publishes

All with the types of [lamp-msgs](msgs.md), under the name `health`.

| Topic | Type | Kind | What `serve` does with it | When |
|---|---|---|---|---|
| `diagnostics` | `lamp_msgs/Diagnostic` | event | Subscribes: every part's report | As they come |
| `diagnostics` | `lamp_msgs/Diagnostic` | event | Publishes the board's own report, named `board` | Once a second. With a stale time under three seconds, three times within it, so that the board is never stale by its own doing |
| `lamp/health` | `lamp_msgs/Health` | latest | Publishes how the whole lamp is | At once when it says something new, and every 5 s besides. It means nothing after 15 s |
| `board/state` | `lamp_msgs/BoardState` | latest | Publishes the board's numbers | Every 5 s, when the board is read. It means nothing after 15 s |
| `lamp/online` | `lamp_msgs/Online` | latest | Publishes whether the lamp can reach the internet, with `--online on` only (the product's unit has it) | With each `lamp/health` turn |
| `lamp/selftest` | `lamp_msgs/SelfTest` | latest | Subscribes: the self-test's newest run, its kept value first ([The self-test's run](#the-self-tests-run)) | As runs come; the subscription is tried again each second while it cannot be made |

The two kept values come five seconds apart, and the bus takes a publisher
that has been silent for its lease of three seconds for gone. So the
program tells the bus that it is alive (`Publisher::alive()`) on all three
topics it publishes, and on `lamp/online` too with `--online on`, with every
turn of its loop, ten times a second or more. The bus
gives a publisher's entry a new time at most once a second
([bus.md](bus.md)), so that is one file touched a second for each topic.

The board's report is also put straight into the program's own table of
parts, so the board is in `lamp/health` even if that message is lost.

`show` subscribes to nothing. It reads the kept values of `lamp/health`
and `board/state` (`Bus::latest`) and creates nothing on the bus. A value
that has outlived its fifteen seconds is the same as none: whoever wrote it
has stopped.

## How the lamp's health is worked out

A part is whatever reports under a name. `serve` keeps each part's newest
report by its `name`, with when it arrived by the program's own clock, and
works the lamp's health out again whenever a report has come and once a
second besides. The rule is one function that touches nothing
(`parts::health`: the reports, the time, what is expected and the stale
time go in, a `Health` comes out):

- **A part that has reported** has the level and the message it gave, and
  `age_ms`, how long ago its newest report came.
- **Stale.** A part that was well and is not heard from for longer than
  the stale time (more than 5000 ms, to the millisecond) is `stale`, and
  its message is `not heard from for N s`, in whole seconds, not rounded
  up. A part never says of itself that it has gone quiet. One that does
  report itself `stale` is taken at its word, with its own message.
- **Quiet after a warning or an error.** A part that had said `warn` or
  `error` and then went quiet keeps that level, and what it said stays in
  front of the seconds: `stopped: elbow is too warm; not heard from for
  9 s`. A body that stopped itself and said why is not made milder by its
  silence, and what it last said is usually why it is silent.
- **Stopping on purpose** (#2415's P3). A part that stops on purpose says
  so: an `ok` report with the fact `state` = `stopping`
  (`Diagnostic::stopping`), sent once. The part is forgotten: it is no
  longer listed, never goes stale and never counts in `quiet_ms`, so a
  planned stop leaves no droop. Its next report tracks it again, from
  nothing: it counts in `quiet_ms` again only once it has said `ok`. A
  part that goes quiet without saying it (a crash) is stale and counts as
  before. It is a fact, not a field or a level: a `lamp-health` from
  before reads it as one more `ok` report. A forgotten part named with
  `--expect` is missing again until it next reports. Who says it: the
  voice, for `voice-keys`, when own keys are turned off (the second
  worker's start with `LAMP_OWN_KEYS` off) and when it drops the second
  worker (#960) with the switch on ([own-keys.md](own-keys.md)); and the
  body, in its last report as it stops (`lamp_body::bus_out::stopped`). A
  body asked to stop says it at `warn` and is forgotten like any stop on
  purpose. A body stopped on a fault (too warm, pushed against, servos not
  answering) says it at `error`: an `error` is never a stop on purpose
  (`Diagnostic::is_stopping`), so it is kept, and once quiet it is that
  error and why, as above, not missing. Until this was fixed (21 Oct to
  1 Nov) an expected body that stopped on a fault was shown as never
  heard from.
- **Missing.** A name given with `--expect` that has never reported is in
  `missing`, in the order given. A part that reported once is never missing
  again, however long ago: it is stale.
- **Order.** The items are worst first by `Level::rank()` (error, warn,
  stale, ok), then by name.
- **The privacy switch** is its own item, `privacy switch`, sent by Controls
  when the switch is attached: `open` or `closed`, with `, not moved since
  Controls started` until it has moved ([privacy-switch.md](privacy-switch.md#the-switch-in-the-lamps-health)).
- **The head pads** are their own item, `head pads`, sent by Controls on the
  touch path: which of the pins' head lines a pad drives, from a probe at
  its start. `warn` when none does ("the head reads no touch") or while a pad
  touched since the start is left out (controls.md, "Said when no pad is
  there").
- **The lamp's level** is the worst item's, and `stale` at least when a
  part is missing.
- **The lamp's message** is empty when the level is ok. Otherwise it is
  the first item that is not ok, as `NAME: MESSAGE`, or
  `NAME: LEVEL, and it does not say why` if that part gave no message.
  When only a missing part makes the lamp not ok, it is
  `NAME: has never reported`.
- **`quiet_ms`** (#1681): how long, in milliseconds, the longest-quiet
  part that has said `ok` at least once since `serve` started has gone
  unheard (its `age_ms`), counting only a part past the stale time; 0 when
  there is none, and then left out of the message, so a well lamp's
  `lamp/health` is what it was before the field. A part never heard from
  (missing at boot) or never `ok` does not count; nor does one whose last
  word was `error`, which keeps that level while quiet and is said by the
  lamp's `error` already, nor one that said it is stopping (above). A
  part restarting for a few seconds is in it
  only for those seconds. It is for a part that came up well and then
  stopped: Not feeling well droops at 5 minutes of it
  ([skills.md](skills.md)). It is an age, so it is no news on its own
  (below). Forgotten when `serve` restarts: a part that stopped before
  that, and never reports again, is not counted.

Every result passes `Health::check()`; the tests ask that of each one.

Two consequences of these rules that may surprise:

- A part that is stale stays in the list for as long as `serve` runs. A
  part that was taken off the lamp is forgotten when `serve` is started
  again.
- Before any part has reported, every expected part is missing, so a lamp
  that has just started is `stale` for its first moments.

**What is kept.** A name is anything on one line of 48 bytes at most, with
no space at either end. A report with another name is passed over like a
message that is no report. A message is put on one line (line ends become
spaces) and cut to 160 bytes, ending in `…` where it was cut. At most 64
parts are kept: a report from a 65th name is turned away, and those kept
go on reporting. With 64 parts and 64 names expected, each as long as may
be, a `Health` is under 50,000 bytes, where the bus carries 60,000.

**What is news.** A health is published at once when it differs from the
one last published in anything but the ages: a level, a message, a part
more, a part no longer missing, a part gone stale or come back. The count
of seconds in `not heard from for N s` is an age too, so a part that stays
quiet is news once and not again with every second. A health that is no
news is published when five seconds have passed since the last.

## The self-test's run

A lamp says a part has failed before its person finds out (#812). The
self-test (`lamp-selftest`, [selftest.md](selftest.md)) publishes no
`diagnostics`: a run is rare, and a one-off part would go stale here. So
`serve` subscribes to `lamp/selftest` and takes each whole run (one that
passes `SelfTest::check`), the run kept on the topic first. Each part keeps
its last *checked* verdict (`parts::SelfTests`): a part that failed stays
failed until a later run checks it and it passes, or is not fitted. A run
that could not check it (`not_checked`, or without it: a passive run checks
no speaker, ring colours or head move) leaves the fail standing (the
reviewer on #1243), **unless the run is another self-test build's** (its
`build`, #1399): then that fail was the older rule's. On lamp-4ace on
17 Oct, a storage fail from the build before #1399 held "the lamp is not
well" after the new build said `not_checked`. Such a fail moves to a
warning, never dropped, since the old fault may have been real; a run that
checks the part settles it. A passive run (`first-boot`, the 6-hourly
timer), which never checks the speaker, the ring's colours or the head move,
never demotes a fail an active run (`on-demand`, `factory`) gave, whatever
its build: any update that changes the self-test changes the build (the
eighth on #1426). A run that names no build (the program run by hand,
outside an install) demotes nothing. Two runs
that name no build count as the same (runs from before the field), and an
older run with no build against a newer one with a build does not. Each time
the health is worked out it gets one more item, `selftest`
(`SelfTests::item`, `parts::mark_selftest`):

- **A part still failed:** `error`, `N parts failing: PART (REASON), …` for each, each part by the name a person knows it by (`storage` is "the SD card", `air` "the air sensor", `parts::part_known_as`),
  in the order they first failed, with each one's newest reason. The lamp
  is then `error` too, with that as its message.
- **The lamp's USB hub dropped** (the newest run's `voice_mic` says `usb_hub_said`): `warn` with that line when nothing failed, and before the failed parts' message when something did (so the line's 160-byte cap never cuts it). Beside failed parts, when the whole line won't fit, it's the self-test's short form (`usb_hub_said_short`: "the lamp's USB hub dropped 2 times since boot, with the voice mic, servo port, speaker/room mic and camera"), so the part's name still fits and at worst the end of its reason is cut; until a run's `voice_mic` says nothing of it. `lamp-health` can't read the kernel's log itself (it runs as `lampos`, without `CAP_SYSLOG`), so it takes the self-test's word.
- **A fault from before an update:** "a fault seen before this update, which this self-test could not check: PART (REASON), …". Alone it is `warn`, after the hub's line if there is one; beside a part failing now it is named after the failed parts, inside the `error`, where the 160-byte cap cuts it first.
- **None failed:** `ok`, `passed`, or `passed; N not checked` for the newest
  run. An ok item lowers nothing.
- **Its age** is since the newest run began, by the wall clock. It is never
  stale: a run holds until the next.
- **A reboot forgets it:** what was heard is kept in `serve`'s memory, and
  `lamp/selftest` is kept on the bus's tmpfs, so after a reboot there is no
  item until the next run (on a product lamp, five minutes after the lamp
  starts, #1245).
- A part that reports on `diagnostics` under the name `selftest` keeps its
  own report.
- `lamp-keeper`, which restarts a part unwell for long enough, passes the
  item over (`keep.rs`, `SELFTEST`): it is a result, not a part that runs,
  and `lamp-selftest.service` is never restarted for it, mid-run least of
  all.

A message on `lamp/selftest` that is no whole run is passed over and said
(`a message on lamp/selftest is not a whole run`); the run before it
stands. If the topic cannot be subscribed to, or can no longer be read,
that is said (`lamp/selftest cannot be received from (…)`) and the health
goes on with what it had, making the subscription again each second.

## Memory reports

Memory opts in with `--bus-root ABSOLUTE_DIR` on its existing command, in
either JSONL or `--memory-days` mode, and Health expects it with
`--expect memory`. The product's units have both since 15 Oct
(`deploy/product/lamp-memory.service` and `lamp-health.service`); the
templates under `deploy/units/` do not.

Memory reports `name: "memory"` on `diagnostics` once per second. Its values
include retained exchange/kept counts, their snapshot age, and the slowest
recent handler duration. Warnings describe its own storage/count failure or
an answer above 300 ms for a session note or 1.5 s for other requests. An
in-flight request is monitored too; a fresh reporter cannot conceal a hung
handler. A write fault remains until an append/remember succeeds. The full
window, exclusions and recovery rules are in
[Memory's report contract](memory.md#optional-health-reports).

Health's aggregate retains level, message and receipt age, **not** Diagnostic
key/value pairs. Read `diagnostics` to inspect counts and timings:

    lamp-bus --root /run/lampos-bus tap diagnostics

Use the bus root actually configured for that lamp. `lamp-health --bus-root
/run/lampos-bus --json` shows Memory's level/message/age in the aggregate.
With the Memory flag absent, an expected Memory stays missing; after a
previous report, silence is handled by the existing stale/warning rules.
Memory's socket loop performs no bus I/O. Bus failure does not imply its
store failed and does not stop its socket service.

No unit is edited by task 28. This reporting addition has host tests only;
its cadence and request cost under the lamp's flash load still need a lamp.

## The board

Every five seconds `serve` reads the board. Each place is an option, so
that a test can point it at a tree of files of its own.

| Number | Read from | How |
|---|---|---|
| `temperature_c` | `SYS/class/thermal/thermal_zoneN/` | Every zone that has a `type` and a `temp`: the type is the key, the temperature is `temp` over 1000. Zones are taken by their number; a second zone of a type already seen gets `_N` after its key. A reading under -273.15 °C or over 1000 °C is not a temperature and the zone is left out |
| `cpu_load` | `PROC/loadavg`, `PROC/stat` | The first number of `loadavg`, over the number of lines `cpu0`, `cpu1`, ... in `stat` |
| `memory_used` | `PROC/meminfo` | 1 less `MemAvailable` over `MemTotal` |
| `disk_free` | `statvfs(DISK)` | The blocks an ordinary program may still use (`f_bavail`) over all the disk's (`f_blocks`) |
| `uptime_s` | `PROC/uptime` | Its first number, in whole seconds |
| `wifi_dbm` | `PROC/net/wireless` | The level of the first device that gives one between -150 and 0. Left out if the file is not there or has no such line |
| `clock_synced` | `RUN/systemd/timesync/`, and with `--clock kernel` the kernel | `true` if the file `synchronized` is in it; else with `--clock kernel` the kernel's answer (unsynchronised status off and a maximum error under 16 s); else `false` if the directory is there without it, left out if there is no directory |

Nothing here fails. `cpu_load`, `memory_used`, `disk_free` and `uptime_s`
are the four the lamp cannot do without: if one cannot be read or is not a
number, no `board/state` is published that round, and the board's report
says which. A zone, the Wi-Fi and the clock are left out where they cannot
be read, and nothing is said. No file is read past 256 KiB, and at most
64 zones and 16 trip points of each are looked at.

Each zone may name trip points: `trip_point_N_temp` and
`trip_point_N_type`, from 0 without a hole. Those of type `passive` and
`critical` are used, the others (`active`, `hot`) are not.

### When the board is not well

The board's report is the worst of what is found, in words, and of two
findings as bad the first in the order of this table. The thresholds are
named constants at the top of `crates/health/src/board.rs`, and each is
tested from both sides.

| What | Warn | Error | Why there |
|---|---|---|---|
| A zone that names trip points | At or above its lowest passive point: `cpul_thermal_zone is at 72 °C: the board slows itself down from 70` | At or above its second passive point, or within 10 °C of its critical point: `... the board turns itself off at 110` | A passive point is where the board slows its processors down to cool, and what the lamp says then comes late. The warning is for a board that is doing that, not for one that is near it. The critical point is where the board turns itself off. A zone that has one of these points and not another is held only to the rules it has points for |
| The processors (every zone whose name begins `cpu`, judged or not) | From 95 °C (`CPU_WARN_C`): `the processors are hot: 96 °C at cpub_thermal_zone, 14 below the board's trip at 110; busiest: lamp-eyes 44%, lamp-voice 20%, lamp-turn-end 14%` | Once a processor group has been held below what it can do for a minute (`THROTTLED_ERROR_AFTER`): `the processors have been held back to cool for 60 s: policy6 may run at 1792 of 2002 MHz, cpub_thermal_zone at 93 °C; busiest: …` | The hardware side (via the owner, 2026-10-16 16:53): the big cores run around 85 °C normally, and only above 95 is it worth worrying about. lamp-4ace peaked at 96 °C at 16:34, and at 16:38 a voice worker held up past its bound exited, with the big cores capped at 1.79 of 2.0 GHz. Either finding names the three busiest processes since the last reading, by share of one core (from `/proc/N/stat` and `/proc/stat`), kept by the lamp's reader with how long the cap has held (`board::Watch`). The product unit judges only the skin zone's own trips: the processor zones' at 70 and 90 °C are passed all day by a lamp at work. "Held back" is a cpufreq policy's `scaling_max_freq` below its `cpuinfo_max_freq` (`devices/system/cpu/cpufreq/policyN`), counted only while the board says it is cooling: a cpufreq cooling device (`class/thermal/cooling_device*`, `type` beginning `cpufreq`) with `cur_state` above 0, or the hottest processor zone at or past its first passive trip. A cap with nothing hot (boost speeds left out, or an image's own cap) is not heat (the reviewer on #1105). The board's idle-injection cooling (`idle-cpuN`) is not counted. Once given, the warning holds until the zone is 3 °C under the line (`CPU_WARN_CLEARS_BELOW_C`), so a zone at 95 does not flip the lamp's health on every reading |
| A zone whose critical point is 60 °C or lower: the lamp's case, `skin_zone`, off at 50 (`LOW_CRITICAL_C`) | From 8 °C under it: `skin_zone is at 42 °C: warm, and the board turns itself off at 50` | From 4 °C under it | Ten under 50 is the case's working heat: on lamp-4ace on 24 Oct it read 40 to 42 °C all afternoon with the voice and eyes at work, an error at ten under, and the lamp drooped as unwell 18 times in three hours (the not-feeling-well skill). The heat itself is the hardware team's |
| A zone that names none | From 80 °C | From 90 °C | Two fixed lines of this program's own, not taken from the board: 90 is where the processor zones have their second passive point |
| `disk_free` | Under 0.10 | Under 0.03 | Logs, recordings and updates all need room. At a tenth there is still time to make some; at three hundredths the next thing written may not fit |
| `memory_used` | Over 0.90 | never | Little is left before the system has to end a program to find some |
| `cpu_load` | Over 1.5 | never | For a minute there has been half as much work again as the processors can do, and everything waits its turn |
| `clock_synced` | `false` | never | The bus judges a publisher's lease and a value's lifespan by the wall clock, and so do the logs. A clock that is not set will be stepped when it is |
| A number that could not be read | Always: `could not be read: cpu_load, uptime_s` | never | The board is not known to be well |

The Wi-Fi signal is published and never judged. With no thermal zone at
all, nothing is said of temperature.

The report's `values` are the numbers as text, each there only if it was
read: `cpu_load` (two decimals), `memory_used` and `disk_free` (three),
`uptime_s`, `hottest_zone` and `hottest_c` (one decimal); the processors'
`cpu_zone`, `cpu_c` (one decimal) and `cpu_next_trip_c` (the zone's next
trip point above it, or its highest); and, where the board has cpufreq,
`throttled` (`true` or `false`), with `cpu_max_mhz` and `cpu_hardware_mhz`
of the group held furthest back when it is, and `throttled_s`, how long
it has been. The self-test and the review page read these.

The real board's processor zones have passive points at 70 and 90 °C and a
critical point at 110 °C, and read about 63 to 66 °C when the lamp is at
rest. The warning first began five degrees under the passive point, at
65 °C, which is in the middle of what that board reads all day: it would
have gone between ok and warn every few seconds and said nothing a person
could act on. It now begins at the point itself. Nothing here holds a
level for a while once it has changed, so a board that sits at its
passive point will still go between the two.

## What `show` prints

Written for the lamp's owner (the first builder on #705, 17 Oct 15:08),
from the same reports as the program's tests: each part by the name a
person knows it by, `--expect actions`, a stale time of two seconds:

    The lamp is not well: voice: the link to the service is down

      error  voice              the link to the service is down
      warn   computer           the processor is at 66 °C: the board slows itself down from 70
      quiet  camera             not heard from for 2 s
      ok     what it remembers
      not running: volume and light commands

    computer  hottest 66 °C  memory 44% used  disk 14% free  up 2 d 10 h

The first line is the lamp's level and message: `The lamp is well.`,
`The lamp has a warning: ...`, `The lamp is not well: ...` for an error,
`Not all of the lamp is reporting: ...` for stale, its part named as
below, the first row's part when it is as bad as the lamp, so the two
never name different parts. A well lamp whose voice waits with its
microphones off says why on that line: `The lamp is well; the microphones
are off (the privacy switch).` (or `(muted)`, `(asleep: a hand held on the
head)`); the voice says it while it waits, below (#1371). Then every part, worst first and, within a
level, by its name; a part not heard from is `quiet`, its message saying
how long. Then the board's numbers, as `computer`: the hottest
temperature, memory, disk, uptime, and the Wi-Fi signal and `clock set` or
`clock not set` where the board could be asked. The kernel's zone names
are said by what they measure, with or without a "," or ";" after them
(`cpul`/`cpub_thermal_zone` and `cpul`/`cpub_idle_zone` "the processor",
`gpu` "the graphics chip", `npu` "the AI chip", `ddr` "the memory chips",
`skin_zone` "the lamp's case", any other `*_zone` "the board"), and
the load is left out; `--json` keeps both.

**Names.** Each part is shown by the name a person knows it by
(`show::known_as`): body is "arm and head", eyes "camera", ears "room
microphone", name "hearing its name", controls "button and touch",
expression "light and moods", air "air sensor", schedule "reminders",
memory "what it remembers", actions "volume and light commands", policy
"when to speak", board "computer", selftest "self-test", tasks "work for
your computer", keeper "parts keeper". A part not in
that list keeps its own name. `--json` keeps the programs' names.

**Ages.** No part's age is shown: a part not heard from says for how long
in its own message. A last line, "written … ago", says how old the picture
is only once it is over ten seconds old (`show::OLD`, twice `serve`'s
pace, so only a missed publish shows it); a value over fifteen is no
longer read at all.

**A user who may not read the bus.** `/run/lampos-bus` is `2770 root
lampos`, so a person's own login cannot read it. `show` then says "Can't
read the lamp's health as this user: run `sudo lamp-health show`." and
ends with 3. On lamp-75c9 on 17 Oct it said instead that nobody reported
the lamp's health, while `lamp-health serve` ran.

With no health to read it prints:

    Nobody is reporting the lamp's health: lamp-health serve is not running, or the bus is somewhere else.
    Looked in /run/lampos-bus.

`--json` prints one line, `{"health":...,"board":...}`: the two values as
they were published, and `null` for one that is not there. The status is
the same as without it.

A kept value that cannot be read, is not of its type, or does not pass its
type's `check()` is said on standard error and counts as not there. Text
from the bus is printed with its control characters turned into spaces:
anyone can write to the bus, and a terminal obeys what it is sent.

## What it prints while serving

One line each on standard error, beginning `lamp-health: `. How it started:

    lamp-health: started bus_root=/run/lampos-bus expect=voice,eyes stale_ms=5000 proc=/proc sys=/sys run=/run disk=/

A line whenever the lamp's level becomes another, with the lamp's message:

    lamp-health: health ok
    lamp-health: health ok -> warn: board: cpul_thermal_zone is at 66 °C: the board slows itself down from 70
    lamp-health: health warn -> ok

And each trouble of the table below once when it begins and once when it
ends. Nothing else: no line for a report, a reading or a publish.

## When something around it fails

None of these stops it.

| What | What it does meanwhile | What it prints |
|---|---|---|
| A topic cannot be published | Goes on with the other topics. The publisher is let go and made again within a second. A health that could not be published stays news and is tried again with each working out | `lamp/health cannot be published (not a directory): it is tried again each second`, then `lamp/health is published again; unpublished=7` |
| One of the four numbers of the board cannot be read | Publishes no `board/state` that round. The board's report is a warning that names the number | `the board cannot be read: cpu_load (/proc/loadavg: No such file or directory (os error 2))`, again when another number joins or leaves, then `the board is read again` |
| A zone, the Wi-Fi or the clock cannot be read | Leaves it out | nothing |
| A message on `diagnostics` is no report, or names a part no part can be called | Passes it over and counts it. The trouble is over when none has come for the stale time: a good report from another part does not end it, or one part sending something else every second would be said to begin and end every second | `a message on diagnostics is not a report: such messages are passed over`, then `diagnostics carries nothing but reports again; passed_over=6` |
| A 65th part reports | Does not keep it | `more than 64 parts report: the newcomers are not kept`, once |
| The socket for reports cannot be read | Waits a turn and tries again | `diagnostics cannot be received from (...)`, then `diagnostics is received from again` |
| Nobody publishes `diagnostics` | The board alone is in the lamp's health, and every expected part is missing | the lamp's level |
| `serve` itself stops | Nothing publishes the two values. Fifteen seconds later they have expired, and `show` says that nobody is reporting and ends with 3 | nothing |

Two things it does not notice. If its socket's file is removed from under
it, it hears no more reports and every part goes stale: it does not
subscribe again. And the board is read in the loop itself, so a file that
hangs when read holds everything up until it answers; `lamp/health` then
expires like that of a `serve` that has stopped.

## On the lamp

A record of 14 Oct. The product's `lamp-health.service` now expects 14
parts and judges `skin` alone, with `--online on`
(`deploy/product/lamp-health.service`).

On lamp-4ace since 03:16 on 2026-10-14, as `lamp-health.service` under
the lamp's ordinary user, with the command also at
`/usr/local/bin/lamp-health`:

    lamp-health serve --bus-root /run/lampos-bus --expect board --expect body --expect policy --judge-zone cpu --judge-zone skin

Three parts report: the board (this program), the body
([body.md](body.md), "On the bus") and the policy
([policy.md](policy.md)). The voice, the eyes, the memory and the actions
program do not report yet and are not expected, so their silence is not
in the lamp's health.

    $ lamp-health
    The lamp is well.

      ok     board     0.8 s ago
      ok     body      0.3 s ago
      ok     policy    0.7 s ago

    board  hottest 65 °C (cpub_thermal_zone)  load 0.10  memory 12% used  disk 14% free  up 2 d 12 h

The body's warnings are listed in [body.md](body.md), "On the bus". With
`LAMP_BODY_STRAIN_GUARD=on` (#4501) they include a joint warming fast, a
long strain, a stand that never lifted, and a lamp seated after a second
hot or strained sit-down within an hour: `body` stays `warn` until a person
holds its head a second, a self-test starts, or the body is started again.

What its first minutes on the lamp showed:

- **The board runs warm.** Its eight zones, as it names them, with the
  lamp at rest: both processor zones 64.7 to 64.9 °C (they slow
  themselves down from 70 and again from 90, and the board turns itself
  off at 110); two processor "idle" zones the same (limits 90 and 100);
  graphics 64.5 (limits 60 and 90, off at 110); a neural processor 63.8
  and memory 63.1 (off at 110); and `skin_zone` 37.0, at which the board
  turns itself off at 50. While builds were being installed over SSH the
  big processors read 75 and then 71 °C, and the lamp's health said so:
  `board: cpub_thermal_zone is at 75 °C: the board slows itself down from
  70`. So the processors are five degrees under their first limit when
  the lamp does nothing, and over it under a few seconds of work.
- **Two rules were wrong for this board** and were changed on what it
  read. The warning began five degrees under a zone's first limit, which
  is where this board sits all day. And every zone was judged, so the
  graphics zone, which the lamp does not use and which sits four degrees
  over its own first limit, made the lamp warn without end. Now the
  warning begins at the limit, and the unit names the zones to judge.

## Not shown

The tests were run on one development Mac (x86_64), which has no `/proc`
and no `/sys`, and as a build for the lamp's processor under emulation in
a container each time a build went to the lamp. On the lamp it has run
for minutes, not days.

What was run: 55 tests of the crate. As 54, 14 times in a row with no failure:
eight while the Mac was busy with other work (a load of 22 to 29) and six
when it was less so (6 to 8). The tests of the loop and of the built
program then passed 30 times more each, six at once. No test failed in any
run after it first passed.

- 12 of the rule that makes a `Health`: order, the worst level, stale to
  the millisecond on both sides, missing, the message, a part that comes
  back, a part that calls itself stale, what is kept of a name and a
  message, the 64 parts, the size of the largest health, and what is news.
- 14 of the board: the reader against trees of files (the real board's
  shapes as the brief gave them, each file missing, garbage in each, no
  zones, trip points), and each threshold from both sides as arithmetic.
- 7 of what `show` prints and 3 of the options.
- 12 of the loop: 10 on a real bus in a directory of its own, the test
  playing the parts and, in all but one of them, handing the loop the
  board it reads; and 2 of what it says and of its times.
- 6 of the built program: `serve` started and killed, `show` and `--json`
  run against it for ok, warn, error, stale and nobody, wrong arguments,
  and a bus that cannot be subscribed to.

Not exercised:

- **A part that fails on the lamp.** The body and the policy report there
  and were well. A real part going quiet, or warning, has been seen only
  in the tests.
- **The tests' board is not quite the real one.** The trees in the tests
  were written from a description of the board before it was read, and
  give limits to the two processor zones only. On the board six of the
  eight zones name limits ("On the lamp"). Whether a zone's `temp` can
  fail to read there has not been seen.
- **The Wi-Fi and the clock.** The real board has neither
  `/proc/net/wireless` nor `/run/systemd/timesync`, so both are left out
  there. The reader was run against files the tests wrote, in the layout
  the kernel and systemd are known to use, not against any taken from a
  board. On lamp-75c9 on 2026-10-16 the clock is kept by chrony
  (`systemd-timesyncd` inactive) and `timedatectl show` said
  `NTPSynchronized=yes`: the kernel's answer that `--clock kernel` reads.
  `lamp-health` itself with `--clock kernel` has been run only on a laptop.
- **The disk on Linux.** `statvfs` was asked of the Mac's own disk, which
  had about 14% free. The tests cannot choose that number: where they need
  the board to be ok they read what the machine gave and say so if it is
  under a tenth. The two disk thresholds are tested as arithmetic.
- **What the thresholds do on a lamp.** They were given, and one was
  moved after the board's numbers at rest were read. None has been watched
  over a working day.
- **The lease of three seconds for long.** One test runs the built program
  with the lamp's own times and watches for about six seconds that two
  subscribers are never told their source has gone. Another shows the
  publishers' entries given a new time while nothing is published. Hours
  of running were not watched.
- **More than about fifteen parts on a bus.** macOS takes no datagram over
  2,048 bytes, so a health of many parts cannot be published on the Mac.
  The 64 parts are tested in the table and as the size of the message, not
  through a bus.
- **A wall clock that is stepped.** `show` works out `written N s ago`
  from the wall clock, and the bus the fifteen seconds.
- **How long a reading takes on a board.** Some fifty small files every
  five seconds on the board as described, read in the loop. Not measured,
  and a file that hangs was not played.
- **The socket that cannot be read.** Its two lines are in the code and
  were not made to happen.
- **A bus that cannot be used at all while it runs.** One topic that
  cannot be published was played with a file in its way. A root that
  cannot be subscribed to at the start was played and ends the program.
- **SIGTERM.** The built program was killed with SIGKILL.
- **Memory reporting in a template unit.** The templates under
  `deploy/units/` do not enable Memory's report by themselves; the
  product's units do (above, "Memory reports").
