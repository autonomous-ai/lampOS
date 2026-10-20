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
  It takes, and tells the broker it takes, packets of up to about 11 MiB
  (`MOST_PACKET_BYTES`): a `faces.add_photo` with the largest photo, in
  base64. `rumqttc`'s own limit, 10 KiB, dropped the connection on every
  photo (Mobile on lamp-0c89, 20 Oct).
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

**Only the lamp's own account channel commands it** (`online::commanded`):
a message on any other topic is neither done nor answered, not its reply
channel, another lamp's, a longer or shorter topic, nor another case. That
is the one guard the owner's kinds rest on (`faces.add_photo`,
`face.remove`, the reminders' cancel, #2214): whoever holds the lamp's
signed-in account speaks as its owner. **The owner's app acts for the whole
lamp, not one person at it:** it can cancel any reminder the lamp keeps,
one another person set by voice at the lamp included, as it can remove any
face the lamp knows.

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
| `skills.remove` (`{"name"}`) | with `LAMP_SKILLS_PAGE=on` ("Removing a skill" below): an installed or made-here skill taken off on the person's tap, asked of root's manage helper over its local socket (`lamp-cloud` never writes the skills folder); the reply is `skills.list`'s, read after it. A core skill, a name that is not a skill's and one not on the lamp are refused. Without the switch, V1's "not on this lamp" |
| `faces.add_photo` (`{"name", "photo"}`, the photo a JPEG in base64, at most 8 MiB) | with `LAMP_PHOTO_FACES=on` (#1968, "A face from the app's photo" below): the photo is handed to the eyes, which keep its face **unconfirmed** until its person says yes at the lamp. `{"added": NAME, "unconfirmed": true}`, or `{"remembered": NAME}` for a face already theirs; else the eyes' reason in plain words. Without the switch, V1's "not on this lamp" |
| `face.list` | with `LAMP_FACES_PAGE=on` (#2192, "The faces it knows" below): `{"faces": [{"name": NAME, "waiting": BOOL}, …]}`, the names the eyes keep and, `waiting`, a face from a photo still to be confirmed at the lamp. Names only. Without the switch, V1's "not on this lamp" |
| `face.remove` (`{"name"}`) | with `LAMP_FACES_PAGE=on`: the person off all of the lamp, the eyes' own forget and then everything memory keeps for them and their rules. `{"removed": NAME, "memory": "forgotten"}`, or `"memory": "kept"` with its `reason`; with `"finishing": true` while set-aside records it waits for aren't searched yet. `the lamp doesn't know that face` for a name it doesn't keep. Without the switch, V1's "not on this lamp" |
| `schedule.list` (`{"after_id"?: n}`), `schedule.cancel` (`{"id": n}`) | with `LAMP_APP_SCHEDULE=on` (#2195, "The lamp's timers and reminders" below): the list is a page of at most eight, `{"items": [{"id", "kind": "timer"\|"reminder", "text", "state", "local", "due_unix_ms", "repeat", "snoozed", "late"}], "next_after_id", "complete"}`, each as lamp-schedule keeps it; a cancel is `{"cancelled": true, "id": n}`, the timer or the whole reminder series. A refusal is the reminder client's fixed sentence: no longer active, changed meanwhile (list again), or not confirmed (it may have happened). A bad `after_id` or an `id` that isn't a whole number above 0 is refused before lamp-schedule is asked. Without the switch, V1's "not on this lamp" |
| `mic.set` | refused: "on this lamp the microphones are muted and unmuted with the button or the switch". The person's own switch and button are the only way |
| `device.soft_reset` | refused until the owner says otherwise: a destructive act from anywhere is the owner's call |
| `timezone.set` (`{"timezone": "Asia/Ho_Chi_Minh"}`), `person.name.set` (`{"name"}`, `""` removes it), `device.rename` (`{"name"}`, 1 to 64 characters) | with `LAMP_SETTINGS_CHANGE=on` (#1959, "Changed after setup" below): V1's `starting` at once, then `success` with what was kept (`{"timezone"}` or `{"name"}`), or `failure` with why. Without the switch, V1's "not on this lamp" |
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
  three are there and setup is done.
- **On in the product (#1815).** The unit itself carries
  `Environment=LAMP_CLOUD=on`. V1's `os-server` holds the same client id,
  so the unit stops it as `lamp-cloud` really starts:
  `ExecStartPre=-+/usr/bin/systemctl stop os-server.service`, as root, and
  going on where there is none.
  - **Not `Conflicts=`:** systemd queues that stop with the start job, before
    any condition is checked. So a `lamp-cloud` skipped for want of setup's
    account (a lamp set up under V1, or put back to it) would still stop
    `os-server`, and the app would see the lamp offline. That breaks the
    #1418 rule that an off `lamp-cloud` never stops it (the reviewer on
    #1909).
  - **Shown on lamp-c0a4** (systemd 252) with stand-in units:
    - `Conflicts=` with a condition that fails stopped the stand-in;
    - `ExecStartPre=-+` with the condition failing left it running;
    - with the condition met, it stopped the stand-in and started;
    - with no such unit, it started.
  - **What it leaves open:** with no `Conflicts=`, an `os-server` started
    later, by hand or by a V1 timer, is not stopped, and holds the same
    client id beside `lamp-cloud`. Each then pushes the other off the broker
    in turn. That only happens on a lamp put back to V1 with lampOS's units
    still enabled, which should be masked there (the reviewer's P3 on
    #1909).
  Install enables it with the lamp, and `lampos.target` starts it once
  setup's marker is there (`lampos-setup-done.path`). Setup writes
  `backend.json` before the marker. So a Lamp 2 set up from the app shows
  online straight away, and again after a reboot, with no `cloud-on.sh`.
  To keep it off on a lamp, mask it (`systemctl mask lamp-cloud`).
- **The lamp's name in the app**, `/etc/lampOS/setup/display-name`, root's,
  0644, one line, written only by root's helper after a `device.rename`
  (#1959). `system.info`'s `host.device_name` is it, else the host's name as
  before. The reset removes it.
- **Never printed:** the broker's password and the key appear in no journal
  line and on no bus topic; the account's debug form hides them.
- **The reset** empties `/var/lib/lampos-cloud` (with the reports' queue and
  the person's later choice on them) and removes
  `/etc/lampOS/setup/backend.json` and `/etc/lampOS/setup/reports.json`
  with the rest of the person's data.

## A face from the app's photo (#1968, `faces.add_photo`)

Off unless `LAMP_PHOTO_FACES=on`, with the photo folder
(`LAMP_EYES_PHOTO_DIR`, the folder given to `lamp-eyes --photo-dir`) and
the eyes' request socket (`LAMP_EYES_WHO_SOCKET`).

- **The folder is `/run/lampos-photos`**, `lampos`'s at 0700, made at boot
  by the product's tmpfiles (`deploy/product/lampos.conf`), and lamp-cloud's
  unit may write it (`ReadWritePaths=-/run/lampos-photos`). It belongs to
  neither unit, so it is there before both start, and an eyes or lamp-cloud
  stop, crash or update removes it from under neither (Reviewer-lead on
  #2263). A folder in the eyes' own `/run/lampos-eyes` was made by the eyes
  after lamp-cloud's namespace could be set up, and went with every eyes
  restart: then read-only to lamp-cloud under `ProtectSystem=strict`, and
  every `faces.add_photo` failed until lamp-cloud restarted (reviewer-two
  and Reviewer-lead on #2263, Mobile on lamp-0c89).
- **A lamp switching it on** sets both units, each in a drop-in. lamp-cloud's
  takes the three variables:

      [Service]
      Environment=LAMP_PHOTO_FACES=on
      Environment=LAMP_EYES_PHOTO_DIR=/run/lampos-photos
      Environment=LAMP_EYES_WHO_SOCKET=/run/lampos-eyes/who.sock

  The eyes' unit gives `--photo-dir` on its command line, so its drop-in
  clears `ExecStart=` and repeats the product's line (`systemctl cat
  lamp-eyes`) with `--photo-dir /run/lampos-photos` at its end:

      [Service]
      ExecStart=
      ExecStart=/opt/lampOS/eyes/current/lamp-eyes ... --photo-dir /run/lampos-photos

  The voice's asking at the lamp is its own switch (`LAMP_PHOTO_FACES` on
  lamp-voice, `docs/switches.md`).
- **On a lamp whose tmpfiles aren't yet the update's**, the folder isn't
  there: the `-` lets lamp-cloud start anyway, the eyes start without photos
  (they answer `photos_off`, and their camera, presence and recognition run
  as before), and `systemd-tmpfiles --create /etc/tmpfiles.d/lampos.conf`
  then a restart of lamp-cloud makes it work. Run that before setting the
  eyes' `--photo-dir`.

lamp-cloud and the eyes are both `lampos`; the eyes refuse a folder open
to others, and make it at 0700 if it is missing (`photo::ready`).

- **Only the lamp's own account:** the kind comes over the lamp's `fa_channel`
  like every other, which only its signed-in account may publish to
  (`crate::online`).
- **The photo** is checked: a name of 1 to 40 characters, and base64 that
  decodes to a JPEG (`FF D8`) of at most 8 MiB (`photos::asked`). Then it's
  written as a **new** file, `app-<ms>.jpg`, at mode 0600 in the folder.
  The eyes are asked `{"remember":NAME,"photo":FILE}` with a 2 s deadline,
  kept short because it's waited for on the runtime that keeps the broker's
  connection alive.
- **Never kept:** the eyes read it once and remove it. lamp-cloud removes it
  too afterwards, answered or not, so a photo the eyes never took doesn't
  stay (`photos::add`).
- **The answer** is the eyes': `added` (unconfirmed) or `remembered`.
  Otherwise their reason in the person's words: no face, more than one
  person, the face too small, not readable, known by another name, a name
  that can't be kept, no room, photos not taken. `the lamp's eyes did not
  answer` when they didn't.
- **Then at the lamp** the voice asks the person once (`LAMP_PHOTO_FACES=on`
  in the voice, `docs/eyes.md`, "A face from a photo"). No answer or no
  deletes the face, and so do 30 days unconfirmed.

Tests (`photos.rs`): the switch needs both absolute places; the name and
the JPEG checked, too long, empty, not base64, not a JPEG and too large
refused; the photo written 0600 as a new file, the eyes asked for it by
name, and gone afterwards, also when nobody answers; the eyes' reasons said
plainly.

## The faces it knows (#2192, `face.list` and `face.remove`)

Off unless `LAMP_FACES_PAGE=on`, with the eyes' request socket
(`LAMP_EYES_WHO_SOCKET`). The Lamp 2 page's list of faces, and removing one:
the privacy half of the photo faces (Mobile on #1396, mobile-P3 item 3).
lamp-cloud keeps nothing; both go to the eyes as the voice's own asks do.

- **Only the lamp's own account,** over its `fa_channel`, as `faces.add_photo`.
- **`face.list`** asks the eyes `{"list":true,"waiting":true}`, an ask only
  lamp-cloud makes, and answers each name with
  `waiting`: `false` for a face kept on its person's yes, `true` for one from
  a photo still to be confirmed at the lamp. Nothing else of the eyes'
  answer is passed on: never a print or a photo (`faces::listed`).
- **`face.remove`** takes the person off all of the lamp (the first builder
  on #2199, 13:08). It checks the name as `faces.add_photo` does, then asks
  the eyes `{"forget":NAME}`: their own forget, which takes the person's
  prints, their familiarity number, a face from a photo by that name, and
  their name in set-aside records. If set-aside records couldn't all be
  searched, the forget waits for them (#1586) and the answer says
  `finishing`. Then memory is asked, as the voice's `forget_everything`
  asks it, `{"op":"forget_all","person":ID}` for the person's id
  (`faces::person_id`, the voice's `p-` and 16 hex digits of the name's
  hash) and again for their rules' (`r-`, the same digits). `memory` says
  `forgotten` only when both were done, else `kept` with a `reason`: memory
  not reached, refused, or not given to lamp-cloud (no `LAMP_MEMORY_SOCKET`),
  so the app never says gone when it isn't. Memory's socket is mode 0600
  and its own user's, so a lamp giving lamp-cloud the socket must also give
  it the access; until then the answer is `kept`, and the app shows that
  the person's memories stay on the lamp, never "removed" alone. "Forget my
  face" at the lamp stays the face only (eyes.md).
- **A face already gone can't take its memories from the page.** After
  "forget my face" at the lamp, or a remove whose photo face couldn't be
  let go after the people's forget ran, the eyes no longer know the name:
  a remove then answers "the lamp doesn't know that face", and memory isn't
  asked. Their memories stay until "forget everything" at the lamp, or an
  ask for memory alone, which isn't built.
- **The waiting names are by convention:** only lamp-cloud sends
  `{"list":true,"waiting":true}`, but the eyes don't check who asks. It's
  the same trust their socket already has (anything running as `lampos`).
- **2 s** for the eyes' answer, as `faces.add_photo`'s, for the same reason.

Tests (`faces.rs`): the switch needs the eyes' socket's absolute path; the
list as names and `waiting` only, an empty one, and the eyes' error
refused; a remove's name checked, and each forget answer said; memory's id
the voice's, asked for the person and their rules, and `forgotten` only
when both were done; both asked over the sockets, and nobody answering
said. `tests/app_contract.rs` pins what the page reads of both (`memory`,
and `reason` with `kept`), in V1's envelope, and both refused without the
switch. `online.rs`: only the lamp's own account channel commands it
(`commanded`).

## The lamp's timers and reminders (#2195, `schedule.list`, `schedule.cancel`)

Off unless `LAMP_APP_SCHEDULE=on` in lamp-cloud's unit. lamp-schedule keeps
every timer and reminder (`/run/lampos-schedule/call.sock`, 0600, the parts'
own user, which lamp-cloud runs as; `LAMP_SCHEDULE_SOCKET` names another).
lamp-cloud asks it through `lamp_actions::schedule::Adapter`, the client the
voice's `list_reminders` and `cancel_reminder` go through: a cancel first
reads the store's identity and revision, and is sent with them as its
compare-and-set guard; every page is checked; and its failures are fixed
sentences that never carry a reminder's words (`crate::schedule`). The
reminders are the household's: anyone at the lamp can already list them or
cancel one by voice. With the switch the owner's app can too, and their
words then pass through the broker to the app, where before they left the
lamp only to the voice's model. Tests: `app_contract.rs`'s
`schedule_list_and_cancel_answer_with_the_switch_and_are_not_here_without`.

## The skills page (#1722, `skills.list`, `skills.remove`)

The Autonomous app's Lamp 2 skills page (mobile's design on #1722, the
first builder's yes at 17:21 on 19 Oct): the lamp's skills as a phone's
home screen of apps. Two of its four kinds are built: `skills.list`, read
only, and `skills.remove` ("Removing a skill" below); `catalog` and
`install` come once the update channel carries skills, and are refused as
kinds this lamp lacks until then.

**The switch.** `LAMP_SKILLS_PAGE=on`, with `LAMP_CLOUD=on`; off unless
set. Without it the kind is answered as any this lamp lacks, `not on this
lamp: skills.list` (and `not on this lamp: skills.remove`), and the app
shows no Skills row. Its envelope is V1's, as `reports.*`'s.

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

### Removing a skill (`skills.remove`)

`{"name": "desk-tutor"}`, sent by the app when the person taps Remove on
an added skill's sheet and confirms. Only the person's tap removes a skill:
the voice has no function for it and the model can't ask for it (the
cloud's kinds come only from the app, over the broker). Whether the voice
should ever remove one is a separate decision, not taken.

- **Who may be removed.** An `installed` or `made_here` skill, whether it
  loads or not. A `core` skill is refused, `dance is a core skill: it comes
  with the lamp, so it can't be removed`, and `lamp-skills` refuses it
  again under its lock. A `name` that is not a skill's, 1 to 64 lowercase
  letters, digits and hyphens (`lamp_skills::package::skill_name_ok`), is
  refused before anything is read, `name must be a skill's name: 1 to 64
  lowercase letters, digits and hyphens`, and is not said back: `..`, `/`,
  NUL, a hidden `.NAME`, an upper-case letter, a `_`, over 64 bytes, a
  number, null or no `name` at all. A name the folder does not hold is `no
  skill NAME on this lamp`. A link named as a skill is `lamp-skills`' to
  refuse (`… is not a skill's folder: left as it is`), and what it points at
  is never touched.
- **Who removes it: root's manage helper, never `lamp-cloud`** (the first
  builder, 18:33 on #1722). `lamp-cloud` talks to the network, and with
  `install` next, a part there that could write the skills folder would let
  a network message put code on the lamp. So `lamp-cloud` stays `lampos`
  under `ProtectSystem=strict` with no new writable path, and asks
  `{"remove": NAME}` over a local socket,
  `/run/lampos-skills-manage/manage.sock`, one line each way, waiting at
  most 5 s for the answer ([skills.md](skills.md), "The manage helper").
  The helper checks the request again itself, whoever connects, and runs
  `lamp-skills`' own remove (`lamp_skills::package::remove`, what
  `lamp-skills remove NAME` runs): no path of its own. Connecting to a
  socket is not a write to its file system, so the read-only `/run` of
  `lamp-cloud`'s unit lets it connect.
- **What the remove does.** Under the folder's lock (so never across an
  install), the skill's folder is renamed aside to `.removed-NAME-PID`, so
  it is gone at once and whole for the running program, then deleted.
  `lamp-skills` keeps no data per skill on `lamp-voice` (a held dim is given
  back at its reload, and #1837's off file, once merged, is one line a
  skill), so nothing else is set aside: the folder is the skill. A remove
  killed between the two steps leaves `.removed-NAME-PID`, which the running
  program passes over and the next install deletes.
- **What the running lamp does.** Nothing is restarted. `lamp-skills`
  looks at its folder every 2 s, loads the rest again, gives back what the
  removed skill held, stops its triggers and writes its tools for the voice
  again (`/run/lampos-skills/tools.json`) without its function. The voice
  sees the catalog change within its next 2 s look and opens a new session
  without that function between turns, once the conversation has ended and
  at most once in 30 s ([tools.md](tools.md); a function taken away is not
  a "came back" change, so it waits for the conversation). Until then a
  call to it gets `lamp-skills`' "No skill of mine does that just now."
- **The reply** is `skills.list`'s, the folder read again just after the
  remove: the page shows the list as the lamp now has it in one round trip,
  which is mobile's design ("Remove, confirm, then the list as the lamp now
  has it"), with nothing for the app to work out and no second kind to
  parse. It is chosen over `{"removed": NAME}` because that would be the
  app's guess of the page, not the lamp's. The running `lamp-skills` lets
  the skill go within 2 s of the reply.
- **A refusal from the helper** is its own words on one line (at most 300
  bytes), and the skill stays whole; the journal says it in full
  (`lamp-cloud: skills.remove NAME from the app refused: …`), and a remove
  done says `lamp-cloud: skills.remove from the app: removed NAME: …`, and
  the helper's own journal line (`lamp-skills: asked: …`). A helper that
  can't be reached, or answers nothing a helper would, is `the lamp's
  skills can't be changed now`, with the error in the journal; nothing is
  then removed, and `lamp-cloud` never falls back to the folder.
- **On a product lamp** the helper's two units are in `deploy/product/`
  (`lamp-skills-manage.socket` and `lamp-skills-manage@.service`) but not
  yet in `lamp-update`'s install list: they go there after their lamp
  check. Until a lamp has them, the remove is answered `the lamp's skills
  can't be changed now`.
- **How long.** A read of the folder before and after, a connection and
  the start of the helper (one process, started by systemd), two renames
  and a delete; it waits for the folder's lock while an install by hand
  runs (a package of at most 16 MB), at most 5 s. Not yet timed on a lamp.

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

### Its files, one release forward (#2052)

A build put back after a newer one ran reads what that build wrote
([updates.md](updates.md), "Every part's saved data, one version ahead").
Each case below is said once in the journal, as `lamp-cloud reports: …`.

- **The choice, for privacy: what it can't read is off, never on.** Its own
  `reports.json` with a field it doesn't know (only `share`, `at`, `by`,
  `held` are), a `by` it doesn't know or a `held` that isn't a boolean
  reads as off, whatever its `share` says, and is kept as it is, so a roll
  forward reads it again. A newer record whose `share` isn't a boolean is
  not kept in place: this build can't read it, so at start it is set aside
  and an off written, as below. That fails safe; a roll forward reads the
  off. Setup's with a field beyond `share`, `asked` and
  `at` reads as off too; `asked` above 1 is still the re-ask, as before. A
  file it can't read at all (not JSON, cut short, no boolean `share`, can't
  be opened) reads as off: its own never falls through to setup's older
  yes. At start its own is set aside whole as `reports.json.unread-<s>` and
  an off (`by` null) written in its place; nothing is sent for it, not even
  `stopped`. Setup's is root's in `/etc`, which `lamp-cloud` can't write,
  so it is left there.
- **A no over a record it can't settle:** a newer build's record or one
  spoiled while running reads as off here, but may be a yes to the build
  that wrote it, so it is not taken for a no already given. The voice's
  and the app's no are both written, delete what waits (the newer build's
  kept reports too) and queue `stopped`, sent twice as any no's.
- **The queue:** a report under `lamp-report/1` with a field it doesn't
  know is sent as is, the newer build's own report under the schema the
  backend agreed. A higher `schema`, or a `kind` it doesn't have, is kept
  where it is and **not sent**: the backend has agreed only `lamp-report/1`'s
  kinds, and one it can't take would be acknowledged by the broker and
  lost all the same; the newer build sends it when it is back. A no deletes
  those too. A file it can't read (not JSON, cut short, over 4 KiB, no
  `sent_at`) is set aside whole as `<seq>.json.unread-<s>`, never sent, and
  the rest are read. `state.json` reads a field added or gone; one it can't
  read is set aside whole as `state.json.unread-<s>`, and the next `seq`
  goes past every report still on flash. With the queue empty too, `seq`
  starts again at 1, so a seq can be used again; that is harmless, since
  the backend drops a duplicate by `(device, seq, sent_at)` and `sent_at`
  differs.
- **The exit notes:** a field added reads; a note it can't read is skipped
  and left whole, since the folder is root's and goes at the next boot.
- **The journal** names the file and why, and a JSON error by its kind,
  line and column only (#2061's rule): never a value from the file, a
  newer `by`, `schema` or `kind` included.

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

## Changed after setup (#1959)

The person changes a name, the lamp's time zone or its Wi-Fi from the app,
without a reset. Off unless `LAMP_SETTINGS_CHANGE=on`, with `LAMP_CLOUD=on`;
the Wi-Fi needs `LAMP_SETTINGS_WIFI=on` too (below).

- **The kinds:** `timezone.set {"timezone"}` (V1's own kind and shapes),
  `person.name.set {"name"}` (the person's name setup asks for; `""` removes
  it) and `device.rename {"name"}` (the lamp's name in the app).
  - **The lamp's name in the app never changes what the lamp answers to.**
    The wake word stays "Lamp"; only `host.device_name` reads it.
- **The ack at once, the result after:** the reply is `starting` as soon as
  the request is read, before anything is written. The result goes out as a
  second reply of the same kind, looked for every 50 ms on the loop texts use,
  once root's helper has answered (10 s at most, else `failure`, "the lamp's
  settings did not answer in time").
- **Who writes: root's helper, never `lamp-cloud`.** `lamp-cloud` stays
  `lampos` and `ProtectSystem=strict`, with no new writable path. It sends one
  line (`{"timezone": …}`, `{"person_name": …}` or `{"display_name": …}`) to
  `/run/lampos-setup-change/change.sock` and reads one back, on a thread of its
  own so the broker's loop never waits. The socket is systemd's
  (`lamp-setup-change.socket`: root's, group `lampos`, 0660, `Accept=yes`, 4
  connections at most), and each connection starts `lamp-setup change` as root
  (`lamp-setup-change@.service`). The helper checks every request itself
  (`setup.md`, "Changed after setup"). **The socket's group is the gate:** the
  unit always passes `--enable`, so once installed any `lampos` process can
  ask for a change; the switch only decides whether `lamp-cloud` asks.
- **Refusals** are the helper's words, never the value asked: `name must be at
  most 64 characters on one line`, `name must be 1 to 64 characters` (a rename
  to nothing), `timezone must be a zone the lamp knows, such as
  Asia/Ho_Chi_Minh`, and `the lamp's settings are not running` when the
  socket is not there.
- **Not yet:** the helper's two units are in `deploy/product/` but not in
  `lamp-update`'s install list, so no lamp has them until their lamp check.

### The Wi-Fi (`wifi.set`)

V1 has no Wi-Fi kind on the broker (a V1 lamp changes Wi-Fi only on its
hotspot's `/wifi` page), so this one is new. Off unless
`LAMP_SETTINGS_WIFI=on` **beside** `LAMP_SETTINGS_CHANGE=on` and
`LAMP_CLOUD=on`; without it, `not on this lamp: wifi.set`.

- **Asked:** `{"cmd":"data","kind":"wifi.set","data":{"ssid":"…","password":"…","request_id":"…"}}`.
  `request_id` is optional, the app's own, 1 to 64 printable characters, and
  said back. An open network has `"password": ""`.
- **Answered:** `starting` at once, then, as the same kind, `success` with
  `data: {"ssid", "request_id"}`, or `failure` with `error` and
  `data: {"request_id"}`. The errors are setup's page's words (`a Wi-Fi
  password has 8 to 63 characters`, `the Wi-Fi name is longer than 32
  bytes`, …) or what the change came to: `wrong WiFi password: the router
  rejected the key; the lamp is back on its old Wi-Fi`, `connected to the new
  WiFi, but the lamp's server could not be reached; the lamp is back on its
  old Wi-Fi`, or `…; the old Wi-Fi did not come back within 60 s, and the
  lamp keeps trying it`. Also `a Wi-Fi change is already under way` (one at a
  time) and `the lamp is not on its broker`.
- **The result may come only after the lamp is back online:** the lamp's link
  to the app is over the very network that changes. The helper is waited on
  180 s over a local socket the network change does not touch, and results
  are handed to the broker only while connected and kept until its PubAck.
  One handed and lost with the old connection goes again, first, on the next
  one, with the same `request_id`. Kept in `lamp-cloud`'s memory only: a
  `lamp-cloud` restarted mid-change loses its result (the change itself
  finishes in root's helper either way).
- **Confirmed and the way back:** root's Wi-Fi helper (`setup.md`, "The
  Wi-Fi changed after setup") takes the new network only once it is joined
  with an address and a TCP connection to the broker `lamp-cloud` is on (sent
  in its line) is made, within 60 s; else the old network comes back, 60 s
  at most, and never the hotspot. The lamp says what happened aloud, never
  the network's name or password. **The socket's group is the gate**
  (`lamp-setup-wifi.socket`, group `lampos`, 0660; the unit always passes
  `--enable`): once installed, any `lampos` process can ask for a Wi-Fi
  change, and only one whose broker the new network reaches is kept.
- **The password** goes from the app's message into the helper's line and
  the lamp's network file, and nowhere else: never a reply, an error or a
  journal line (`lamp-cloud` journals no payload). **The backend passes the
  password through to the lamp and never stores or logs it** (the backend's
  side, to be confirmed by them).

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
answers on `fd_channel`; a command sent first on five other topics (the
reply channel, another lamp's, longer, shorter, another case) done and
answered only once, from `fa_channel`; a `faces.add_photo` with the
largest photo read and answered, not dropped with the connection; a refused login backing off and said once; a dropped
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
One release forward (#2052): a choice it can't read (cut short, garbage,
empty, no boolean `share`, spoiled while running) is off, kept whole and
said once; a newer build's choice, own or setup's, reads off and is left
as it is; queued reports with a newer field (sent), a newer schema or kind
(kept, not sent, deleted by a no), cut short or garbage (set aside whole),
with the next `seq` past them all; `state.json` with a field added or gone,
or unreadable (set aside); exit notes unreadable (skipped once, left whole)
or with a newer field (taken). A no, the voice's and the app's, over a
newer build's yes or a choice spoiled while running is written, deletes
what waits and sends `stopped` twice; a value planted in `state.json`, a
queued report, an exit note and a newer `by` never reaches the journal.

The skills page (#1722): the reply's shape; a core skill never removable
and an installed one removable; a skill made here in its own group; each
permission's line and the privacy switch's; a skill that does not load, with
why; `[stop]` and the off file; the bounds; a folder that can't be read; the
switch's two gates and the refusal's exact words. `skills.remove`: a core
skill refused and left whole; a made-here skill removed, the reply the page
as it now is, and asked again, not there; an installed skill removed, and
one that does not load; an unknown name; every path trick (`..`, `/`, NUL,
a hidden name, 65 bytes, not a string) refused with the folder and the
folder beside it untouched; a link left as it is; a folder that can't be
changed leaving the skill whole; no helper, or one answering nonsense,
removing nothing; and off, `not on this lamp: skills.remove`. Each remove
goes through the manage helper's own code on a real Unix socket on a short
`/tmp` path. `tests/skills_contract.rs` pins the fields as sent, and
removes Desk tutor from a copy of `lampOS/skills/` through `Real`, the
socket and the envelope.

Changes after setup (#1959): the switch's two words; each kind refused as
one this lamp lacks without it, `device.rename` included; each kind's line for
the helper; the ack `starting` within 100 ms while a stand-in helper takes
300 ms, then its result; a refusal, a helper not running and one that never
answers; through the test broker, `starting` then `success` on `fd_channel`;
and `host.device_name` from the helper's file, the host's name without one.

`tests/app_contract.rs` holds the fields the Autonomous app's device page
reads, by name and type (Fourth on #1396): `versions.firmware`,
`network.private_ip` and `host.hostname` of `system.info`; `volume`, `raw`
and `max_volume` of `volume.get` and `volume.set`; `muted`,
`hw_switch_muted` and `available` of `mic.get`. Each is checked in the real answer, as sent.
The app reads them leniently, so a renamed or retyped field would otherwise
go blank on its page unnoticed. `hw_switch_muted` is null while nothing reads
the switch (the app takes that as not known, so not blocked by the switch), and `versions.openclaw` and `versions.openclaw_detected` stay out
of the reply: only V1's "Starting" rule waits for them.
