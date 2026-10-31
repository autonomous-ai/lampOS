# The self-test

`lamp-selftest` ([#812](https://github.com/autonomous-ai/lampOS/issues/812))
checks whether each part of the lamp works, in two minutes, at the factory,
at the first boot after setup (#806), every six hours after on a product
lamp, and when asked. For each part it says
pass, fail, not fitted or not checked, with one plain reason. Nothing leaves
the lamp.

## What a run gives

- `lamp/selftest`, type `lamp_msgs/SelfTest` (`crates/msgs/src/selftest.rs`),
  kind latest, published once at the end of a run: the occasion, when it
  started, how long it took, every part's result in the order it ran, the
  run's verdict, and its `build`: the program's folder under
  `/opt/lampOS/selftest/` as installed (`build_of`), absent when run from
  anywhere else, so `lamp-health` can tell an older rule's fail (#1399).
  And its `lamp`: the lamp that ran it, by its host name as the kernel has
  it (`/proc/sys/kernel/hostname`, `lamp_selftest::lamp_of`), the name
  `lamp-cloud`'s `info` answers as `hostname`: `lamp-` and the last four of
  its serial or Ethernet address, which the image's hotspot script gives it
  at the first boot. A test station refuses a report naming another lamp
  than the one it identified ([production-testing.md](production-testing.md)).
  Absent where the name cannot be read, is not a host name (1 to 64
  letters, digits, `-`, `.`, `_`), or is not one lamp's own: a name other
  lamps share, the image's `autonomous`, a stock board's `orangepi`, or
  `lamp-` with no suffix (`SelfTest::is_own_name`, El on #3073). Absent too
  from runs before it: those still read (`#[serde(default)]`), with no lamp.
- `/var/lib/lamp-selftest/<started>.json` (`--dir`, both units) for each run, the newest 20
  kept, with `latest.json` and `latest.txt` (one line a part) beside them;
  a run with no `--dir`, as the factory's, keeps them in
  `/var/lib/lampOS/selftest` (`report::DIR`).
  Each file is written whole and renamed, so a power cut leaves no half
  report.

A part's verdict is one of:

| Verdict | Means |
|---------|-------|
| `pass` | the part did what it was asked |
| `not_fitted` | this lamp has no such part and is not meant to; not a fault |
| `not_checked` | it could not be checked this time (its owner was not running, no person to touch it, out of time); not a pass |
| `fail` | the part is there and did not do what it was asked |

The run's verdict is the worst of its parts, `fail` before `not_checked`
before `pass`; `not_fitted` makes no run worse. A part reached after the
two minutes are spent is `not_checked` and says so.

The self-test publishes its own topic and no `diagnostics`: `lamp-health`
keeps every part it has ever heard and would show a one-off `selftest`
part as stale until it restarts (`crates/health/src/parts.rs`). Instead
`lamp-health` reads `lamp/selftest` and carries the runs as one item of
`lamp/health`: `error` naming each part that failed and why, until a later
run checks that part and it passes; a run that could not check it leaves the
fail (#812; [health.md](health.md#the-self-tests-run)), unless the run is
another build's: an older build's fail it could not check is a warning, not
"not well" (#1399). A reboot forgets it.

## How each part is checked

One writer per device: the self-test never opens a device another part
owns. It listens on the bus, or asks the owner through the interface the
owner already has.

| Part | Owner | How | State |
|------|-------|-----|-------|
| voice microphone (Jieli) | lamp-voice | a second of `mic2/audio` blocks (given with `LAMP_BUS=on` and `LAMP_BUS_AUDIO=on`), their level above digital silence (-90 dBFS) and blocks lost counted; none in 2 s is `not_checked` (closed by the privacy switch, or not given), unless this lamp's voice mic is a Jieli (its card `device_micro2` is there, or the kernel found one this boot) and that card is gone from `/sys/class/sound`: then it fails, "not on USB now", since the voice cannot open it (#1178: on lamp-4ace on 17 Oct it fell off USB twice, the voice's capture failed at open with `errno 19` and the restart guard muted the lamp). On an image whose mic is a ReSpeaker or XVF3800, `device_micro2` is only an ALSA alias, no card has that name, and nothing is said of USB (`on_usb` "unknown"). Run by hand without `CAP_SYSLOG`, or with the kernel's ring wrapped past the Jieli's record, a Jieli mic that is gone now is `not_checked`, not a fail, and `on_usb` says which. Its USB port (`usb_port`, from the card, or from the kernel's `Manufacturer: Jieli` record when the card is gone), whether it is on USB now (`on_usb`) and its `USB disconnect` records since boot (`usb_drops`, from `/dev/kmsg`) are said every run. The drops are "at least" when the kernel's ring no longer reaches back to boot (its first record kept is not the boot's first, or some were overwritten), and "unknown" when the log cannot be read (run by hand, without `CAP_SYSLOG`) or no port is known, never 0. **The hub it hangs from** (`3-1.1` for `3-1.1.1`) is counted too: its own `USB disconnect` records (`usb_hub`, `usb_hub_drops`), and what dropped within 1 s of each, named from the kernel's own records of each port (Jieli the voice mic, C-Media the speaker with the room mic, since that speakerphone is lamp-ears' mic1 too, `cdc_acm … ttyACM` the body's servo port, a product with "camera" the camera). The reason then says it as one thing: "the lamp's USB hub dropped 2 times since boot; the voice mic, the body's servo port, the speaker with the room mic and the camera went with it" (`usb_hub_said`; lamp-4ace at 17:12 on 17 Oct; "at least 2 times since boot" when the ring wrapped, kept short to fit `lamp/health`'s line), then the mic's own drops apart from those ("the voice mic alone dropped 1 time since boot"). A failed mic's reason leaves the hub's line out and says only its own drops ("not on USB now: …, the voice mic alone dropped 1 time since boot"), since `lamp/health` adds `usb_hub_said` before the failed parts, or `usb_hub_said_short` ("the lamp's USB hub dropped 2 times since boot, with the voice mic, servo port, speaker/room mic and camera") when the whole line and the parts don't fit in its 160 bytes. Drops are said in the reason ("off USB 2 times since boot") but never fail it alone: the mic's own drops on a passing mic are in the run's report and on `lamp/selftest`, not in `lamp/health`; a hub drop makes `lamp/health`'s `selftest` item `warn` for the rest of the boot. The microphone is never opened: it is the voice's, behind its privacy switch. What it heard of the lamp's own speech is the `speaker` part below | listens: done; USB presence and drops built, not yet run on a lamp |
| room microphone (C-Media) | lamp-ears | `ears/level` arrives, above digital silence (-90 dBFS) | listens: done |
| speaker | lamp-voice | at the factory and on demand only: the voice is asked on `selftest/speaker/request` and answers on `selftest/speaker` from the lamp's own last speaking turn of 4 s or more, playing nothing ([speaker-check.md](speaker-check.md)): the verdict, the voice microphone's rise over the room, what the canceller took off at the turn's loudest block (`cancelled_db`) and its median (`cancelled_median_db`, the verdict's), `cancelled_lower_bound` when the residual hit its floor, and the turn's age (`turn_age_s`). No such turn yet is `not_checked`, so at the factory the lamp is first made to speak for 4 s or more (asked something with a long answer, such as a short story) and the self-test is run after; no answer in 2 s is `not_checked` (the voice runs without `LAMP_SELFTEST_SPEAKER=on`); at first boot it asks nothing | done |
| servos 1 to 5 | lamp-body | `body/joint_states` fresh for all five joints, and the body's own warning (a joint warm, or one that met resistance) fails them. At the factory and on demand only, `head_move`: the head's pitch is asked 40 counts up and down and its roll 20 counts each way through `expression/body`, eased in and out under 200 ms leases as Expression asks, which `lamp-body` takes at rest under `--rest-glance` and cuts to 50 and 25 counts. Each move begins from where the joint holds still, within half a count for 0.2 s (at most 1 s, or the check is `not_checked`), not from where the last move's return was counted while the joint still moved (Tests on #1329). It passes when the body's goal moved the asked way and held still 0.2 s, and the joint is within 12 counts of where it was asked to go (the asked counts from where it began, or fewer if the body cut its goal shorter, as lamp-52e6's roll to one count), of the goal the body wrote, or anywhere between, within 2 s (`off_the_way`). A lifting head's goal is written 40 counts ahead (`GLANCE_LIFT_LEAD`): lamp-52e6's head stops about 30 short of it, and lamp-c0a4's, a head that follows, goes all the way, 80 counts for the asked 40 (Thirteen on #1340, 20 Oct 10:31). Both followed. Then the joint is back within 12 counts of where it began within 3 s of being let go. An axis whose goal the body moves under 5 counts either way (`FITTED_FROM`) is not fitted to move, as the roll on lamp-4ace and lamp-c0a4, whose units give it a reach of one count: it is named in the report (`head_roll_fitted`: no, and the pass's reason), never skipped silently, and the head move is judged on the other axis (the first builder on #1340, 20 Oct 11:09). Neither axis fitted is `not_checked`. The base, shoulder and elbow are never moved, only read. Nothing is asked unless the body's report on `diagnostics` says resting, standing (0.95 or more) and no joint at 62; `body/hand` is not pushed; and nobody else asked on `expression/body` in the last minute: otherwise `not_checked` with the reason, and a folded lamp is not stood up for it (the factory's steps stop `lamp-expression`, wait the minute and stand it before the run, #2232, install.md; on demand with `lamp-expression` running it stays `not_checked`). A hand, any request on `expression/body` from someone else (all of them are heard while it runs), the lamp leaving rest or starting to fold during it, the easing back and the return included, ends it, `not_checked`, as does a newest `body/joint_states` more than 1 s old when a move begins ("lamp-body is not publishing body/joint_states", not a false start from a kept reading), or nothing new on it for 1 s during a move or its return, on the check's own clock ("lamp-body stopped publishing body/joint_states during the move", not a red fail for a head that was moving; the reviewer on #1276); each of the four moves is judged, and one whose goal never moved is `not_checked`, naming it (none moved: the body runs without `LAMP_EXPRESSION_BODY=on` or with `--rest-glance off`); a failed move is reported and nothing more is asked in the run. Never at first boot | readback done; the move built (#812, the fifth builder's terms), not yet run on a lamp |
| ring | lamp-voice | at the factory and on demand, each of the ten colours (level 3) asked for on `light/test`, which the ring's owner shows for 2 s at most without touching the person's light, and seen on `light/drawn`, which it says only from its own write receipts, within 1 s, held 0.3 s. All drawn passes; some not drawn fails, naming them; none drawn is `not_checked` (the ring does not draw under the privacy switch or a turn); a light asked for on `light/command` during it ends the check and is `not_checked`. The self-test never writes `light/command`. At first boot `light/state` is only listened to: fresh is `not_checked` ("listened only"), stale or unreadable fails | done |
| camera | lamp-eyes | `humans/faces/tracked` published within 2 s (the eyes publish each look while the camera is open); otherwise the `eyes` item of `lamp/health`: a warning ("does not open", "stopped giving pictures") fails, ok means closed because seeing is not permitted now and is `not_checked`. The self-test never opens the camera itself, so the privacy switch stays authoritative | listens: done |
| touch pads, button | lamp-controls | `controls/touch_state` ready, no fault, nothing held 10 s or more, and the head's `head pads` item in the lamp's health, when fresh, not a fault: a pad left out since the head was first read, or no pad on any of its lines, fails as a pad held 10 s does, since a stuck pad is left out and never held (#1798; lines only not probed pass, said as `head_pads`); and at the factory only, `touch_by_hand`: the person is asked on the console to swipe the strip, touch the head and press the button, and all 20 s are watched (a swipe is read to its end) for each source to feel the hand; the electrodes each felt are listed, the strip's from the bits of its `active_mask` (`E0` to `E11`), the head's by pad name, so one that never fires shows; a source not felt is `not_checked` and named, but a head not felt while lamp-controls' `head pads` item in the lamp's health isn't well (no pad on the lines the pins name, or a pad left out since Controls started; [controls.md](controls.md), "Said when no pad is there") is a `fail` that gives the item's words: pads that don't read are said, not taken for nobody touching (#1798, `hand::head_unread`). Lines the item says were only `not probed` (another program held them) are an unknown: still `not_checked`, with its words. The item is taken only while fresh, its health and its own age together within 3 s (`HEALTH_FRESH`), so an old warning never fails a head fixed since. `lamp/health` keeps an item's words and not its values, so the kind is read from the words lamp-controls uses, which the test pins against its own `HeadLines::health`. Each source is one `controls/touch_state` lists: with the base strip off (unless `LAMP_CONTROLS_STRIP=on`, [controls.md](controls.md), "The base strip is off"), there is no strip source, so the head and the button are judged. The console's prompt still asks for a swipe, which nothing reads; changing that prompt is the self-test owner's, left as it is here | idle done; by hand done |
| air | lamp-air | `air/state` fresh (15 s) and in range; one that read this boot and has gone stale fails (lamp-4ace on 17 Oct: its sensor answered, then 20–30 reads a minute failed). With no reading since boot (the bus is under `/run`), `lamp-air`'s own word decides: a sensor that never answered since `lamp-air` started, which `lamp-air` reports at `ok` as "not fitted: no air sensor has answered since start" after three misses (its journal says "no air sensor answers on I2C bus N"), is `not_fitted` (the sensor is optional; lamp-06c1 and lamp-cb94 have none, the first builder on 17 Oct); "the sensor does not answer", in those first looks, is `not_checked`; "the air sensor answered but gives no reading", said instead once it has answered by name or with a reading that failed its CRC, fails (lamp-4ace after a reboot: a fitted sensor that is faulty or unseated, the Seventh and the reviewer on #1324), as does any other warning, such as a bus that cannot be opened. The words are `lamp-air`'s own constants (`watch::LOOKING`, `NOT_FITTED`, `ABSENT`, `NO_READING`), and a test runs `lamp-air`'s loop against both kinds of sensor | done |
| board | (none) | `board/state` from `lamp-health`: hottest under 85 °C, 3% of the disk free or more | done |
| storage | (none) | `/dev/kmsg`, read to its end. Fails only on what hurts the SD card, since boot, and only on a line naming the card (`mmcblk`): an I/O error (`blk_update_request: I/O error, dev mmcblk1, …`, `Buffer I/O error on dev mmcblk1p1, …`), an ext4 error (`EXT4-fs error (device mmcblk1p1): …`), its filesystem remounted read-only (`EXT4-fs (mmcblk1p1): Remounting filesystem read-only`), or a task blocked on it (`INFO: task jbd2/mmcblk1p1-8:187 blocked for more than 120 seconds.`). The same lines for another device (a USB stick's `sda1`) pass, and a hung task that does not name the card (`INFO: task lamp-voice:812 blocked …`) passes, counted as `blocked_other`. The SD/MMC hosts' re-tune (`sunxi_mmc_host-4021000.sdmmc:[INFO]: start tuning, …`, often followed by `[ERR]: wait dma hold bit clear timeout`) passes. Every board does it about hourly: lamp-52e6 every 3,670 s for each host, lamp-4ace every 61 minutes, and lamp-75c9 shows the same lines with its period not measured (#784). The parts ride it out since #791 and #911. Re-tunes (`start tuning` lines) and DMA timeouts are counted. Two re-tunes of one host under 3,000 s apart still pass, and the reason says "more often than hourly" with the gap. That reason is only in the run's JSON: `lamp-health` reads only failed parts' reasons, so its item says "passed". These pass too, because they hurt only once an I/O or ext4 error follows: `mmc1: tuning execution failed: -5`, `mmcblk1: error -84 transferring data …` (a CRC error, retried), `mmc1: card … removed`, and `Aborting journal on device mmcblk1p1-8.`. Not read: a root mounted `ro` from boot with no remount line. The kernel's log cannot say that a stall ended a part, so a part a short stall ended shows in its own item; unreadable (no `CAP_SYSLOG` under `dmesg_restrict`) is `not_checked`, and so is a log that no longer holds the whole boot (its first record kept is not sequence 0, or records were overwritten while read) with no fault in what is left; a fault kept still fails | done |
| voice route | lamp-voice | With `LAMP_VOICE_ROUTE=second` in the voice's unit (`/etc/systemd/system/lamp-voice.service` and its drop-ins, read as systemd reads them), whether the files the voice needs are there: `LAMP_VOICE_SECOND_CONFIG`'s configuration, every key file it names (each `*_credential_file`) and every `LoadCredential=` source. A source that is a folder passes, empty or not, as systemd loads it file by file: the person's own keys' folder (`own-keys-voice.conf`, #2094), which the voice reads only if it holds keys and otherwise uses the relay's; one the unit's `ConfigurationDirectory=` names passes even before it is there, since systemd makes it at start (lamp-06c1 on 21 Oct, its voice folder opened, was a false `fail`, "the unit's credential own /etc/lampOS/voice/own is not a file", from a build before folders were taken). A key is never read: each file is only looked at, as there, a regular file and not empty. Run as `lampos` (the timer's), root's configuration can't be read, so its key is taken as the relay key install places, `/etc/lampOS/voice/credential`, and the reason says so. One missing, empty or not a file fails, naming each ("the second route's configuration /etc/lampOS/voice/second.json is missing: the voice does not start on the second route"), since the voice then does not start and never falls back to Live (#1712). `second` without a configuration fails. A file that can't be looked at at all (its folder closed to `lampos`, as lamp-06c1's `/etc/lampOS/voice`, root's 0700, on 21 Oct) is not missing: the voice runs as root and may have it, so with nothing else wrong the part is `not_checked`, naming each file, and never `fail`; the lamp's health is not made `error` for it. Gemini Live (any other route) is `not_fitted`, and a unit that can't be read is `not_checked` | run on lamp-06c1 on 21 Oct: a false `fail` there (its voice folder closed); the `not_checked` for it is not yet run on a lamp |

A reading never given is `not_checked` (its owner may not run on this
lamp); one older than its freshness, or unreadable, is `fail`. A topic
never given a reading is not the end of it: the room microphone and the
joints then ask their owner's item in `lamp/health` (`ears`, `body`), and
the owner's own warning fails the part with its words. The air asks
`lamp-air`'s item too, judged by `lamp-air`'s own words (the row above):
lamp-4ace on 2026-10-16, `lamp-air` alive since a power cut and no reading
yet, is `not_checked` while it is still looking. Touch with no `controls/touch_state` but a fresh `controls/state` is
`not_checked`: Controls runs, and whether it reads the pads is not known. Each joint
of the five is its own part. Run it with

```sh
lamp-selftest [--occasion factory|first-boot|on-demand] [--dir ABSOLUTE] [--bus-root ABSOLUTE]
```

The storage part reads `/dev/kmsg`, which a lamp with `kernel.dmesg_restrict = 1`
(lamp-75c9, 2026-10-16) opens only to a process with `CAP_SYSLOG`; the
journal is no substitute there (two kernel lines for the whole boot). A unit
that runs the self-test gives it `AmbientCapabilities=CAP_SYSLOG`; run by
hand without it, storage is `not_checked`.

On lamp-4ace the unit is `deploy/lamp-4ace/lamp-selftest.service`: a
oneshot, started only by hand (`sudo systemctl start lamp-selftest`), never
at boot, as `orangepi` with `CAP_SYSLOG` and no other capability,
`--occasion on-demand` on `/run/lampos-bus`, keeping its runs in
`/var/lib/lamp-selftest` (its own `StateDirectory`: the other parts keep
theirs under `/var/lib/lampOS`, which is root's alone, so `orangepi` cannot
reach a directory made inside it; lamp-4ace, 2026-10-16 06:43).

On a product lamp (#812) the passive run is `deploy/product/lamp-selftest.service`, started only by
`lamp-selftest.timer`: five minutes after the lamp starts (so the first boot after setup, #806,
once its parts have had time to say how they are) and every six hours after, while the lamp runs.
It is `--occasion first-boot`: nothing lit, played or moved, every part judged from what it
already says. As `lampos` with `CAP_SYSLOG` alone, keeping its runs in `/var/lib/lamp-selftest`.
The timer is in `lampos.target` and waits for setup's marker; the service has no `[Install]`, so
nothing else starts it. Exit 3 (a part not checked) counts as the unit's success; 1 does not.
It never begins inside a conversation (`quiet`, the first builder on #812): it waits
until `voice/state` has said `resting`, or nothing fresh within 3 s, on five looks in a row,
one a second, since the voice rests for a moment between the lamp's answer and the person's
next words too (the seventh on #1245). It looks for at most two minutes, and leaves a
conversation longer than that for the next run, exit 0, saying so.
A run a person asked for begins at once. Passive, it wakes nothing in sleep hours.
A passive run that failed a part checks that part again half a minute later
(`lamp_selftest::again`), and keeps the fail only if it fails twice: a part restarting
at that moment, or a hand resting on a pad, would otherwise mark a sound lamp
`error` for six hours (the reviewer on #1245). Every timed run reports
`occasion: first_boot`, the passive one. The timer comes with `lamp-update install`,
which writes and enables the units, and an update round adds it to a lamp that
lacks it, with the self-test it starts (updater 8 and later, updates.md, "A part
added over the air").
`lamp-health` then carries the newest run in `lamp/health`, so a part that failed since is said
before its person finds out. The factory's run and an on-demand one are started by hand.

Exit status: 0 every part passed or is not fitted, or a first-boot run was
left for the next time because the voice stayed in a turn; 1 a part failed,
or the bus could not be opened; 3 a part could not be checked; 2 the
arguments were wrong.

## Shown on lamps

- **lamp-75c9**, 2026-10-16, V1 runtime only, an empty bus: every part
  `not_checked` with its reason, exit 3, 2.0 s, nothing lit or moved
  ([evidence](verification/selftest-lamp-75c9-20261016/)).
- **lamp-4ace**, 2026-10-16 01:14, by the first builder, on its bus with
  sudo (#812): both microphones, board, camera, the five joints and the ring
  (ten colours seen, the light put back) pass; storage failed on 9 SD/MMC re-tunes that timed out since boot, as `dmesg` agrees
  (a re-tune passes now: it is every board's hourly round, #784); air and
  touch were `not_checked` for the two reasons mended above.

The speaker check is built on both sides. Until #968 the voice played a tone (#895; played and passed on lamp-4ace by the first builder on 2026-10-16 at 03:46); it now reads the lamp's own speech instead (shown on lamp-52e6, [verification/echo-lamp-52e6-20261016](verification/echo-lamp-52e6-20261016/README.md)). The head's small move is built and passes against a body faked on a bus in its tests as lamp-52e6's (a lifting goal written 40 ahead, the roll cut to one count), which follows, sticks, takes nothing, folds, or is asked by Expression too; it has not run on a lamp yet (lamp-75c9 runs no lamp-body).
