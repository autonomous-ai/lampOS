# The lamp in the Autonomous app (`lamp-cloud`, #1396)

The Autonomous app manages every device the company builds. A lamp shows as
online there once the backend sees it on the company's MQTT broker and
answering, and the app's device page asks it things through the same broker.
V1's os-server did this; `lamp-cloud` does it for V2, on V1's wire, so the
backend and the app need nothing new to see a V2 lamp. **Off unless
`LAMP_CLOUD=on`.**

The design was agreed on #1396 (the first builder, 17 Oct 21:52 and 22:00),
on V1's contract as Nine wrote it down from V1's code and El checked it
against lamp-06c1.

## What it does

- **Stays on the broker.** MQTT v5 (`rumqttc`, pinned) to
  `mqtt://{mqtt_endpoint}:{mqtt_port}` with the account's login, V1's client
  id `os-server-{device_id}`, a clean start and a 60 s session expiry, as V1.
  The keepalive is 60 s (V1 asks 65,535 s, which never notices a dead link).
  On every connection it subscribes to `fa_channel` at QoS 1 and says `info`
  unasked on `fd_channel`, so the backend sees it at once. A connection that
  fails, or a login the broker refuses, is tried again after a wait that
  doubles from 1 s to a minute, and the journal hears of it once, not on
  every try.
- **Answers on `fd_channel`, at QoS 1, in V1's shapes** (`mqtt.md`): every
  reply carries `device` (`lamp`), `generation` (`lamp-v2`), `type`, `version`
  (the installed build, `<sequence>+<commit>`), `id`, `mac` (the lamp's
  `lamp-XXXX` name), `time`, `wakeword_enabled` and `voice_input_mode`, and
  `timezone` and `local_ip` when known.
  - **`generation`** is how a Lamp 2 is told from V1's: the backend keeps one
    device type, `lamp`, for both, and routes a lamp by `device`, so that
    stays `lamp`. The backend keeps the `generation` of the unasked `info` on
    the device's record, and the app picks the Lamp 2 page by it (#1396, the
    first builder and the backend, 19 Oct). V1's readers pass over a field
    they don't know. Setup's status says the same (`docs/setup.md`).
- **Sends health and crash reports** with `LAMP_REPORTS=on` and the
  person's yes (#1683): numbers and listed names only, on
  `<fd_channel>/report` at QoS 1 ("Health and crash reports", below).
