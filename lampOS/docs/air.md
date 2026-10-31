# Air: the air sensor

`lamp-air` (`crates/air`) reads the lamp's Sensirion SEN63C every five
seconds and says on the bus what the air is like. It decides nothing. With
it stopped the lamp runs on as before.

## The sensor

A SEN63C on I2C bus 0 at 0x6B (beside the base's touch strip on the same
bus), measuring particles (PM1, PM2.5, PM4, PM10), relative humidity,
temperature and carbon dioxide (`robots/lamp/hardware/components.md`).
Commands used: product name (0xD014), data ready (0x0202), read measured
values (0x0471, seven words, each with Sensirion's CRC-8), and start
measurement (0x0021), sent only when no new reading has come for 15 s,
as after a power cut. Every word's CRC is checked; a bad one is no value.
0xFFFF (and 0x7FFF for the signed humidity and temperature) means the
sensor does not know the value yet, and it is left out. CO2 says it as
either: for its first seconds after a start it reads 0x7FFF (lamp-4ace,
17 Oct 18:3x), which taken as 32,767 ppm would make every start Stuffy.

**Filler, a failed name, and a cause not reproduced.** On lamp-4ace on 17
Oct every look failed its CRC from at least 11:00 until 18:36, when the
reads came back after the hardware team swapped the sensor and a start was
sent by hand. The cause is not reproduced: at 20:13 to 20:19 the swapped
sensor, measuring, gave no `0xff` filler at 400 kHz or at 100 kHz (0 in
3,360 bytes each), and at 20:42 to 20:44, stopped (`0x0104`) and idle, it
gave none either (0 in 480) and its name read clean; the installed
`lamp-air` restarted it as it should (the first builder, #705 18:28 to
20:44). So the filler decode and the start when the name fails are guards,
not a shown fix; what is shown is that a sensor just started reads CO2
`0x7FFF` (the old `lamp-air` took it as 32,767 ppm, very stuffy). What was
seen at 18:2x, in the swapped sensor's first minutes on the bus: the SEN6x
speaks I2C standard mode only (100 kHz) and stretches the clock; lamp-4ace's
bus 0 runs at 400 kHz (`twi@2510000`'s `clock-frequency`), the master read
`0xff` while the sensor still held the line, and `0xff` bytes came between
the real ones,
up to two inside one word (the first builder, #705 17 Oct 18:28 and
18:30). So each answer is read at three
times its length and decoded by its CRCs (`words_among_filler`): each
word is two bytes and a CRC that matches, a `0xff` is taken as filler
first and as data only where no decode with it skipped holds,
backtracking over every `0xff` (at most 8 skipped in one place), so a
real 0xFFFF still reads as "not known yet". Filler first, because filler
can make a word: 0xFF00 is `ff 00 00` (its CRC is 0x00), and `ff 00 ff 00`
around a null word, with the inner `ff` skipped, reads as it. A reply that reads straight (its first words all with good CRCs) is taken as it is, before any backtracking, so with no filler a real `0xff` is never skipped for the bytes after the answer, whatever the sensor sends there (the reviewer on #1366: T 0x1BFF then CO2 0x0370, with `0x00` after, read 28,756 ppm without it). When the name still fails its
CRC (something answered), the measurement is started anyway and the
values tried, a guard for a name that fails while the values would read
(on lamp-4ace on 17 Oct the name failed all day; whether a start would
have brought the values back is not shown, and the branch has not run on
a lamp). The journal then says
"the sensor answers: an air sensor whose name failed (…); its
measurement started". A start it refuses (one already measuring may)
still leaves the values to try, and the loop starts it again after 15 s
with no reading. Nobody acknowledging is never started, so a lamp
without the sensor makes no more transactions. The journal says it once
a run: "the air sensor's answer to 0x… had N filler bytes (0xff)
between its words, skipped by their CRCs". The bus at 100 kHz (#1368) is the sensor's own
limit, not shown to be the fix.

A sensor that does not answer, or answers its name but gives no reading
(a half-seated plug, a CRC fault), is looked for three times, five seconds
apart, then once every ten minutes (`src/watch.rs`); only a good reading
counts it as back. Each look is an I2C transaction, and the kernel logs
every one nobody acknowledges ("ACK not received"): on lamp-cb94, which
has no sensor, a look every 200 ms made about 300 kernel lines a minute,
and the journal kept only about 90 minutes. The journal hears of it twice,
the first miss and when the sensor counts as absent. The health report
says it steadily: a sensor that never answered since `lamp-air` started is
`ok`, "not fitted" (a lamp without one is well), and one that answered, by
name or with words that failed their CRC, and gives no reading is a
warning with the last error. A reading already kept on `air/state` this
boot counts as an answer, so a restart of `lamp-air` never takes a fitted
sensor that now only refuses for an absent one (#1324). A sensor that stops answering is
looked for again 5 s later, then the same way; one plugged back in is found
within ten minutes, or at once after `systemctl restart lamp-air`. A bus
that cannot be opened (no `/dev/i2c-0`, or no permission) makes no
transaction, so it is looked at again every five seconds.

### The bus's clock

The SEN6x take I2C's standard mode only, 100 kHz (Sensirion's datasheet),
and the board runs bus 0 at 400 kHz; this keeps the bus within the sensor's
limit. It is not shown to be what made the sensor fail: on lamp-4ace on
17 Oct its answers came back with filler `0xff` bytes at 18:28, but from
18:36 they read clean at 400 kHz as at 100 kHz (0 filler bytes in 3,360,
20 of 20 readings each), after a start measurement sent by hand; a sensor
that was not measuring is the likelier cause (#1366). The
Allwinner controller takes a new rate at run time: `lamp-air limit-clock`
(`src/clock.rs`) reads the rate from
`/sys/class/i2c-adapter/i2c-0/device/info` (`twi->freqency`, the kernel's
spelling; the device tree's `clock-frequency` is only the rate it booted
with), writes `100000` to `device/freq` when it is faster, and reads it
back. A rate still above 100 kHz, a refused write, no rate, or no adapter after
10 s is an error,
and the unit fails; a bus already that slow is never written, and another
controller is left. V1's HAL does the same (`hal/drivers/environment/i2c.py`).
The product runs it as root at every start, before `lamp-air` and
`lamp-controls` (`deploy/product/lamp-air-clock.service`); a reboot puts the
device tree's rate back. Neither reader needs it to start.

The base's touch strip (the MPR121, 0x5A) is on the same bus and works at
100 kHz. The product reads no MPR121 today: `lamp-controls` starts its strip
worker only when the pins file sets `base` (`crates/controls/src/main.rs`),
and neither the product's nor lamp-4ace's does (#711). When it does, each
read checks a fifth of the configuration in turn (#1374), 354 clock bits
worked out (not measured): about 0.9 ms of wire time a sample at 400 kHz
and 3.5 ms at 100 kHz, under the strip's 5 ms pace (`controls.md`). The strip's rules are times, not counts
(`crates/controls/src/base.rs`, which already allows an 11 ms poll). The
adapter is waited for up to 10 s, in case its driver binds late; on a board
with no `i2c-0` at all, that holds the air and the controls back by about
9 s at boot, since both start after it. Started by hand while the air and
the controls run, the rate changes under them: at worst one transfer should
fail, and both try again (a change mid-transfer could in principle leave a
device holding SDA). Seen once on lamp-4ace (17 Oct, 19:25): written by hand
with both running, then no controls error and no failed air read. Proven on lamp-4ace (17 Oct, 20:13):
`limit-clock` lowered 400000 to 100000 and a second run left it; a 48-byte
read of the sensor took a median 4.60 ms at 100 kHz against 1.30 ms at
400 kHz, so the real clock changed. The strip is not read there (no `base`),
so it has not been seen at 100 kHz.

The temperature and humidity are those inside the base, warmed by the
lamp itself (33 °C on 2026-10-14 in a cooler office; on 17 Oct it climbed
from 35.5 to 44 °C in 6 s after a start, and the humidity fell with it):
they are not the room's, and nothing should speak of them as if they
were. Where the sensor sits in the base is the hardware team's question
(`lamp-4ace.md` question 5).

## What it says

| Topic | Type | Kind | What |
|---|---|---|---|
| `air/state` | `AirState` | latest, every 5 s | PM1, PM2.5, PM4, PM10 (ug/m3), humidity (%), temperature (°C), CO2 (ppm), and `feel`: fresh (under 800 ppm), fine (to 1,200), stuffy (to 2,000), very stuffy, or unknown |
| `diagnostics` | `Diagnostic` | every second | `air`: ok, or a warning with why the sensor does not answer ("the sensor does not answer: …" in its first three looks; then, for a sensor that has never answered since `lamp-air` started, an **`ok`** report, `not fitted: no air sensor has answered since start` (the sensor is optional, so a lamp without one is well; the journal keeps `no air sensor answers on I2C bus 0 (…); looked for again every 10 min`), or `the air sensor answered but gives no reading on I2C bus 0 (…)` for one that has, by name or with a reading that failed its CRC. The self-test reads these words, kept as `watch::LOOKING`, `NOT_FITTED` (and `ABSENT`) and `NO_READING`: the first is `not_checked`, the second `not_fitted`, the third a `fail` (#1324)); the feel, CO2 and PM2.5 |

The bands follow carbon dioxide because that is what a person at the desk
feels as a stuffy room; they are round figures, not a standard.

## On the lamp

    lamp-air --bus-number 0 --bus-root /run/lampos-bus
    lamp-air once        # the sensor's name and one reading, no bus
    lamp-air limit-clock --bus-number 0   # the bus at 100 kHz (root)

Unit `lamp-air.service`, in the product's units (`deploy/product/`) and
lamp-4ace's and lamp-52e6's deploy folders. Running on
lamp-4ace since 2026-10-14 17:43: CO2 1,856 to 1,872 ppm ("stuffy"), PM2.5
102 to 105 ug/m3 in the office at work.

## What the creature does with it

See [creature.md](creature.md). The voice's part, behind
`LAMP_AIR_FACTS=on` in its unit (off unless set, and set in the product's
voice unit; `crates/voice/src/air_told.rs`;
on lamp-4ace since 2026-10-14, 18:46, when the room read 1,560 ppm, "stuffy";
first told at 19:45 that evening, beside a played request for a story, and
the model ended the story with "By the way, the air here is getting pretty
stale; should we open a window for some fresh air?"):
it reads `air/state` every five seconds; once the air has felt stuffy (or
very stuffy) for ten minutes, the model is told so beside the next thing a
person says to the lamp, in a room that is not busy, with leave to suggest
a window once if it fits; not again for three hours. A reading older than
30 s counts as no reading. The quiet droop and the light a shade dimmer,
and since 25 Oct one short line with them, are the Stuffy air skill
(`skills/stuffy-air`, #1021, `docs/skills.md`), a core skill that install
puts on every product lamp switched on (`install::PRODUCT_SKILLS`).

The PM figures are read by two skills (#1684, `docs/skills.md`), installed
but off until measured (`install::PRODUCT_SKILLS_OFF`): Smoky air (`skills/smoky-air`)
says one exact line when PM2.5 jumps by 100 µg/m³ within three minutes to
150 or more, and Dusty air (`skills/dusty-air`) one a day when it has been
125 or more for an hour. Neither is a smoke alarm: the sensor has no VOC,
gas or carbon monoxide reading, and (as #1684's design reads Sensirion's
datasheet, not yet confirmed) it counts no particle under about 0.3 µm,
which fresh flame makes.

Their words never say fire, alarm or smoke alarm: Smoky air's line is
"The air reads like smoke. If something's burning, please check it.", and
Dusty air's "The air here has been very dusty for about an hour. I can't
tell where it's coming from." Neither has a `[stop]`, so a passing "stop"
said to the lamp cannot snooze them (#1837's P3). Their numbers are
provisional: four of Smoky air's cases (kettle steam, toast, a candle blown
out, a match) are placeholders, to be replaced by the real recording
(#1684), and the numbers are set again from those traces.

### Smoky air and Dusty air on a trial lamp

They are off unless put on: `lamp-update install` and a round write them
disabled, `enabled = false` above each `skill.toml`
(`install::PRODUCT_SKILLS_OFF`), and there is no switch to turn on. Both
say `kind = "core"`, so `lamp-skills install` refuses them ("a core skill
comes with the lamp"). For a trial, as root on the lamp, copy each folder's
own files over the installed ones, which `lamp-skills` looks at every 2 s:

    cp skills/smoky-air/skill.toml skills/smoky-air/SKILL.md /var/lib/lampOS/skills/smoky-air/
    cp skills/dusty-air/skill.toml skills/dusty-air/SKILL.md /var/lib/lampOS/skills/dusty-air/

`lamp-skills list` then shows both enabled. With no air sensor, publish `air/state`
by hand, as Stuffy air's check did
(`docs/verification/stuffy-air-lamp-06c1-20261017/`). To turn them off,
put `enabled = false` back at the top of each `skill.toml` (`lamp-skills
remove` refuses a core skill); a later round keeps a file the lamp changed.

## Not shown

- The PM figures against a reference instrument: one sensor, one office.
- What a power cut does to the sensor's measuring state (it is started
  again after 15 s without a reading; not seen on a lamp yet).