- **Pings the backend every 15 s**, as V1's `status_reporter`: `POST
  {base}/ping`, where `base` is `llm_base_url` with a trailing `/v1` cut,
  `?mqtt=true` while no broker is kept, with the voice's key as the bearer
  and V1's `PingPayload` fields V2 has. A reply that carries a different
  broker (`mqtt_server`, `mqtt_port`, `mqtt_usr`, `mqtt_pwd` and the
  channels) or a new `device_id` replaces the account: the connection is made
  again with it, and the rotated login is kept in `lamp-cloud`'s own state
  folder (`/var/lib/lampos-cloud/rotated.json`, 0600), preferred over
  setup's while its device id is the same. A failing ping is said once, and
  once when it answers again. When the lamp's address moves, the HTTP
  client is made afresh, as V1 drops its idle connections.

## The device API

Over MQTT only: the app talks to the lamp through the broker, with the
device's credentials and topic ACLs (`mqtt.md`), so the page works away from
the home network, and V1's local admin, Fo, MCP and `/api/health/live`
routes are not served. Each kind is carried out by the part that owns it.

| `cmd` / `kind` | Answer |
|---|---|
| `info` | the identity fields above |
| `system.info`, `system.version`, `system.network` | V1's shapes: the versions (`firmware`, which the app's About reads, and `build`, both the installed build; `lamp-cloud`); the default route's interface, its address, MAC, Wi-Fi name (asked of `iw` within 0.5 s and kept a minute, so a hung `iw` never holds the keepalive; one that outlives its kill, asleep in the Wi-Fi driver, is never waited for: it is reaped at a later ask, and no other `iw` starts while it is there) and gateway; the host's name, device id, uptime and zone |
| `volume.get`, `volume.set` (`{"volume": 0..100}`) | through `lamp-actions`' socket (`volume_level`, `volume_to`, `actions.md`), its only writer: `{"volume", "raw", "max_volume"}`, a set read back |
| `mic.get` | `{"muted", "hw_switch_muted", "available"}`: the voice's own mute (`voice/mute`), the privacy switch (`voice/privacy`, `null` while unknown) and whether the voice is running |
| `lamp.health` | `lamp/health`'s `level` and `message`, and `stale` |
| `led.resting.get` | the person's own light (`light/state`, `docs/ring-choreography.md`), in V1's snapshot: `mode` (`off`; `default` while it is the light the lamp starts with, warm at the lowest level; else `custom`), `color` (`[r, g, b]` for `custom`, else `null`), `default` and `effective` (`{"effect", "color"}`), and `light`, the light by its own names (`on`, `colour`, `level`) |
| `led.resting.set` (`{"mode": "default" \| "off" \| "custom", "color": [r, g, b]}`) | asked on `light/command`, as the actions program asks it, and read back from `light/state` within 1 s; the snapshot of what is shown. `off` keeps the colour and the level, and black is `off`, as V1 |
| `led.resting.preview` (`{"color": [r, g, b]}`) | asked on `light/request` at `conversation` priority (over a skill's glow, under the person's own light), on the host's boot clock, holding 10 s unless asked again, so an abandoned screen leaves nothing behind (V1's 10 s); a save gives it back first. A light asked for at the lamp ("make your light blue") ends it, and a drag that goes on stays given way until it pauses past its 10 s (the reviewer on #1464). Answered only when it fails, as V1, so a drag does not flood `fd_channel`. Refused where the ring takes no lights that lapse (`LAMP_LIGHT_REQUESTS`, on in the product) |
| `chat.send` (`{"message", "session_id", "speak"}`) | with `LAMP_MESSAGES=on`: acked with `{"run_id", "session_id"}` and put on `chat/in`; the answer goes out after it as `chat.event`s, in V1's shapes ([messages.md](messages.md)). Refused at once when nothing on the lamp takes texts, and for a photo or a file, which aren't read yet. Without the switch, V1's "not on this lamp" |
| `reports.get`, `reports.set` (`{"share": bool}`) | with `LAMP_REPORTS=on` (#1683, "Health and crash reports" below): `{"share", "since", "queued", "by", "held", "at"}`, whether the person's yes stands, since when (RFC 3339 UTC, or `null`), how many reports wait, and how the newest choice was made: `by` is `voice` (only ever a stop), `app` or `setup` (`null` for a choice kept before these were), `held` is true for a voice's stop from a line whose answer was held, and `at` is when (as `since`), so the app's row can say "Stopped by voice at 3 pm" (#1880). A set takes only a JSON bool (else `"share must be true or false"`), is kept and read back; a no deletes what waits and leaves one `stopped`. The only way to turn reports on after setup. Without the switch, V1's "not on this lamp" |
| `skills.list` | with `LAMP_SKILLS_PAGE=on` (#1722, "The skills page" below): `{"version", "skills", "left_out"}`, every skill on the lamp as `lamp-skills` loads it, each with its group (`core`, `installed`, `made_here`), whether it can be removed or turned off, and what it may use in plain words. Read only. Without the switch, V1's "not on this lamp" |
| `mic.set` | refused: "on this lamp the microphones are muted and unmuted with the button or the switch". The person's own switch and button are the only way |
| `device.soft_reset` | refused until the owner says otherwise: a destructive act from anywhere is the owner's call |
| `device.rename` | refused for now; step 2, through setup's record |
| anything else (`add_channel`, the other `skills.*`, `face.*`, `tts.*`, the other `led.*`, `system.reboot`, …) | V1's `failure`, `"not on this lamp: <kind>"` |

### The light: V1's colours on this lamp's

V1's resting light is any `[r, g, b]`. This lamp's light is the person's
own, as a voice asks for it: on or off, one of ten colours by name, a
level from 1 to 5 (`lamp_msgs::Light`). So a colour the app sends is the
nearest of the ten (its channels against each colour's tint, both scaled
to their brightest, `Light::TINTS`, the table the ring draws from), at the
level its brightest channel reaches, rounded up so a faint colour is the
lowest level and not off. A light is told to the app as its tint at its
level's share of 255, so each of the fifty lights read and saved unchanged
is the same light. Two differences from V1 the page should know:

- a colour between two of the ten comes back as the nearer one: the page
  shows what the lamp took, from the reply;
- the light lasts until the board restarts, as a voice's does
  (`docs/ring-choreography.md`), where V1 keeps the choice on disk; after a
  restart the page reads `default`.

## Where things are kept

- **Setup's account**, `/etc/lampOS/setup/backend.json`, root's, 0600:
  `device_id`, `mqtt_endpoint`, `mqtt_port`, `mqtt_username`,
  `mqtt_password`, `fa_channel`, `fd_channel` and `llm_base_url`, as the
  app's setup request sends them. Setup writes it (#1401, `setup.md`), and
  `tests/setup.rs` reads the product's own setup settings and setup's own
  writer, so the unit's path and the fields can't drift from setup's. The
  voice's key stays in `/etc/lampOS/voice/credential`.
- **The unit** (`deploy/product/lamp-cloud.service`) runs it as `lampos` and
  hands it those two files and `lamp-update`'s `state.json` (for the build)
  through `LoadCredential=`, so no file is widened. It starts only once all
  three are there and setup is done. The drop-in that turns it on carries
  both `Environment=LAMP_CLOUD=on` and `Conflicts=os-server.service`
  (V1's `os-server` holds the same client id), so an off `lamp-cloud`
  never stops `os-server` (the reviewer on #1418; the unit's header has
  the drop-in).
- **Never printed:** the broker's password and the key appear in no journal
  line and on no bus topic; the account's debug form hides them.
- **The reset** empties `/var/lib/lampos-cloud` (with the reports' queue and
  the person's later choice on them) and removes
  `/etc/lampOS/setup/backend.json` and `/etc/lampOS/setup/reports.json`
  with the rest of the person's data.

## The skills page (#1722, `skills.list`)

The Autonomous app's Lamp 2 skills page (mobile's design on #1722, the
first builder's yes at 17:21 on 19 Oct): the lamp's skills as a phone's
home screen of apps. `skills.list` is the first of four kinds; `remove`
comes next, then `catalog` and `install` once the update channel carries
skills. Only `skills.list` is built, and it is read only.

**The switch.** `LAMP_SKILLS_PAGE=on`, with `LAMP_CLOUD=on`; off unless
set. Without it the kind is answered as any this lamp lacks, `not on this
lamp: skills.list`, and the app shows no Skills row. Its envelope is V1's,
as `reports.*`'s.

**Where it reads.** `lamp-skills`' own folder, `/var/lib/lampOS/skills`
(the product unit's `--dir`), with `lamp-skills`' own loader
(`lamp_skills::load_each`), so the page says what `lamp-skills list` says,
a tool's name taken among them; no socket, topic or copy of the schema of
its own. The folder is 0755 and its files 0644, so `lamp-cloud` reads it as
`lampos` under its unit's `ProtectSystem=strict` with nothing widened. On
this Mac the 13 skills of `lampOS/skills/` are read in 32 ms (a debug
build; not yet timed on a lamp). A folder that can't be read is answered
"the lamp's skills can't be read now".

**The reply:**

```json
{"version": 1, "left_out": 0, "skills": [
  {"name": "dance", "title": "Dance",
   "description": "When someone asks the lamp to dance (…)",
   "group": "core", "version": "1.0.0",
   "can_remove": false, "can_turn_off": false, "off": null,
   "loads": true, "why": null,
   "may_use": [
     {"name": "speak", "says": "speaks, or plays a short sound", "privacy": null},
     {"name": "light", "says": "changes the light", "privacy": null},
     {"name": "body", "says": "moves the lamp", "privacy": null},
     {"name": "hearing", "says": "hears what is said to it", "privacy": "the privacy switch stops it"}]},
  {"name": "desk-tutor", "title": "Desk tutor", "description": "…",
   "group": "installed", "version": "1.0.0",
   "can_remove": true, "can_turn_off": false, "off": null,
   "loads": true, "why": null,
   "may_use": [
     {"name": "speak", "says": "speaks, or plays a short sound", "privacy": null},
     {"name": "light", "says": "changes the light", "privacy": null},
     {"name": "body", "says": "moves the lamp", "privacy": null},
     {"name": "camera", "says": "uses the camera when it runs", "privacy": "the privacy switch stops it"}]}]}
```

- **`version`** is the reply's own: adding a field keeps 1; removing,
  renaming or retyping one is 2. A skill's own version is its `version`.
- **`skills`**, core first, then installed, then made here, by name in
  each; at most 64, the rest counted in `left_out`.
- **`name`** is the folder's (the skill's); **`title`** its `SKILL.md`'s
  first `# ` heading (at most 80 bytes), else the name; **`description`**
  its `SKILL.md` description (at most 1,024 bytes).
- **`group`**: `core` for `kind = "core"` (comes with the lamp); `made_here`
  for an installable skill whose `SKILL.md` front matter says `made: here`,
  what a skill a person makes on the lamp by talking to it carries (the
  first builder's 17:21: its own group, "Made on this lamp", removable);
  `installed` for any other. A core skill that says `made: here` is still
  core. Nothing writes `made: here` yet: the lamp skill creator will, when
  it makes one.
- **`can_remove`**: false for a core skill, true for the others.
- **`can_turn_off`**: the skill has a `[stop]` (#1886) and loads; the
  app's switch on a core row. No product skill has one, so it is false for
  every product skill; the wellbeing skills (#1685, installable) share one.
- **`off`**: with a `[stop]`, true when the skill is turned off, as
  `lamp-skills --state-dir` keeps it (#1886, `/var/lib/lampos-skills-state/off`,
  a name a line), false when it is not; it may be snoozed until the morning
  then, which `lamp-skills` keeps only in memory. Null without a `[stop]`,
  or when the off file can't be read.
- **`loads`** and **`why`**: whether the running `lamp-skills` loads it, and
  if not, why, in its words on one line (at most 300 bytes). A skill that
  doesn't load stays in its group, with its kind, version and permissions
  as its `skill.toml` says them.
- **`may_use`**: what its `[permissions]` give, each once, in this order,
  each with its line, and `privacy`, the privacy switch's line, for what
  the camera or a microphone gives:

  | `name` | `says` | `privacy` | From |
  |---|---|---|---|
  | `speak` | speaks, or plays a short sound | — | `speak` |
  | `light` | changes the light | — | `light` |
  | `body` | moves the lamp | — | `body` |
  | `camera` | uses the camera when it runs | the privacy switch stops it | `look` |
  | `faces` | notices faces in front of it, with the camera | the privacy switch stops it | `humans/faces/tracked` |
  | `presence` | notices whether someone is at the desk, with the camera | the privacy switch stops it | `room/presence`, `room/activity` |
  | `hearing` | hears what is said to it | the privacy switch stops it | `voice/heard`, `name/heard` |
  | `room_sound` | hears how loud the room is | the privacy switch stops it | `ears/level`, `ears/sudden` |
  | `touch` | feels a touch on the lamp | — | `controls/touch`, `controls/gesture` |
  | `air` | reads the air sensor | — | `air/state` |
  | `reminders` | knows when a reminder is due | — | `schedule/due` |
  | `tasks` | knows how the work you handed over is going | — | `tasks/status` |
  | `memory` | reads what you asked it to remember | — | `memory/rules` |
  | `lamp_state` | knows what the lamp itself is doing | — | any other topic (`voice/state`, `lamp/health`, …) |
  | `actions` | uses the lamp's own controls | — | `actions` not empty |

`tests/skills_contract.rs` pins the reply's fields by name and type, in
V1's envelope through JSON, from `Real` over `lampOS/skills/`, and the
refusal's exact words without the switch.

## Health and crash reports (#1683, step 1)

If something on a lamp breaks in a home, the company should know and fix it
in an update. The owner's verdict (`v1-verdicts.md`): "Health and crash
reports reaching the company: **System**, opt-in at setup: only health and
crashes, never sound, pictures or words." Agreed on #1683
([the design](https://github.com/autonomous-ai/lampOS/issues/1683#issuecomment-6259598135)
and the first builder's yes; the backend agreed the record on #1683, with
[two changes](https://github.com/autonomous-ai/lampOS/issues/1683#issuecomment-6259804444):
a sub-topic and `id`). This is the lamp side
(`crates/cloud/src/reports.rs`); the app's screen and the backend's store
are built by their own builders.

**Off unless set.** Nothing is sent, read or written for reports unless all
three are on: `LAMP_CLOUD=on`; `LAMP_REPORTS=on` in `lamp-cloud`'s unit
(and in the voice's, for the stop by voice); and the person's yes, read at
each send, never kept past a change. Without the switch `lamp-cloud` starts
no driver, subscribes to nothing, writes no file, and answers `reports.get`
and `reports.set` as kinds it lacks. Without the yes, the driver only says
`reports/state` (`share: false`) and reads nothing else.

### What is sent

Every field is a number, a boolean, an enum, an RFC 3339 time the lamp
writes, or a name from a fixed list, by construction: a part's or a unit's
name in a report is a `&'static str` taken from the list, never the name as
it came; a build is hex digits and `+`; and no type in a report holds text
that came in. So no `message`, `reason`, `values` or journal line can reach
a report, whatever a part writes on the bus or in an exit note. The canary
test (`no_secret_planted_in_any_input_reaches_any_report`) plants a
person's name, a Wi-Fi name, an address, the host name, the zone, a key and
words said in every input, and finds none of them in any report of any
kind, nor in the app's answer.

Every report has:

| Field | Type | From |
|---|---|---|
| `type` | `"report"` | fixed |
| `schema` | `"lamp-report/1"` | fixed: adding a field keeps `/1`; removing, renaming or retyping one is `/2` |
| `device` | `"lamp"` | as every other reply: the backend routes by it |
| `id` | string | the device's id, as `info` carries it: the backend finds the device by it, and drops a message without one |
| `generation` | `"lamp-v2"` | as setup's status says it |
| `kind` | `health`, `crash` or `stopped` | |
| `seq` | number | counts up on this lamp; starts again after a reset |
| `sent_at` | RFC 3339 UTC, whole seconds | set once, when the report is made, and kept on every send again: the backend drops a duplicate by `(device, seq, sent_at)` |
| `clock_synced`, `uptime_s` | bool or null, number | `board/state` (a crash and `stopped` say null and 0) |
| `build` | `<sequence>+<commit12>` or null | `lamp-update`'s state |
| `dropped` | `{queue_full, over_rate, too_big}` | the lamp's own counts of what it did not send |

- **`health`** adds `level` (`lamp/health`; `stale` when it is past its
  lifespan), `parts` (`[{name, level, age_ms}]`, worst first, at most 32,
  named by the list; no `message`), `missing`, `other_parts` (names not in
  the list, and parts past 32), `board` (`cpu_load` 2 dp, `memory_used` and
  `disk_free` 3 dp, `hottest_c` 1 dp, `wifi_dbm`; zones' names are never
  sent; `cpu_c`, `throttled` and `throttled_s` are null in step 1, as they
  are only on the board's own diagnostic), `online` and `down_for_s`
  (`lamp/online`), `selftest` (`{verdict, failed, other_failed,
  not_checked, build}`, part names only, never a reason or a value) and
  `restarts` (each unit's `NRestarts` since boot, asked of `systemctl show`
  within 2 s, at most once a minute).
- **`crash`** adds `unit`, `result` (systemd's `$SERVICE_RESULT`:
  `exit-code`, `signal`, `core-dump`, `watchdog`, `timeout`, `oom-kill`,
  `start-limit-hit`, or `other`), `exit_code` (with `$EXIT_CODE=exited`),
  `signal` (`SIGSEGV` and fifteen others by name, when `killed` or
  `dumped`), `ran_s`, `restarts_since_boot`, `level_before` (that part's
  level in the last `lamp/health` read) and `panic`, null until step 2.
- **`stopped`** adds nothing: the last word when the person says no, so the
  backend deletes what it has of this lamp.

**The lists.** Parts: the product's `--expect` list
(`deploy/product/lamp-health.service`) and those `lamp-health show` knows
by name, plus `skills` and `cloud`. Units: the product's `lamp-*.service`
files, held the same by a test. Self-test parts: those `lamp-selftest`
names.

**Never sent:** sound, pictures, anything said or typed, any name (the
person's, a face's, a speaker's), Wi-Fi names or passwords, addresses, the
host name, the zone, reminders, tasks, memory, skills' contents, any
message, reason or journal line, and the person's key. The other replies'
identity (`mac`, `local_ip`, `timezone`) is not used: a report has its own
builder. The 15 s ping is unchanged.

### How often, and how it is sent

- **Health:** five minutes after `lamp-cloud` starts, then every 15
  minutes. A part at `error` for 60 s sends one at once, at most once an
  hour for that part. It waits while `voice/state` is not `resting`, up to
  60 s.
- **Crash:** each exit note is read within 10 s and handed over at once,
  at most 12 an hour and 50 a day across every unit; past that only
  `dropped.over_rate` counts up.
- **Size:** at most 4 KiB; one over that is not sent and counts in
  `dropped.too_big`.
- **The wire:** `lamp-cloud`'s own connection and login, on
  **`<fd_channel>/report`**, at QoS 1. Below the reply channel, so a report
  is never handed to the app's side as a reply (the backend). No reply is
  expected. A report is handed to the client only while connected and kept
  until the broker acknowledges it: the client numbers its publishes in the
  order they are handed over, so each acknowledgement is matched to its
  report. A connection that drops first loses the client's copy (a clean
  start), and the lamp hands the report over again, with the same `seq`,
  once it is back.
- **The queue,** `/var/lib/lampos-cloud/reports/`: crash reports are files
  there (0600), read back when `lamp-cloud` starts; only the newest health
  waits, in memory, so flash is written only when something crashes.
  Bounded at 64 reports or 256 KiB, nothing older than 7 days; when full,
  the oldest health goes first, and a crash only when nothing but crashes
  is left, each counted in `dropped.queue_full`.

### Crashes: the exit note

`lamp-cloud exit-note UNIT` runs as `ExecStopPost=` of every lampOS unit,
through one prefix drop-in, `/etc/systemd/system/lamp-.service.d/exit-note.conf`
(`deploy/product/exit-note.conf`). It writes only systemd's
`$SERVICE_RESULT`, `$EXIT_CODE` and `$EXIT_STATUS`, the unit's name,
`systemctl show`'s restarts and run time, the monotonic time and the boot,
to `/run/lampos-exits/<unit>@<at_us>.json` (root, 0644), the newest 8 per
unit. A `success`, or a unit not on the list, writes nothing; the note never
fails a stop (`-`); no journal is read. `lamp-cloud` reads the folder every
10 s and takes each note once, across its own restarts, until the next
boot. `lamp-update install` writes no drop-in, so this one goes on a lamp
with the switch, by hand for the trial. Core dumps and backtraces are never
sent; step 2's panic hook (`{file, line}` only) comes after step 1 has run
a week.

### The person's choice

- **At setup**, `"share_reports": true` in the setup request; only the
  JSON `true` is a yes. Setup keeps the answer, a no included, in
  `/etc/lampOS/setup/reports.json` ([setup.md](setup.md)), which
  `lamp-cloud` reads where it is. A lamp set up on its own page is off.
- **Later, in the app:** `reports.get` and `reports.set` (the device API
  above), the only way to turn reports on after setup: it is the person's
  own tap, from their account. `lamp-cloud` keeps that choice in
  `/var/lib/lampos-cloud/reports.json` (0600), which wins over setup's.
- **By voice:** "stop sending reports" calls `reports_stop`, a system
  function the voice declares only with `LAMP_REPORTS=on`
  ([tools.md](tools.md)). It puts `{"share": false}` on `reports/request`
  and answers from `reports/state` within 2 s. **The bus can only turn
  reports off:** `lamp-cloud` takes no `share: true` from it, since
  anything on the lamp can write there. There is no voice "on".
  **A held line's stop is done** (the first builder on #1722, 16:44):
  when the busy-room rule holds the line's answer, `reports_stop` still
  runs and the spoken answer stays held, since stopping only reduces what
  leaves the lamp ([attention.md](attention.md), "What happens to an
  answer that is held back"). It costs all of a no, below. **It is made
  visible, never silent** (the first builder on #1722, 18:44, on Third's
  read of #1880): the voice asks with `held: true`, and `lamp-cloud` keeps
  the stop with `by: voice`, `held: true` and when, says them on
  `reports/state`, and returns them in `reports.get` for the app's row.
  The voice says it only when asked about reports: `reports_state`
  ([tools.md](tools.md)) answers "Someone asked me to stop them at 3:04 pm
  today. They can be turned back on in the app." It is never announced
  unasked. The ruling's example said "say 'start sending reports'", but the
  voice can only turn reports off, so it points to the app. A stop when
  reports are already off changes nothing, so the stop that stopped them
  is the one kept. The journal says `request N: reports_stop done though
  the answer was held (consent withdrawn)`. #1796's spoken
  stop on lamp-06c1 had shown the call dropped with the answer.
- **A no deletes:** every queued report and its file goes at once, and one
  `stopped` takes their place. It is kept, on flash too, until it is sent,
  never dropped offline, and sent once more on the next connection: the
  broker's ack is not the backend's store, and a no lost there would last
  until the reports expire (the backend; deleting twice is harmless). A yes
  again drops a `stopped` already through once, so it never comes after
  the new reports. Nothing else is sent after it.
- **Factory reset** removes `/etc/lampOS/setup/reports.json` and empties
  `/var/lib/lampos-cloud` (the later choice and the queue); the next setup
  asks again, default off.

On the bus: `reports/request` (`ReportsRequest {share}`, an event) and
`reports/state` (`ReportsState {share, queued, by, held, at_s}`, kept,
every 5 s, lasting 15 s), in [msgs.md](msgs.md). `lamp-cloud` keeps `by`,
`held` and `at` beside `share` in its own `reports.json`; when several
asks come in one second, the stop is kept as held only if every one was.

### Changes from the design

- Setup's file is read where it is (0644) rather than handed in by
  `LoadCredential=`: a credential whose file is missing stops a unit from
  starting, and no lamp set up before this has one. No unit line changes:
  `ProtectSystem=strict` leaves `/etc` and `/run` readable.
- Added under `/1`: `dropped.too_big`, `selftest.other_failed` and the
  `other` result.
- From the backend on #1683: the topic is `<fd_channel>/report`, not
  `fd_channel`; the payload carries `id` (the design proposed none);
  `stopped` is kept until it is sent and sent once more on the next
  connection, rather than dropped when offline.
- "Are you sending reports?" by voice is not built: only the stop is
  declared, one function of the 32.

### The backend's side, as agreed

Reports are kept by device, apart from the account and the person's
profile, for 90 days, then deleted; not used for analytics, and report
bodies stay out of the backend's logs. Support reaches them only with the
lamp's id the person gives. `stopped` deletes that device's reports and
nothing more is kept until reports start again; removing a device from its
account deletes them too. **The store is up, in the test environment and in
production (#1683, 19 Oct)**, and the company broker takes a publish on
`/report`. What the backend reads from a report is in "What the backend
reads", below.

Not shown: no lamp has run it. The trial is on lamp-06c1 under a hold, with
a broker on the Mac first and the company's only once the backend says its
store is ready: turn latency with and without, `lamp-cloud`'s CPU over an
hour (under 0.5 % added), bytes over 24 h (under 200 KB), no flash write
without a crash, a `kill -SEGV` reported within 60 s, a crash loop held at
12 an hour, delivery after 30 min offline, the stop by voice and by
`reports.set`, and the reset.

## What the backend reads

What the backend takes from a lamp's messages on `fd_channel`, and when
(#1396). Field names and meanings only. Each later change to this contract
updates this section in the same pull request.

- **The `info` message:** the one said unasked on each connection, and the
  reply to `cmd: info`. The backend keeps facts about the device from it:
  - `id`: the device it is about. A message without it is passed over.
  - `device` stays `"lamp"`: the backend sorts a lamp's messages by it, so
    another value would change how the lamp is handled.
  - `mac`, the lamp's `lamp-XXXX` name: kept on the device's record. An
    `info` without it is passed over.
  - `version`: kept as the installed build.
  - `timezone`: kept as the device's zone, when it is a zone name the
    backend knows.
  - `local_ip`: kept as the lamp's last known address on its network.
  - `generation`, `"lamp-v2"` (#1731): kept on the device's record as
    `os_generation`, which the app reads to tell a Lamp 2 from a V1 lamp.
  - `type` says which message it is; `time`, `wakeword_enabled` and
    `voice_input_mode` are not kept.

  A field left out, or empty, never clears a stored value: a message
  without `generation` keeps the `os_generation` there.
- **The reply to a `data` kind** (`system.info`, `volume.get`, …) is
  passed back to whoever asked, the app, and is not stored. So a field that
  rides every reply, as `generation` does, is kept only from `info`, and a
  fact said only in a `data` reply reaches the app's page but never the
  device's record.
- **A health or crash report** (#1683), on `<fd_channel>/report` only; the
  same message on `fd_channel` itself is not a report. The backend reads:
  - `type` must be `"report"`, and `device` exactly `"lamp"`: anything else
    on `/report` is dropped.
  - `id` must be the device in the topic; a report without it, or for
    another device, is dropped.
  - `kind`: `health` and `crash` are kept; `stopped` deletes every kept
    report of that device and is not kept itself.
  - `seq` (a number, 0 or more) and `sent_at` (RFC 3339) are both needed:
    a report already kept with the same `(device, seq, sent_at)` is
    dropped as a duplicate.
  - `schema` and `generation` are kept with the report.
  - The whole report is kept as it came, at most 4 KiB; a larger one, or
    one that isn't JSON, is dropped. The other fields of "What is sent",
    above, are kept in it and not read one by one.

  A report is kept for 90 days from its arrival and is never passed to the
  app as a reply. Nothing is sent back to the lamp, so a report handed over
  while the backend restarts can be lost after the broker has taken it;
  that is why `stopped` is sent once more on the next connection.

## Not yet known, for the backend (#1396)

- Whether the backend's `connected` comes from MQTT, the ping, or both, and
  where the app's `lan_ip` comes from.
- Whether the issued port is plain MQTT (V1's `mqtt://`) or TLS, and v5 or
  3.1.1. The client is built with rustls, unused while the port is plain.
- `version` is the build when `lamp-cloud` started: `state.json` is handed
  in at start, so after an update it says the new build once `lamp-cloud`
  restarts with it.

## Tests

`cargo test -p lamp-cloud`: the account and its rotation, V1's shapes, each
kind's answer and refusal, each of the fifty lights back from its own
colour, the light asked of a stand-in ring on a bus (a save read back, a
preview at the person's priority for 10 s and given back before a save, a
ring that takes no previews or shows nothing in time said), a preview
answered only when it fails, the ping's URL, body and reply, and, against a
broker in the test built from `rumqttc`'s own codec: coming online as V1
(client id, keepalive, clean start, expiry, login), `info` on connect and
answers on `fd_channel`; a refused login backing off and said once; a dropped
connection made and subscribed again. A backend in the test checks the ping's
bearer and body, a failure said once, and a rotated broker kept. Not yet run
against the company's broker or on a lamp.

The reports (#1683): each kind's fields as sent; the canary; each of the
three gates closed sends nothing (no setup file, a no, one unreadable,
`"true"`, `1`, a newer wording; a later no of the lamp's own); the bus only
turning reports off; a no deleting the queue and its files with nothing
after but `stopped`; a no given offline kept across a restart, sent on the
next connection and once more on the one after; the bounds (12 an hour, 50
a day, 64 reports, healths before crashes, 7 days) and the 4 KiB cut; exit
notes (a success and a unit not lampOS's write nothing, 8 per unit, each
taken once across a restart, a new boot starting again); the drop-in's
line; the lists against the product's units; `reports.get` and
`reports.set` with and without the switch; and against the test broker, a
report on `fd/…/report` with `info`'s id leaving the queue on the ack, and
sent again after a dropped connection. `tests/report_contract.rs` pins the
fields by name and type, the topic, and `sent_at` kept on a send again.

The skills page (#1722): the reply's shape; a core skill never removable
and an installed one removable; a skill made here in its own group; each
permission's line and the privacy switch's; a skill that does not load, with
why; `[stop]` and the off file; the bounds; a folder that can't be read; the
switch's two gates and the refusal's exact words. `tests/skills_contract.rs`
pins the fields as sent.

`tests/app_contract.rs` holds the fields the Autonomous app's device page
reads, by name and type (Fourth on #1396): `versions.firmware`,
`network.private_ip` and `host.hostname` of `system.info`; `volume`, `raw`
and `max_volume` of `volume.get` and `volume.set`; `muted`,
`hw_switch_muted` and `available` of `mic.get`. Each is checked in the real answer, as sent.
The app reads them leniently, so a renamed or retyped field would otherwise
go blank on its page unnoticed. `hw_switch_muted` is null while nothing reads
the switch (the app takes that as not known, so not blocked by the switch), and `versions.openclaw` and `versions.openclaw_detected` stay out
of the reply: only V1's "Starting" rule waits for them.
