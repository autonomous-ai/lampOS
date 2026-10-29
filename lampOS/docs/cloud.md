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
  With `LAMP_CLOUD_TLS=on` the connection is TLS (1.2 and 1.3, the webpki
  roots, as the backend's ping already is), on the account's port, which is
  then the broker's TLS port; if TLS cannot be set up it does not connect,
  never falling back to a plain connection. Off unless set, until the
  backend's broker is shown taking it on a lamp.
  The keepalive is 60 s (V1 asks 65,535 s, which never notices a dead link).
  It takes, and tells the broker it takes, packets of up to about 11.7 MiB
  (`MOST_PACKET_BYTES`): a `faces.add_photo` with the largest photo, in
  base64, and 1 MiB more for the envelope and a sender writing `/` as
  `\/`. `rumqttc`'s own limit, 10 KiB, dropped the connection on every
  photo (Mobile on lamp-0c89, 20 Oct). The broker holds back a packet past
  it, so a photo that large gets no answer, not the "at most 8 MiB"
  refusal; the app sends at most 1 MiB.
  On every connection it subscribes to `fa_channel` at QoS 1 and says `info`
  unasked on `fd_channel`, so the backend sees it at once. A connection that
  fails, or a login the broker refuses, is tried again after a wait that
  doubles from 1 s to a minute, and the journal hears of it once, not on
  every try.
  With `LAMP_CLOUD_OFFLINE_WILL=on` (set in the product unit, #2636), each
  connection also leaves the broker a
  will: the identity with `"type": "offline"` on `fd_channel`, QoS 1, not
  retained, said 30 s after the broker finds the connection lost
  (`WILL_DELAY_S`, MQTT v5's will delay). The backend takes it as the lamp
  gone, and its next ping or `info` as back. Without it, the backend marks a
  lamp offline 5 to 8 min after its last ping or `info` (Backend3 on #2602,
  21 Oct 19:4x). The will's `time` (and its `timezone` and `local_ip`) are
  those of when the client was made, at start or on a relink after a Wi-Fi
  join; a reconnect inside the event loop keeps them. The backend reads only
  `type`, `id` and `device` of an `offline`, in the order the channel brings
  them (Backend3 on #3035). What it
  means in practice (the Reviewer on #2636, MQTT v5 3.1.2.5, 3.1.2.10,
  3.1.3.2.2):
  - **A reconnect inside the delay sends nothing.** The spec lets an ended
    session's will go at once, but the lamps' broker doesn't: on lamp-06c1
    (23 Oct 09:11:11), lamp-cloud restarted with the will on and the backend
    got only the new connection's `info`, one second later, with no
    `offline` before it. So a restart that comes back within 30 s leaves the
    record online, with no flicker.
  - **A process killed or stopped** closes its connection, and the will goes
    30 s later. lamp-cloud never sends a DISCONNECT, so a `systemctl stop`
    is an offline too, half a minute on.
  - **A lost network or a power cut** is found by the keepalive (60 s, about
    90 s in practice), then the 30 s delay, so up to about 2 minutes. That's
    still well under the 5 to 8 minutes without it.
  - **The session outlasts the delay.** The broker says a will at the
    earlier of its delay and the session's end, so the connection's 60 s
    session expiry (`SESSION_EXPIRY_S`, longer than `WILL_DELAY_S`, checked
    when lamp-cloud is built) lets the 30 s hold.
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
`face.remove`, the reminders' cancel, #2214, guard mode's `guard.on` and
`guard.off`, #2562, and `skills.make`, #2813, which asks but never puts a
skill on): whoever holds the lamp's
signed-in account speaks as its owner. **The owner's app acts for the whole
lamp, not one person at it:** it can cancel any reminder the lamp keeps,
one another person set by voice at the lamp included, as it can remove any
face the lamp knows, and any installed or made-here skill, one another
person made by voice at the lamp included (`skills.remove`); nothing at the
lamp says so.

| `cmd` / `kind` | Answer |
|---|---|
| `info` | the identity fields above |
| `system.info`, `system.version`, `system.network` | V1's shapes: the versions (`firmware`, which the app's About reads, and `build`, both the installed build; `lamp-cloud`); the default route's interface, its address, MAC, Wi-Fi name (asked of `iw` within 0.5 s and kept a minute, so a hung `iw` never holds the keepalive; one that outlives its kill, asleep in the Wi-Fi driver, is never waited for: it is reaped at a later ask, and no other `iw` starts while it is there) and gateway; the host's name, device id, uptime and zone |
| `system.info`'s `can` | the kinds in this table the lamp answers now (#2864): those with no switch always, each switched kind only while its switch is on (`chat.send`, `reports.*`, `skills.list`/`skills.remove`/`skills.off`/`skills.on`/`skills.camera` with the page (#4099), `skills.install`, `skills.make`/`skills.made`, `wifi.set`/`wifi.confirm`, `keys.*`, the settings changes, `faces.add_photo`, `face.*`, `schedule.*`, `guard.*`, `update.status`/`update.check`, `system.reboot`, `system.shutdown`), read without asking any part of the lamp. The same rule as the answers' own (`answer::on`), so the app shows only rows the lamp takes; an app that reads no `can` (or a lamp that sends none) keeps today's behaviour |
| `system.info`'s `recovery` | `true` while `lamp-setup recover` runs with `LAMP_WIFI_RECOVERY=on` (#3020; its file `/run/lampos-setup-recover/on`): a lamp whose saved network is gone opens setup's hotspot after 90 s, so the app may name that step. No field otherwise, read as before. The app keeps the last one it heard, for a lamp it can't ask |
| `system.info`'s `reset` | whether the reset button's 5 s hold takes this lamp back into setup (#2937): `hold` when `/etc/lampOS/controls/controls-pins.json` names a `reset` line (a product lamp), `none` when not (a lamp moved from V1, whose pins never carry one: its way back is the hotspot, [setup.md](setup.md)). No field when the pins can't be read; the app reads a missing field as before, the hold. The app keeps the last one it heard, for a lamp it can't ask |
| `volume.get`, `volume.set` (`{"volume": 0..100}`) | through `lamp-actions`' socket (`volume_level`, `volume_to`, `actions.md`), its only writer: `{"volume", "raw", "max_volume"}`, a set read back |
| `mic.get` | `{"muted", "hw_switch_muted", "available"}`: the voice's own mute (`voice/mute`), the privacy switch (`voice/privacy`, `null` while unknown) and whether the voice is running |
| `lamp.health` | `lamp/health`'s `level` and `message`, and `stale` |
| `led.resting.get` | the person's own light (`light/state`, `docs/ring-choreography.md`), in V1's snapshot: `mode` (`off`; `default` while it is the light the lamp starts with, warm at the lowest level; else `custom`), `color` (`[r, g, b]` for `custom`, else `null`), `default` and `effective` (`{"effect", "color"}`), and `light`, the light by its own names (`on`, `colour`, `level`) |
| `led.resting.set` (`{"mode": "default" \| "off" \| "custom", "color": [r, g, b]}`) | asked on `light/command`, as the actions program asks it, and read back from `light/state` within 1 s; the snapshot of what is shown. `off` keeps the colour and the level, and black is `off`, as V1 |
| `led.resting.preview` (`{"color": [r, g, b]}`) | asked on `light/request` at `conversation` priority (over a skill's glow, under the person's own light), on the host's boot clock, holding 10 s unless asked again, so an abandoned screen leaves nothing behind (V1's 10 s); a save gives it back first. A light asked for at the lamp ("make your light blue") ends it, and a drag that goes on stays given way until it pauses past its 10 s (the reviewer on #1464). Answered only when it fails, as V1, so a drag does not flood `fd_channel`. Refused where the ring takes no lights that lapse (`LAMP_LIGHT_REQUESTS`, on in the product) |
| `chat.send` (`{"message", "session_id", "speak"}`) | with `LAMP_MESSAGES=on`: acked with `{"run_id", "session_id"}` and put on `chat/in`; the answer goes out after it as `chat.event`s, in V1's shapes ([messages.md](messages.md)). Refused at once when nothing on the lamp takes texts, and for a photo or a file, which aren't read yet. Without the switch, V1's "not on this lamp" |
| `reports.get`, `reports.set` (`{"share": bool}`) | with `LAMP_REPORTS=on` (#1683, "Health and crash reports" below): `{"share", "since", "queued", "by", "held", "at"}`, whether the person's yes stands, since when (RFC 3339 UTC, or `null`), how many reports wait, and how the newest choice was made: `by` is `voice` (only ever a stop), `app` or `setup` (`null` for a choice kept before these were), `held` is true for a voice's stop from a line whose answer was held, and `at` is when (as `since`), so the app's row can say "Stopped by voice at 3 pm" (#1880). A set takes only a JSON bool (else `"share must be true or false"`), is kept and read back; a no deletes what waits and leaves one `stopped`. The only way to turn reports on after setup. Without the switch, V1's "not on this lamp" |
| `skills.list` | with `LAMP_SKILLS_PAGE=on` (#1722, "The skills page" below): `{"version", "skills", "left_out"}`, every skill on the lamp as `lamp-skills` loads it, each with its group (`core`, `installed`, `made_here`), whether it can be removed or turned off, and what it may use in plain words. Without the switch, V1's "not on this lamp" |
| `skills.install` (`{"url", "sha256", "name", "version"}`, all but `url` optional) | with `LAMP_SKILLS_PAGE=on`, `LAMP_SKILLS_STORE=on` and the separate Store public key ("Installing from the Skills Store" below): a Skills Store catalogue entry, passed to root's manage helper, which fetches the package over HTTPS and installs it only when it is signed with the Store's key (never a core skill or a core skill's name; `lamp-cloud` writes and fetches nothing); the reply is `skills.list`'s, read after it. Without the Store switch or a valid key, V1's "not on this lamp" |
| `skills.remove` (`{"name"}`) | with `LAMP_SKILLS_PAGE=on` ("Removing a skill" below): an installed or made-here skill taken off on the person's tap, asked of root's manage helper over its local socket (`lamp-cloud` never writes the skills folder), on a lamp installed before #2429 only after `lamp-update install` once; the reply is `skills.list`'s, read after it. A core skill, a name that is not a skill's and one not on the lamp are refused. Without the switch, V1's "not on this lamp" |
| `skills.off`, `skills.on` (`{"name"}`) | with `LAMP_SKILLS_PAGE=on` (#2815, "Turning a skill off and on" below): a skill turned off or on on the person's tap, asked of root's manage helper as a remove is, and kept in `lamp-skills`' `off` (a core skill shipped off turned on in its file and kept in `on`, #2585), so a round and a restart keep it; the reply is `skills.list`'s, read after it. A skill that is part of the lamp itself (a core grant), one that doesn't load, a name that is not a skill's and one not on the lamp are refused with why. Without the switch, V1's "not on this lamp" |
| `skills.camera` (`{"name", "allow": bool}`) | with `LAMP_SKILLS_PAGE=on` (#3504's piece C, "The owner's yes to a skill's camera" below): the owner's yes to the skill seeing the camera's picture, given (`allow` true) or taken back, past the app's warning, asked of root's manage helper as off and on are and kept in `lamp-skills`' `camera`; the reply is `skills.list`'s, read after it, with `camera: {name, allowed}`, what was kept for that name, on the lamp or not (#4099). A name that is not a skill's is refused, and `allow` that isn't a JSON bool is `allow must be true or false`. Without the switch, V1's "not on this lamp" |
| `guard.on`, `guard.off` | with `LAMP_GUARD_FROM_APP=on` (#2562, [guard-mode.md](guard-mode.md)): guard mode turned on or off from the owner's app, handed to `lamp-skills`' call socket (`/run/lampos-skills/call.sock`) as `{"op":"guard","turn":"on"\|"off","deadline_us":...}`, which takes it only with the same switch in its own unit and only from `lamp-cloud`'s unit (by `SO_PEERCRED` and cgroup); `lamp-cloud` turns nothing itself. `deadline_us` is the moment `lamp-cloud` sent the op plus the 2 s it waits (`watch::FROM_APP_WAIT`, shared), on the host's shared monotonic clock (`lamp_ipc::monotonic_us`): that socket answers one connection at a time, so an op read at or past it, behind a voice call, is let go ("guard.on let go: it waited N ms behind another call, past the app's 2 s") and nothing is turned, as the app was told; one with no `deadline_us`, from an older `lamp-cloud`, is let go too. `{"guard": "on"}` or `{"guard": "off"}`, or `lamp-skills`' reason (the privacy switch closed, its mark not showable, paused under the person's light, not installed), or "guard mode can't be reached on the lamp now". The faint mark still shows, and nothing is counted until the ring confirms it. Without the switch, V1's "not on this lamp" |
| `skills.make`, `skills.made` | with `LAMP_APP_MAKE_SKILL=on` and the skills page (`LAMP_SKILLS_PAGE=on`), and the same switch in `lamp-voice`'s unit (#2813, "A skill typed in the app" below): `skills.make` (`{"words", "request_id"}`, the words at most 180 bytes) hands the words to `lamp-voice`'s app-make socket (`/run/lampos-app-make/make.sock`), which makes the skill as the voice's own; `{"making": true, "request_id"}`, or why not in plain words. `skills.made` (`{"request_id"}`, or `{}` for the newest) tells what became of the app's own ask: `state` (`making`, `kept`, `asked`, `on`, `left_off`, `not_kept` with `why`), and once kept its `name`, `description`, `version`, `sha256` and `may_use` (the skills page's rows). Nothing from the app puts a skill on: only a spoken yes at the lamp. Without the switches, V1's "not on this lamp" |
| `faces.add_photo` (`{"name", "photo"}`, the photo a JPEG in base64, at most 8 MiB) | with `LAMP_PHOTO_FACES=on` (#1968, "A face from the app's photo" below): the photo is handed to the eyes, which keep its face **unconfirmed** until its person says yes at the lamp. `{"added": NAME, "unconfirmed": true}`, or `{"remembered": NAME}` for a face already theirs; else the eyes' reason in plain words. Without the switch, V1's "not on this lamp" |
| `face.list` | with `LAMP_FACES_PAGE=on` (#2192, "The faces it knows" below): `{"faces": [{"name": NAME, "waiting": BOOL}, …]}`, the names the eyes keep and, `waiting`, a face from a photo still to be confirmed at the lamp. Names only. Without the switch, V1's "not on this lamp" |
| `face.remove` (`{"name", "entirely"}`) | with `LAMP_FACES_PAGE=on`: the person off all of the lamp, the eyes' own forget and then everything memory keeps for them and their rules (`entirely` true or left out), or the face only (`"entirely": false`, `"memory": "not asked"`). `{"removed": NAME, "memory": "forgotten"}`, or `"memory": "kept"` with its `reason`; for a face already gone (a remove `entirely` after a face-only one, or a retry after `kept`), `"face": "already gone"` with `"memory": "by name only"` (memory was asked by the name's id only), or `kept`; with `"finishing": true` while set-aside records it waits for aren't searched yet. `the lamp doesn't know that face` for a name it doesn't keep. Without the switch, V1's "not on this lamp" |
| `schedule.list` (`{"after_id"?: n}`), `schedule.cancel` (`{"id": n}`) | with `LAMP_APP_SCHEDULE=on` (#2195, "The lamp's timers and reminders" below): the list is a page of at most eight, `{"items": [{"id", "kind": "timer"\|"reminder", "text", "state", "local", "due_unix_ms", "repeat", "snoozed", "late"}], "next_after_id", "complete"}`, each as lamp-schedule keeps it; a cancel is `{"cancelled": true, "id": n}`, the timer or the whole reminder series. A refusal is the reminder client's fixed sentence: no longer active, changed meanwhile (list again), or not confirmed (it may have happened). A bad `after_id` or an `id` that isn't a whole number above 0 is refused before lamp-schedule is asked. Without the switch, V1's "not on this lamp" |
| `update.status` | with `LAMP_APP_UPDATE=on` (#2194, "The lamp's software update" below): `{"running": {"sequence", "commit"} \| null, "now": "idle"\|"checking"\|"fetching"\|"switching"\|"watching"\|"restoring"\|"unknown", "last": {"outcome", "sequence", "at_unix_ms", "reason"?, "started_at_unix_ms"?} \| null, "stale"}`, from lamp-update's kept `update/status`; `unknown` and nulls before lamp-update has said anything. Without the switch, V1's "not on this lamp" |
| `update.check` (`{}`) | with `LAMP_APP_UPDATE=on` (#4153, "Check for update" below): asks `lamp-update serve` for its next round now. `update.status`'s object with `"check": "started"`, or `failure` with `a round is running` (`now` is not `idle` or `unknown`), `asked less than a minute ago`, or `lamp-update is not listening for a check`. Without the switch, V1's "not on this lamp" |
| `mic.set` | refused: "on this lamp the microphones are muted and unmuted with the button or the switch". The person's own switch and button are the only way |
| `device.soft_reset` | refused until the owner says otherwise: a destructive act from anywhere is the owner's call |
| `keys.set` (`{"service": "deepseek" \| "deepgram", "key", "base_url"?, "request_id"?}`), `keys.remove` (`{"service", "request_id"?}`), `keys.get` (`{"request_id"?}`) | with `LAMP_OWN_KEYS=on` beside `LAMP_SETTINGS_CHANGE=on` (#2094, [own-keys.md](own-keys.md), the contract on #2094 at 11:13 and its additions after): `starting` at once (with `data.request_id`), then `success` with `{"service", "uses": "own", "code": "works", "answer"}` (the answer names where the person's data goes), `{"service", "uses": "lamp"}` or `{"deepseek": "own" \| "lamp", "deepgram": …, "since": {"deepseek", "deepgram"}, "deepseek_host"}`, or `failure` with `error` and `data.code`: `refused`, `unreached`, `not_kept`, `bad_field`, `busy`, `not_allowed` or `unavailable`. `request_id` is said back on every reply. The key is in no reply. Without the switch, V1's "not on this lamp" at once |
| `system.reboot` | with `LAMP_APP_RESTART=on` (#2193, "A restart from the app" below): asked of setup's change helper, never done by `lamp-cloud`; `success` with `{"restarting": true}` once the helper has taken it, before the lamp goes down, or `failure` with the helper's reason ("an update is being installed: try again once it is done"). The lamp's parts restart (`lampos.target`), not the machine. `data` is not read. Without the switch, V1's "not on this lamp" |
| `system.shutdown` | with `LAMP_SHUTDOWN=on` (#3697, "The lamp turned off from the app" below): asked of setup's change helper, never done by `lamp-cloud`; `success` with `{"shutting_down": true}` once the helper has taken it, before the lamp goes down, or `failure` with the helper's reason ("an update is being installed: try again once it is done"). `data` is not read. Once taken, asked again within 60 s (`lamp::YES_KEPT`) it is answered the same `success` and the helper is not asked again; after that the helper is asked again. Without the switch, V1's "not on this lamp" |
| `timezone.set` (`{"timezone": "Asia/Ho_Chi_Minh"}`), `person.name.set` (`{"name"}`, `""` removes it), `device.rename` (`{"name"}`, 1 to 64 characters) | with `LAMP_SETTINGS_CHANGE=on` (#1959, "Changed after setup" below): V1's `starting` at once, then `success` with what was kept (`{"timezone"}` or `{"name"}`), or `failure` with why. Without the switch, V1's "not on this lamp" |
| `owner.place` (`{"lat", "lon", "name", "set_at"}`, `set_at` ISO 8601 UTC), `owner.place.clear` (`{}`) | with `LAMP_OWNER_PLACE=on` beside `LAMP_SETTINGS_CHANGE=on` (#4333, [owner-place.md](owner-place.md), "The owner's place" below): rounded here to about a kilometre and handed to root's helper as the names are; `starting` at once, then `success` with what was kept (`{"name", "lat", "lon", "set_at"}`) or `{"cleared": true}` (also when none was kept), or `failure` with why; a request that doesn't fit fails at once. `name` is an area name (sub-locality and locality from the phone's geocoder), never a street address; `lat`/`lon` is where the lamp stands, set by the person at setup or with Update now, never resent as the phone moves, rounded by the app to 2 decimals. Listed in `can` only with the switch. Without it, `owner.place` gets V1's "not on this lamp", and `owner.place.clear` is still taken, unlisted (#4356) |
| anything else (`add_channel`, the other `skills.*`, the other `face.*`, `tts.*`, the other `led.*`, …) | V1's `failure`, `"not on this lamp: <kind>"` |

### News for the phone, unasked (`messages/out`)

With `LAMP_MESSAGES=on`, a skill's short line for the person's phone, as
guard mode's "Someone was at your desk at 14:31, for about 2 minutes.", goes
up on `fd_channel` with nothing asked first: V1's `chat.event` with no run,
its `data.event` a `task_news` event with its own `id` and `time` and the
skill's text whole, as the voice's unasked task news (`crate::unasked`,
[messages.md](messages.md), "Unasked texts"). At most 6 in any hour, in
order, one heard again dropped; at most 4 wait while the broker is away and
go on reconnect, each kept until the broker's ack. A named text (guard mode
with `LAMP_GUARD_NAMES=on`, `MessageOut::unnamed` and `named`, #2562) goes
named only when it is sent as it is taken and the eyes' `tellable`, read
just then beside `LAMP_CLOUD_KNOWN_FILE` (`lamp_msgs::known::still_tellable`),
still has a yes from each one it names; else, and held or owed, it goes as
its unnamed words, so a late, held or retried text never names anyone who
took their yes back. It is also sent unnamed when a send of lamp-cloud's own
is still not acknowledged, since the MQTT client may hold it behind that one
(a broker's receive maximum) past the eyes' word. The journal says each
once: "a named text sent as unnamed: the eyes did not confirm each yes as it
went", "a named text sent unnamed: an earlier send was still
unacknowledged", "a named text held as unnamed: the broker was away". The journal says one line
a piece, the skill and the count, never the text.

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
  `tests/it/setup.rs` reads the product's own setup settings and setup's own
  writer, so the unit's path and the fields can't drift from setup's. The
  voice's key stays in `/etc/lampOS/voice/credential`.
- **The unit** (`deploy/product/lamp-cloud.service`) runs it as `lampos` and
  hands it those two files and `lamp-update`'s `state.json` (for the build)
  through `LoadCredential=`, so no file is widened. It starts only once all
  three are there and setup is done. It sets
  `LAMP_CLOUD_KNOWN_FILE=/run/lampos-eyes/known` (#2562): beside it, the
  eyes' `tellable`, read as guard mode's named text goes. Read only, as
  `lampos`, under `ProtectSystem=strict` (`/run/lampos-eyes` is 0755 and
  `tellable` the eyes' 0600 file of the same user); missing or unreadable,
  the text goes unnamed.
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
  To keep it off on a lamp, mask it (`systemctl mask lamp-cloud`). A kept
  drop-in or edited `store.env` that sets `LAMP_CLOUD` to anything but `on`
  leaves it saying `off: LAMP_CLOUD is not on`; install and `check-units`
  name such a file ([install.md](install.md), step 5).
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

## A restart from the app (#2193, `system.reboot`)

Off unless `LAMP_APP_RESTART=on`. `lamp-cloud` restarts nothing itself: it
sends `{"restart":"lamp"}` to setup's change helper on
`/run/lampos-setup-change/change.sock` (`lamp-setup change`, root's, on every
lamp; [setup.md](setup.md), "Changed after setup"), and waits at most 2 s
for its one line back, as for the eyes' answer to a photo. A product lamp
has no keeper, so the keeper's own socket (`lamp-keeper restart-asked`,
[keeper.md](keeper.md)) is not asked; both helpers share one copy of the
request, the answer and the update lock (`lamp_keeper::restart`). The
helper refuses while an update installs, and otherwise answers first and,
holding the update's lock, restarts `lampos.target` once the voice has
rested between turns (5 s; at most 45 s, then all the same), so the app's
`success` leaves before `lamp-cloud` is stopped with the rest of the lamp.
A helper not there is "the lamp's settings are not running"; one that does
not answer within 2 s, "the lamp's settings did not answer in time".
Pinned in `tests/app_contract.rs`, with a stand-in helper that answers with
the helper's own code against a real update lock.

Whether the app's "restart" should reboot the machine, as V1's does, is
the first builder's answer on #2193; until then it restarts the lamp's
parts.

## The lamp turned off from the app (#3697, `system.shutdown`)

Off unless `LAMP_SHUTDOWN=on` (the switch of every way a person asks to
turn the lamp off, `switches.md`). The app's off button (Mobile, #3698)
sends it. `lamp-cloud` turns nothing off itself (it is not root): it sends
`{"shutdown":"lamp"}` to setup's change helper, as the restart above, with
the same 2 s bound. The helper takes lamp-update's lock without waiting:
held by an update, or not checkable, it refuses with why. Otherwise it lets
the lock go, answers `{"done":{"shutting_down":true}}` and starts
`lamp-setup shutdown`, which says it is going to sleep, rests the lamp,
stops its parts, lets the body go, lights the ring and powers off
([shutdown.md](shutdown.md)). That runner takes the update's lock again
itself, waiting up to 5 s (`shutdown::LOCK_WAIT`) for the helper, which
lets go of it as it exits. An update that holds it past that is not cut:
the runner refuses and its journal says so (`lamp-setup: shutdown:
refused: …`), and the app has already heard `success`. `lamp-cloud` keeps
the helper's yes for 60 s (`lamp::YES_KEPT`): an app that asks again before
the lamp is down gets the same `success`, and the helper is asked once. A
lamp still on after that wasn't turned off, so the helper is asked again
and the app hears why. The runner's refusal never reaches `lamp-cloud`, so
a tap within those 60 s still gets the kept `success`; 60 s is the bound. A refusal is not kept, so the app may ask again once
the update is done.

`system.info`'s `can` lists `system.reboot` and `system.shutdown` only while
their switches are on in `lamp-cloud`'s unit. The helper is not probed: it
ships in the same build as `lamp-cloud`, so the build that has the switch
has the helper that answers it.

The request and its replies, for the app (V1's `data` envelope):

- **Asked:** `{"cmd":"data","kind":"system.shutdown","data":{}}` (or
  `"kind":"system.reboot"`). `data` is not read.
- **Taken:** `{"cmd":"data","kind":"system.shutdown","status":"success","data":{"shutting_down":true}}`
  (`{"restarting":true}` for the restart), said before the lamp goes down.
  The lamp then drops off the broker; for the shutdown it does not come
  back until it is powered on again.
- **Refused:** `{"cmd":"data","kind":"system.shutdown","status":"failure","error":WHY}`,
  where WHY is one of: `an update is being installed: try again once it is
  done`; `the update's state could not be checked: …`; `the lamp's settings
  are not running` (the helper is not there); `the lamp's settings did not
  answer in time`.
- **Off:** `"status":"failure","error":"not on this lamp: system.shutdown"`,
  and the kind is not in `can`.
- **Asked again** after a `success`: the same `success`, nothing asked again.

Pinned in `tests/app_contract.rs` with the stand-in helper.

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
  `finishing`. Then, unless the app asked for the face only, memory is
  asked `{"op":"forget_person","person":ID}` for the person's id
  (`faces::person_id`, the voice's `p-` and 16 hex digits of the name's
  hash), which forgets them and their rules (`r-`, the same digits) in one
  request (memory.md, "Forgetting a person"). With the stable memory id
  (#2176), the id the eyes say too, and the name's own only when the eyes
  say it is nobody else's (`name_id_free`, #2330; memory.md): one
  `forget_person` for each. Memory then tells the lamp's other parts on
  `memory/forgot`, as for "forget everything" by voice. `memory` says
  `forgotten` only when each was done, else `kept` with a `reason`: memory
  not reached, refused, or not given to lamp-cloud (no
  `LAMP_MEMORY_FORGET_SOCKET`), so the app never says gone when it isn't.
  "Forget my face" at the lamp stays the face only (eyes.md), but with
  `LAMP_MEMORY_FORGET_ALL=on`, which takes the known person's memory
  too (#2850).
- **`entirely`** (agreed with the mobile app): `true`, or left out as
  before, is the eyes and then memory; `false` is the eyes' forget only,
  and the answer carries `"memory": "not asked"`; any other value is
  refused before the eyes are asked. A remove `entirely` of a face the eyes
  no longer know (after a face-only remove, or a retry after `kept`) still
  asks memory for the name's id while the eyes say nobody else holds it,
  and answers `"face": "already gone"` with `"memory": "by name only"` (a
  person renamed under `LAMP_MEMORY_STABLE_ID` keeps theirs under their
  first name's id, which nothing could look up then) or `kept` (Third on
  #2563); a
  face-only remove of an unknown face is refused as before. `{"entirely": true}` with no name is
  refused by the name's check, ending `; entirely is taken`: that refusal
  is the app's probe for its two choices (Mobile on #2563). A lamp of
  before `entirely` refuses it plainly (and would forget memory too on a
  face-only remove), and a lamp without `face.remove` answers `not on this
  lamp`.
- **`LAMP_MEMORY_FORGET_SOCKET`** is memory's narrow socket
  (`lamp-memory --forget-socket … --forget-group …`): mode 0660 for one
  group, taking `forget_person` only, so lamp-cloud can forget a person and
  read nothing; lamp-cloud's user must be in that group. Memory's own
  socket (`LAMP_MEMORY_SOCKET`, 0600) is no longer asked by lamp-cloud,
  with no fallback to it. The product units don't set either yet: until a
  lamp sets both, the answer is `kept`, and the app shows that the
  person's memories stay on the lamp, never "removed" alone.
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
said. `tests/it/app_contract.rs` pins what the page reads of both (`memory`,
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

## The lamp's software update (#2194, `update.status`, `update.check`)

Off unless `LAMP_APP_UPDATE=on` in lamp-cloud's unit. lamp-update says on
the bus, as a kept value, `update/status` (`lamp_msgs::UpdateStatus`,
`lamp_update::status`): the build its records say the lamp runs, what its
round is doing now (`checking` the channel, `fetching` and checking the new
build's files beside the old, `switching` at a quiet moment, `watching` the
parts for a minute after, `restoring` while a rollback puts the previous
build back and watches it, `idle` between rounds), and how the last round
ended, by the outcome's name (`up-to-date`, `switched`, `refused`,
`rolled-back`, `failed`, ...) and the build it was about. It carries no reason
text, which may name paths. A round that refused, failed, rolled back, was
unreachable or found no quiet moment also says where it stopped by a fixed
name, `reason`: the phase it was in (`checking`, `fetching`, `switching`,
`watching`, `restoring`), or `models` for a build whose models wait for the
models switch. A round's end says when it began, `started_at_unix_ms`. A
round that ends before its first phase, such as one that finds the update's
lock taken, has neither. A new lamp-update's confirmation of its own
switch is an end too, `self-confirmed`, after the `self-switched` that
began it. Both are absent from a value read back from the
records at start, and from an older lamp-update. lamp-cloud answers from that value and never
reads lamp-update's records, which stay root's alone (0700). Before
lamp-update has said anything, `now` is `unknown`. Only a round that acts
(`--apply on`) says it. The last result is read back from lamp-update's own
log when it starts, so a restart, a reboot or its own switch never says it
as none. The value is said again every minute and lasts three: `stale` is
true when lamp-update stopped saying it (it died mid-round), and its `now` is
then not to be believed. Tests: `app_contract.rs`'s
`the_software_update_is_answered_from_lamp_updates_status`, and
`lamp-update`'s `status` tests and `a_round_with_nothing_to_change_only_checks`.

### Check for update (#4153, `update.check`)

With the same switch, the app's Check for update asks `lamp-update serve`
for its next round now instead of after its wait (`--every-s`, 15 minutes).
lamp-cloud only asks: it says one `update/check` (`lamp_msgs::UpdateCheck`,
empty, a stream nothing keeps), and `serve` reads it between rounds, looking
once a second. The round is the same round: the same lock, a quiet moment
before any switch (nobody heard or answered for 45 s, or the lamp asleep or
muted), and `--apply` as set, so a tap never installs in the middle of a
conversation. The reply is `update.status`'s object with
`"check": "started"`; the app then follows the round on `update.status`.

It is refused, in V1's failure shape with the reason the app shows:

- `a round is running`: `update/status`'s `now` is not `idle` or
  `unknown`. A stale value counts as it reads.
- `asked less than a minute ago`: lamp-cloud took one within the last
  60 s (`UpdateCheck::AGAIN_AFTER`). A refused ask does not count.
- `lamp-update is not listening for a check`: nothing running reads
  `update/check`, so nothing would happen. That is an older lamp-update (one
  build apart, it never subscribes), a dry `serve` (without `--apply on`,
  which also says no `update/status`), or none running.

On `serve`'s side too a check that came while a round ran is dropped,
never kept for after it, and at most one a minute starts a round.
`started` means a running lamp-update has the check in its socket, not
that a round has begun: a lamp-update stopped between that and its next
look starts none, and the app sees it on `update.status` (`now` stays
`idle`, then reads `stale`). Tests: `answer.rs`'s
`the_app_asks_for_a_round_now_between_rounds_and_once_a_minute`,
`update.rs`'s tests, `app_contract.rs`'s
`the_software_update_is_answered_from_lamp_updates_status`, and
`lamp-update`'s `app_check` tests.

## A skill typed in the app (#2813, `skills.make`, `skills.made`)

Off unless `LAMP_APP_MAKE_SKILL=on` in `lamp-cloud`'s unit, with the skills
page (`LAMP_SKILLS_PAGE=on`), and the same switch, `LAMP_MAKE_SKILL=on` and
the tasks part in `lamp-voice`'s (`crate::make_skill`; the voice's side is
in [skills.md](skills.md), "Asked for in the app"). The first builder's
contract on #2602 (22 Oct 10:3x); the app's screen is Fourteen's (#2814).

- **`skills.make`** (`{"words", "request_id"}`): the words, trimmed, 1 to
  180 bytes, and a request id, are sent to `lamp-voice`'s app-make socket
  (`/run/lampos-app-make/make.sock`, in `ReadWritePaths` as
  `-/run/lampos-app-make`) as `{"make": {"words", "request_id"}}`. The voice
  answers at once and hands the words over after, on its own thread, as
  its own brief: typing them in the app is the person's ask, so there is no
  "shall I start it?". The app is told `{"making": true, "request_id"}`, or
  why not in plain words: one already being made, the same request id
  again, no request id, the words too long, or "making a skill can't be
  reached on the lamp now" (no answer within 2 s). The journal never has
  the words.
- **`skills.made`** (`{"request_id"}`, or `{}` for the newest): the app's
  own ask only, never a skill made by voice. `state` is `making`, `kept`
  (not yet asked about aloud), `asked` (the offer a spoken yes would put on
  now: the newest, within its 120 s), `on`, `left_off` (asked, and no longer
  that offer: its 120 s passed, or another made skill was asked about
  since; a spoken no shows as `asked` until then) or `not_kept`, with
  `why` in plain words, never the agent's text. Once kept: `name`,
  `description`, `version`, `sha256` (what the lamp asks about) and
  `may_use`, the skills page's rows (`skills::may_use`). With no ask,
  `{"state": "none"}`.
- **Put on only by a spoken yes at the lamp** (the lead's ruling on #1711,
  20 Oct 08:37): the lamp tells the kept skill aloud at a good moment with
  someone there, and only a spoken yes within 120 s of that ask puts it on,
  checked again by the manage helper (#2874). Nothing the app sends puts a
  skill on, and a typed yes doesn't either.

- **The played check** (`MAKE_FROM=app tools/played/make-skill.sh LABEL
  FIXTURE both`, with `CLOUD=` an ARM64 `lamp-cloud` from the same build):
  `lamp-cloud make-skill make WORDS REQUEST_ID` and `made REQUEST_ID` ask
  the same `make_skill::make` and `made` the kinds call, on the same
  socket, and print `making` or `refused: WHY`, and `STATE SHA256`, one
  line, for the script to compare. No switch: it is run by hand on a lamp
  under test, as `lampos`, `lamp-cloud`'s own user, so the socket's group
  is what lets it in (`sudo -n -u lampos`).

Tests: `make_skill::tests` (the line to the voice and its answer, refusals
before asking, no voice; `may_use` from the voice's `uses`; the played
command's plain words), the kinds' gates in `answer::tests`,
`switch::the_unit_may_reach_lamp_voices_app_make_socket`, and
`tools/played/make-skill-test.sh`'s app arms (kept with the same SHA-256,
then on or still off; another SHA-256, or on in the no arm, fails the check
and the run; no `CLOUD` changes nothing; no Python on the lamp).

## The skills page (#1722, `skills.list`, `skills.remove`, `skills.off`, `skills.on`, `skills.camera`, `skills.install`)

The Autonomous app's Lamp 2 skills page (mobile's design on #1722, the
first builder's yes at 17:21 on 19 Oct): the lamp's skills as a phone's
home screen of apps. All its kinds but `catalog` are built: `skills.list`, read
only, `skills.remove` ("Removing a skill" below), `skills.off` and
`skills.on` ("Turning a skill off and on" below), `skills.camera` ("The
owner's yes to a skill's camera" below) and, behind a switch of
its own, `skills.install` ("Installing from the Skills Store" below);
`catalog` comes with the Store's server, and is refused as a kind this lamp
lacks until then.

**The switch.** `LAMP_SKILLS_PAGE=on`, with `LAMP_CLOUD=on`; off unless
set. Without it the kind is answered as any this lamp lacks, `not on this
lamp: skills.list` (and `not on this lamp: skills.remove`), and the app
shows no Skills row. Its envelope is V1's, as `reports.*`'s.
`skills.install` needs `LAMP_SKILLS_STORE=on` and a valid Store public key
as well; without either, `not on this lamp: skills.install`. The manage
helper checks both in its own process and refuses with the same words
([skills.md](skills.md), "The manage helper"). `LAMP_ADDONS` is read the same way: without
`LAMP_ADDONS=on` in `lamp-cloud`'s unit, an add-on (`kind = "addon"`,
[addons.md](addons.md)) is listed with `loads: false` and `why: "add-ons
are off on this lamp"`, as `lamp-skills` does not run it then.

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
   "group": "core", "version": "1.0.0", "sha256": null,
   "can_remove": false, "can_turn_off": true, "off": false,
   "camera_allowed": null, "loads": true, "why": null,
   "may_use": [
     {"name": "speak", "says": "speaks, or plays a short sound", "privacy": null},
     {"name": "light", "says": "changes the light", "privacy": null},
     {"name": "body", "says": "moves the lamp", "privacy": null},
     {"name": "hearing", "says": "hears what is said to it", "privacy": "the privacy switch stops it"}]},
  {"name": "desk-tutor", "title": "Desk tutor", "description": "…",
   "group": "installed", "version": "1.0.0",
   "sha256": "…",
   "can_remove": true, "can_turn_off": true, "off": true,
   "camera_allowed": false, "loads": true, "why": null,
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
- **`sha256`** (#2801): for a skill put on from a package (from the Skills
  Store, by hand or made by voice), the SHA-256 of that package's bytes,
  64 lowercase hex, the one the Store's catalogue names beside the same
  `version`, so the app can tell the lamp runs the package the Store lists.
  Kept in the skill's folder at install ([skills.md](skills.md), "What an
  installed package keeps"); a newer package's replaces it. Null for a
  core skill, one put on from a folder, one installed before the lamp kept
  it, and one whose version was changed by hand since. It names the package
  installed, not the folder as it is now: a hand edit that keeps the
  version still lists the package's `sha256`.
- **`can_remove`**: false for a core skill, true for the others.
- **`can_turn_off`**: the skill loads and is not part of the lamp itself
  (#2815): true for every one but those granted a core operation (`core =
  [...]` in `[permissions]`: Night sleep, Onboarding, Prepare to unplug,
  `lamp_skills::package::cannot_turn_off`), which `skills.off` refuses.
  The app's switch, on a core row and an added one alike.
- **`off`**: when it can be turned off, true when it is off: in the off
  file `lamp-skills --state-dir` keeps (#1886, #2815,
  `/var/lib/lampos-skills-state/off`, a name a line), or shipped off
  (`enabled = false`) and not turned on; false when it is not (it may be
  snoozed until the morning then, which `lamp-skills` keeps only in
  memory). Null when it can't be turned off, or when the off file can't
  be read.
- **`camera_allowed`** (#3504's piece C): when it asks the camera's
  picture (its `may_use` has `camera`), true when it may see it: a core
  skill, or one the owner said yes to (`skills.camera`, kept in
  `/var/lib/lampos-skills-state/camera`, a name a line, beside the off
  file); false when not, or when that file can't be read, as
  `lamp-skills` then gives no picture. Null when it doesn't ask the
  picture.
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
  | `output_limits` | caps the light and the volume at set hours | — | `output_limits` |
  | `messages` | sends a short message to your phone | — | `messages` |
  | `propose_say` | suggests its own words for the lamp to say, quoted; the lamp may say no | — | an add-on's `propose` naming `say`, or `say = "quoted"` |
  | `propose_light` | suggests a change of light; the lamp may say no | — | an add-on's `propose` naming `light` |
  | `propose_move` | suggests a small move; the lamp may say no | — | an add-on's `propose` naming `move` |
  | `propose_mood` | suggests a change of mood; the lamp may say no | — | an add-on's `propose` naming `mood` |
  | `camera` | uses the camera when it runs | the privacy switch stops it | `look` |
  | `faces` | notices faces in front of it, with the camera | the privacy switch stops it | `humans/faces/tracked` |
  | `known_faces` | knows whether a face is one it has met, never whose | the privacy switch stops it | `known_faces` |
  | `presence` | notices whether someone is at the desk, with the camera | the privacy switch stops it | `room/presence`, `room/activity` |
  | `hearing` | hears what is said to it | the privacy switch stops it | `voice/heard`, `name/heard` |
  | `room_sound` | hears how loud the room is | the privacy switch stops it | `ears/level`, `ears/sudden` |
  | `touch` | feels a touch on the lamp | — | `controls/touch`, `controls/gesture`, `controls/pet` |
  | `air` | reads the air sensor | — | `air/state` |
  | `reminders` | knows when a reminder is due | — | `schedule/due` |
  | `tasks` | knows how the work you handed over is going | — | `tasks/status` |
  | `memory` | reads what you asked it to remember | — | `memory/rules` |
  | `lamp_state` | knows what the lamp itself is doing | — | any other topic (`voice/state`, `lamp/health`, …) |
  | `actions` | uses the lamp's own controls | — | `actions` not empty |

`tests/it/skills_contract.rs` pins the reply's fields by name and type, in
V1's envelope through JSON, from `Real` over `lampOS/skills/`, and the
refusal's exact words without the switch.

### Removing a skill (`skills.remove`)

`{"name": "desk-tutor"}`, sent by the app when the person taps Remove on
an added skill's sheet and confirms. Only the person's tap removes a skill:
the voice has no function for it and the model can't ask for it (the
cloud's kinds come only from the app, over the broker). Whether the voice
should ever remove one is a separate decision, not taken.

- **Its camera yes goes with it.** The helper takes the name out of
  `lamp-skills`' `camera` first, under the same lock, so a skill put on
  again by that name asks again (`skills.camera` below).
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
  back at its reload, and #1837's off file, `OFF_FILE` in `--state-dir`,
  is one line a skill), so nothing else is set aside: the folder is the
  skill. A removed skill's line in the off file stays, so a skill installed
  again under the same name starts off, as it was left. A remove
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
  (`lamp-cloud: skills.remove from the app refused: …`), and a remove
  done says `lamp-cloud: skills.remove from the app: removed NAME: …`, and
  the helper's own journal line (`lamp-skills: asked: …`). A helper that
  can't be reached, or answers nothing a helper would, is `the lamp's
  skills can't be changed now`, with the error in the journal; nothing is
  then removed, and `lamp-cloud` never falls back to the folder. The helper
  waits at most 4 s for the folder's lock
  (`lamp_skills::package::ASKED_LOCK_WAIT`, under `lamp-cloud`'s 5 s), and
  past that refuses in the same words, `the lamp's skills can't be changed
  now`, having done nothing, so a remove is never finished after
  `lamp-cloud` stopped waiting for it.
- **Renamed aside but not deleted** (the second step failed) is a remove
  done: the skill is already gone for the running program, and the next
  install deletes `.removed-NAME-PID`. The app gets the list as usual; where
  the folder was set aside, and why it was not deleted, go to the helper's
  journal alone, never into the reply.
- **On a product lamp** the helper's two units
  (`lamp-skills-manage.socket` and `lamp-skills-manage@.service`) are in
  `lamp-update`'s install list (`install::PRODUCT`, #1685): the socket is
  enabled, the template never. A round never adds a unit, so a lamp
  installed before #2429 has it only after `sudo lamp-update install …`
  once; until then the remove is answered `the lamp's skills can't be
  changed now`.
- **How long.** A read of the folder before and after, a connection and
  the start of the helper (one process, started by systemd), two renames
  and a delete; it waits for the folder's lock while an install by hand
  runs (a package of at most 16 MB), at most 4 s, then refuses. Not yet
  timed on a lamp.
- **Known limit: a core skill whose `skill.toml` doesn't parse** counts as
  installable (`lamp-skills`' `kind_of` reads the kind from the file), so
  it can be removed, by hand and from the app. It does not load anyway,
  and the next `lamp-update install` writes it back. Taking "core" from
  `lamp-update`'s list of shipped names instead would close it; not done
  here.

### Turning a skill off and on (`skills.off`, `skills.on`)

`{"name": "dance"}`, sent by the app when the person flips a skill's
switch on the My skills page (#2815, for #2816). The person's own choice,
kept as a `[stop]`'s off is, so an update round and a restart keep it.

- **The same path as a remove.** `lamp-cloud` checks the name (`name must
  be a skill's name: …`, not said back) and asks root's manage helper
  `{"off": NAME}` or `{"on": NAME}` over its socket, waiting at most 5 s;
  it writes nothing itself ([skills.md](skills.md), "The manage helper").
  The reply is `skills.list`'s, read after it, the skill's `off` now true
  or false.
- **What is kept.** Off puts the name in `lamp-skills`' `off`
  (`/var/lib/lampos-skills-state/off`, a name a line), which a round never
  touches; the running `lamp-skills` reads it again within 2 s and stops
  what the skill does by itself, as a `[stop]`'s off does (asked for by
  name in a talk, its function still answers). On takes it out. A core
  skill the build ships off is turned on in its `skill.toml` and kept in
  `on` beside `off`, so a round keeps it on (#2585); turned off again its
  file is the build's again, it leaves `on` and it is in `off`.
- **Refused, with why.** A skill that is part of the lamp itself, granted
  a core operation, `night-sleep is part of how the lamp itself works, so
  it can't be turned off` (its row says `can_turn_off: false`); one that
  doesn't load (`NAME does not load, so it can't be turned off or on: …`);
  one not on the lamp (`no skill NAME on this lamp`). A helper that can't
  be reached is `the lamp's skills can't be changed now`, as for a remove.
  The journal says `lamp-cloud: skills.off from the app: turned NAME off`,
  or its refusal.
- **On a product lamp** the helper reaches `lamp-skills`' state folder
  (`lampos`'s, 0700, as the daily journal requires) with one capability,
  `CAP_DAC_OVERRIDE`, and a writable path in its unit, both from
  `lamp-update install`'s units: a lamp whose helper unit is from before
  #2815 fails closed, answering an off or
  an on with the helper's `Permission denied` (or the folder's read-only
  error) until `sudo lamp-update install …` once.
- Without `LAMP_SKILLS_PAGE=on`, `not on this lamp: skills.off` (and
  `skills.on`).

### The owner's yes to a skill's camera (`skills.camera`)

`{"name": "desk-tutor", "allow": true}` (#3504's piece C), sent once the
owner has read the app's strong warning for a skill that asks the camera's
picture ([skill-format.md](skill-format.md), "The camera"; the warning
screen is the app's), or `"allow": false` to take it back. As off and on:
`lamp-cloud` checks the name and that `allow` is a JSON bool, asks the
manage helper `{"camera": NAME}` or `{"camera_off": NAME}`, writes nothing
itself, and replies with `skills.list`'s page, the skill's
`camera_allowed` now true or false, and beside it
`"camera": {"name": NAME, "allowed": BOOL}`: what the helper kept for that
name, read back from its file after the change (#4099). That holds for a
name not on the lamp yet too, which has no row on the page, so the app can
read back a yes given before `skills.install`. `allowed` is null only when
the file can't be read just then; the app takes that as no yes. A refusal
(a name that is not a skill's, `allow` not a bool, the helper's own) is
V1's `failure` with why, nothing kept and no `camera`.

- **What is kept.** The name in `lamp-skills`' `camera`
  (`/var/lib/lampos-skills-state/camera`, a name a line, beside `off`),
  which a round never touches. The name need not be on the lamp yet: a
  Store package that asks the camera is refused until its yes is kept
  (`not installed: NAME: it asks the camera, and the owner hasn't said
  yes`), so the app says yes before `skills.install`. `skills.remove`
  takes the name out with the skill, so one put on again by that name
  asks again.
- **What it does.** The running `lamp-skills` reads the file at every
  call to a skill that asks the picture: a skill that is not core runs only
  while its name is there. Without it the whole call is refused, nothing
  moved, and the model is told `The owner hasn't allowed this skill's
  camera; they can allow it in the app.`. A yes taken back holds from the
  next call, nothing restarted ([skills.md](skills.md), "The manage
  helper"). A core skill needs no yes.
- Without `LAMP_SKILLS_PAGE=on`, `not on this lamp: skills.camera`.

### Installing from the Skills Store (`skills.install`)

`{"name": "desk-tutor", "version": "1.0.0", "sha256": "…", "url":
"https://…/desk-tutor-1.0.0.tar"}`, a Skills Store catalogue entry (the
first builder's 22:23 on #1722), sent by the app when the person taps Add;
`url` alone is taken too. Only with `LAMP_SKILLS_STORE=on` beside the
page's two switches and a valid Store public key: listing and removing
change nothing new on the lamp, and an install puts new code there, so the page can be on while the Store,
its server and its key are not. Without it, `not on this lamp:
skills.install`. Install writes `/etc/lampOS/skills/store.env` with the
switch off, and both parts read it. The product cloud unit also defaults
the Store to off; placing a key does not turn it on. `lamp-cloud` reads
the key as the helper does (the build's
`/var/lib/lampOS/skills/store-key.pub`, else the factory's; #3692,
`lamp_skills::package::store_keys`), but once, at its start: a round that
brings the first valid key needs `lamp-cloud` started again before the
app is offered installs. The build that first ships the key also ships the
`lamp-cloud` that reads it, and the round restarts that part, after the
settings are written; a later rotation needs no restart, since the helper
reads the key for every install. `lamp-cloud` is not the only gate:
root's manage helper checks the switch and key itself and, off or without
a valid key, refuses an install with the same words whoever asked, while
a remove still runs. Its installed `store.conf` prepares the network even
while off, but the helper creates no HTTP client until both checks pass.
[install.md](install.md), "The Store, prepared but off", gives the factory
key step and the single shared switch to change after it.

- **What `lamp-cloud` does: checks the shape and passes it on.** An
  https URL (`url must be an https URL of at most 2048 characters, with no
  spaces, no user and nothing but plain ASCII`), a SHA-256 as 64 lowercase
  hex, a skill's name, `MAJOR.MINOR.PATCH`, each refused in plain words
  before anything is asked, and nothing the app sent is said back. Then
  `{"install": {...}}` goes to the manage helper over its socket, as a
  remove does, and `lamp-cloud` waits at most 40 s. It writes nothing and
  fetches nothing: a network message never puts code on the lamp through
  it (the first builder, 18:33).
- **What the helper does** ([skills.md](skills.md), "From the Skills
  Store"): checks the request again; reads the Store's key (a build's
  `/var/lib/lampOS/skills/store-key.pub`, else the factory's
  `/etc/lampOS/skills/store-key.pub`); fetches the package itself over HTTPS
  (redirects only to https, at most 18 MiB, within 25 s); checks the
  entry's `sha256` against the bytes; checks `lamp-skill.sig` against the
  key before the manifest is read; checks the package as any install does
  (plain files, no `..`, no absolute path, no link, the size and file
  bounds, each file's SHA-256), and its name and version against the
  entry's; then installs it under the folder's lock (waited for at most 4
  s, `ASKED_LOCK_WAIT`, as a remove's, then `the lamp's skills can't be
  changed now` with nothing written), loaded first and
  renamed into place, never core, never over a core skill, never older
  than the version installed (the same is a reinstall; `not installed:
  desk-tutor 1.10.0 is on this lamp, and this package is the older 1.2.3:
  a skill is never put back to an older version`), and never one that asks
  the camera's picture before the owner's yes to it is kept (`not
  installed: NAME: it asks the camera, and the owner hasn't said yes`;
  `skills.camera` above). Nothing is written before that last step.
- **The key: separate from updates.**
  `/etc/lampOS/skills/store-key.pub`, named by
  `lamp_skills::package::STORE_KEY` and the helper unit's `--key-file`.
  Factory step 3 takes it with `--store-key FILE`, as a 32-byte Ed25519
  public key in base64. A build's key in the skills folder wins over it,
  one or two keys a line for a rotation ([skills.md](skills.md), step 2,
  #3692). No fallback reads the update key. The helper's explicit
  `--key-file` override remains available for a manual run.
- **The reply** is `skills.list`'s, read after the install, as a remove's.
  A refusal is the helper's words on one line (at most 300 bytes), for
  example `not installed: it carries no lamp-skill.sig: …` or `not
  installed: lamp-skill.sig does not match lamp-skill.json: the package
  was changed after it was signed`. A helper that can't be reached is `the
  lamp's skills can't be changed now`.
- **How long, and what waits.** The fetch's 25 s at most, then the checks,
  the folder's lock (at most 4 s) and the install, under the 40 s
  `lamp-cloud` waits (a constant assert holds 25 s + 4 s under it). An
  install can't fit in a remove's 5 s, so it has its own wait. `lamp-cloud` answers one command at a time, so the
  app's other commands wait behind an install; the broker's keepalive (60
  s, a lost link noticed at about 90 s) outlasts it. An answer of
  `starting` at once and `success` or `failure` after, as V1's
  `timezone.set`, would free it, and is not built. Not yet timed on a lamp.
- **Not built:** `skills.catalog` (the signed list the entries come from);
  the voice installing a skill.

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

**Off unless set.** No health or crash report is queued or sent unless all
three are on: `LAMP_CLOUD=on`; `LAMP_REPORTS=on` in `lamp-cloud`'s unit
(and in the voice's, for the stop by voice), both on in the product units
since #2862; and the person's yes, read at
each send, never kept past a change. Without the switch `lamp-cloud` starts
no reports driver, subscribes to nothing for reports, writes no reports
file, and answers `reports.get` and `reports.set` as kinds it lacks.
Without the yes, the driver says
`reports/state` (`share: false`) and does not read health values or queue
crashes. It still consumes local exit notes and keeps their bookkeeping.
The separate local exit-note recorder below does not depend on these
switches or consent; recording a note grants no permission to send it.

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
  `restarts` (each unit's `NRestarts` since boot) and `failed_units` (a sorted,
  unique list of units whose `ActiveState` is `failed`, always `[]` when none
  are observed). Both use one `systemctl show` query within the existing 2 s
  command budget, with its success or failure cached for 60 s. Health also
  adds `waits`, only when something waits
  (`{unit: why}`, both fixed text): with `LAMP_UPDATE_SELF=on` in
  `lamp-cloud`'s own environment too, `systemctl show` of
  `lamp-update-back.timer` within 2 s, and unless it is loaded, persistently
  enabled and active, `{"lamp-update.service": "independent recovery is not
  ready: no armed lamp-update-back.timer"}`: lamp-update's self-update waits
  for it (#2709, `updates.md`). It reads only the timer, not the fallback
  file or its service, which lamp-update checks too; a query that fails says
  nothing. Off (the switch unset), the field is left out and nothing is
  asked. An added field, so still `lamp-report/1`.
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

**Manager query bounds.** The health queries and exit-note metadata query
accept a complete UTF-8 response through 256 KiB within two seconds. Closed
stdout does not end the deadline while the command still runs, and an exited
leader does not end it while a descendant holds stdout. A timeout, oversized
response, invalid UTF-8 or failed command supplies no observation; a prefix is
never interpreted as a successful complete response. Unit-query failures still
cache empty states for 60 seconds, recovery-query failures still omit `waits`,
and unavailable exit metadata still allows the exit note and service stop.
No report consent gate, text filtering or stop/restart allowance changes.

The private nonblocking reader retains the unreaped leader through output
validation and stops its owned group on failure. It never signals a group
after losing that identity to another reaper. A successful response does not
promise cleanup of detached descendants or descendants that closed stdout.
Kernel spawn/reap and filesystem operations have no new hard deadline, and
this per-query limit is not an overall health-report deadline.

The query tests include a writer that never closes stdout and one with a
pipe-holding child. Both must be refused at the size cap before the query
deadline, and their owned processes must be gone within the bounded probe.

`failed_units` is a current observation, not a count or a crash history. A
failed oneshot appears even with `NRestarts=0`; after it recovers, it leaves
the list on the next fresh successful systemd sample. The usual health
cadence and 60 s query cache still apply. Names must exactly match `UNITS`;
unknown names, unknown states, missing fields, malformed records and
duplicate `Id` or `ActiveState` fields do not name a failed unit. Repeated
records for a listed unit are ambiguous and omit that unit. The health
builder filters, sorts and deduplicates the names again. A failed query
produces an empty list and restart map, so `[]` alone does not certify that
every service is healthy. This additive field keeps `lamp-report/1`, needs
no extra switch or query, and is read only for a due or urgent health report
after the existing consent check and voice deferral. The separate `waits`
query and its self-update switch are unchanged.

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

`lamp-update install` places the reserved
`/etc/systemd/system/UNIT.d/exit-note.conf` for every service in Cloud's
fixed `reports::UNITS` list, including its five templates. An install test
holds the installer list equal to Cloud's list and the product's service
files. Each drop-in runs `lamp-cloud exit-note %n` as `ExecStopPost=` with
the existing `-+` prefixes (`deploy/product/exit-note.conf`): the recorder
runs as root and a failure never fails the service's stop. The installer
adds no reporting switch, consent, service write path or network access,
and starts no service. It records reload debt before writing these files;
a failed install retries it. A reinstall leaves identical files alone,
repairs a missing reserved file, and preserves every other drop-in,
including an earlier prefix trial file.
The recorder is appended to any existing `ExecStopPost` commands. If an
earlier stop-post command fails without `-`, systemd skips the later ones
([command ordering](https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html#ExecStopPost=));
installing a hook therefore cannot guarantee a note for every failure.

The recorder writes only systemd's
`$SERVICE_RESULT`, `$EXIT_CODE` and `$EXIT_STATUS`, the unit's name,
`systemctl show`'s restarts and run time, the monotonic time and the boot,
to `/run/lampos-exits/<unit>@<at_us>.json` (root, 0644), the newest 8 per
fixed unit name. An instance of a listed template is stored under the
template name, such as `lamp-setup-change@.service`; its instance text is
never in the note's contents, filename or report. The recorder still asks
`systemctl show` about the actual instance for its run time and restart
count. Empty or unrelated instances are refused. A `success`, or a unit
not on the list, writes nothing; no journal is read.

These bounded local notes can be written with sharing off. With its
reporting switches on, `lamp-cloud` reads the folder every 10 s and takes
each note once, across its own restarts, until the next
boot, even without a yes; queueing and sending a crash still require the
person's current yes. The recorder itself never connects to Cloud.
Core dumps and backtraces are never sent; step 2's panic hook
(`{file, line}` only) comes after step 1 has run
a week.

### Software update alerts

When the lamp updates itself, the company is told at each stage, so a
refused or rolled-back update is seen without asking the person. It is not
a report kind: `lamp-cloud` posts to the backend's alert endpoint, the
base of the account's `llm_base_url` with `/alert` after it (a trailing
`/v1` stays), with the voice's key as the bearer and the body
`{"device_id", "type": "ops", "message", "ts", "alert_id"}`
(`crates/cloud/src/update_alerts.rs`). Health and crash reports are
untouched.

**Off unless set,** by the same three gates as the reports: `LAMP_CLOUD=on`,
`LAMP_REPORTS=on`, and the person's yes, read at each send through the
reports' own reading. Nothing is sent or retried without them, and a stage
seen while any gate is closed is dropped, not kept for later. The small
file below is kept either way: it holds no personal data (outcomes, builds
and times) and is what stops a stage seen before the yes being sent after
it.

**One alert per stage of a round,** from `update/status`: `started` (the
first phase after `idle` that goes on to fetch; a round that only checks
says nothing), then `done` (`switched`), `failed`, `refused`, `rolled-back`
or `self-switched` (lamp-update switched itself; its new self confirms on
its next round). A round whose `started` was told and that ends with
nothing switched (`not-quiet`, `unreachable`, `would`, `up-to-date`,
`nothing-published`) ends as `not-switched`, with `code:` the outcome's
own name (`other` for a name not known), so no start is left without an
end. Those outcomes in a round with no `started` told are not alerts. The message is a fixed template:

```
Software update lamp - done
from: 3309+4a39f04
to: 3310
took: 95 s
```

`from` is the build the lamp ran when the round began (digits, hex digits
and `+`), `to` the build the round was about. A `code:` line (a fixed
reason name) and the `took` time come from lamp-update's `last.reason` and
`last.started_at_unix_ms` when it says them; absent, the lines are left
out. A reason that is not on `update_alerts::REASONS` is not told, and
that list is empty until lamp-update's names are agreed. Every value is
typed so that no text that came in can reach an alert (the part, a build,
a stage, a number); a canary test plants secrets in every input.

**A repeat is told once.** A lamp that keeps being refused, failing, or
talked to through every update window would send the same alert each round.
A `refused`, `failed` or `not-switched` end is told once per stage, build
and code (a missing build or code counts as its own value), and told again
only after a round ends some other way (`done`, `rolled-back`,
`self-switched`, or a different stage, build or code). While such a streak
holds, the `started` of the next rounds is not told either, since its end
would not be. The streak is kept in `update-alerts.json` too, so a restart
mid-streak tells nothing again; a file from before this field loads as no
streak. So the end that breaks a streak is told without its start, and with
`took:` once lamp-update gives when the round began.

**Once, across restarts.** `update/status` says the last result every
minute and again whenever lamp-update or lamp-cloud starts, and a round
restarts lamp-cloud when it changed it. `update-alerts.json` in
lamp-cloud's state folder (0600) keeps the last end told, keyed on outcome,
build and time, and the round whose start was told, so neither goes twice
and a start from the old lamp-cloud and the end from the new one are joined.
With no such file the result found is taken as told. A factory reset empties
the folder with the rest.

**Best effort:** a 10 s timeout, one try, a failure said in the journal
(`an update alert was dropped: …`) and dropped; never queued or retried.

**The first build that carries alerts cannot send its own rollback:** a lamp
put back from it runs the previous `lamp-cloud`, which has none. A
`rolled-back` is seen from the second such build onward.

### The person's choice

- **At setup**, `"share_reports": true` in the setup request; only the
  JSON `true` is a yes. Setup keeps the answer, a no included, in
  `/etc/lampOS/setup/reports.json` ([setup.md](setup.md)), which
  `lamp-cloud` reads where it is. The lamp's own setup page asks too
  (#2862): "Send health and crash reports", a box left unticked, which
  sends `share_reports` only as the box is. Unticked, it is off.
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
- **Installed:** the helper's two units are in `lamp-update`'s install list
  (`install::PRODUCT`, #2853): the socket enabled, the template never.

### The owner's place (`owner.place`, #4333)

The app gives the lamp its owner's approximate place, so "near me" needs no
asking ([owner-place.md](owner-place.md) has the whole contract). Off unless
`LAMP_OWNER_PLACE=on`, beside `LAMP_SETTINGS_CHANGE=on` and `LAMP_CLOUD=on`;
`can` lists `owner.place` and `owner.place.clear` only then. With it off,
`owner.place.clear` is still taken, unlisted, whenever the helper is there
(`settings::PLACE_CLEAR_KIND`, #4356), so a place kept while it was on
never outlives it. `name` is an area name, the neighbourhood, district or
city the phone's geocoder gives (the app joins sub-locality and locality),
never a street address; digits are not refused ("District 1"). `lat`/`lon`
is where the lamp stands, set by the person at setup (with the lamp) or when
they tap Update now, never the phone's position resent as the owner moves. The app
rounds `lat`/`lon` to 2 decimals before sending, so the exact position never
crosses the relay. `lamp-cloud` checks and rounds the place (`lamp_msgs::owner_place::fit`: latitude to
0.01°, longitude to a step at least 1 km wide), so what leaves it is never
finer than the grid, and hands `{"owner_place": …}` (or `null` to clear) to
`lamp-setup change`, which rounds it again and keeps it beside the person's
name. The journal names the kind and its status only, never the place.

### The Wi-Fi (`wifi.set`)

V1 has no Wi-Fi kind on the broker (a V1 lamp changes Wi-Fi only on its
hotspot's `/wifi` page), so this one is new. Off unless
`LAMP_SETTINGS_WIFI=on` **beside** `LAMP_SETTINGS_CHANGE=on` and
`LAMP_CLOUD=on`; without it, `not on this lamp: wifi.set`.

- **Asked:** `{"cmd":"data","kind":"wifi.set","data":{"ssid":"…","password":"…","request_id":"…"}}`.
  `request_id` is optional, the app's own, 1 to 64 printable characters, and
  said back. An open network has `"password": ""`.
- **Answered:** `starting` at once. Once the new network is proven, the
  link to the broker is dropped and made again at once over it (the old one
  is dead, and the keepalive alone would notice only after 60 to 120 s;
  #2617's P2-1), and on the new link, a kind of its own, **`wifi.joined`**
  `success` with `data: {"ssid", "confirm_within_s": 120, "request_id"}`:
  joined, confirm please. Then, as `wifi.set`, `success` with
  `data: {"ssid", "request_id"}` only once the app's confirmation has kept
  the network, or `failure` with `error` and `data: {"request_id"}`. An app
  that does not know `wifi.joined` never confirms, and the lamp goes back:
  it fails safe.
- **Confirmed by the app (#1959's P1, #2617's P2-2):** on `wifi.joined`, the
  app sends `{"cmd":"data","kind":"wifi.confirm","data":{}}`, which reaches
  the lamp only over the new network. A `request_id` in it must be the
  change's (`wifi.set`'s), else `failure`, `this wifi.confirm's request_id
  is not the Wi-Fi change's`, and nothing is handed on; one with none is
  taken as before (#2617's follow-up 5), and one with a `request_id` is
  taken for a `wifi.set` that had none, which has nothing to differ from
  (#2686's P3 5). **The window is the helper's**, sent
  in its `joined` line (`confirm_within_ms`, 120 s), and **starts when the
  broker acks `wifi.joined`**, not at the join: `lamp-cloud` starts its copy
  then and tells the helper (`{"online": true}`), which starts its own, so
  `lamp-cloud`'s never ends after the helper's. Its wait for that ack is the
  helper's bound less 1 s (`online_within_ms`, 60 s, so 59 s;
  `WIFI_ONLINE_MARGIN`), so it ends before the helper's, which started
  first (follow-up 2). A `wifi.joined` not yet said when that wait ends (the
  link not back in time) is dropped, never said after the lamp has gone
  back (follow-up 3). Within the window a
  `wifi.confirm` is answered `success` at once, and so is a second one (the
  app resends every 10 s); after it, or with no change waiting, `failure`,
  `no Wi-Fi change waits for a confirmation`. A confirmation is answered
  once: `success` and handed to the helper, or `failure` and not. Without
  one, the lamp goes back to its old network, the link is made again over
  it, and `wifi.set` `failure` follows, `the app did not confirm the new
  Wi-Fi within 120 s; the lamp is back on its old Wi-Fi`. The errors are
  setup's page's words (`a Wi-Fi
  password has 8 to 63 characters`, `the Wi-Fi name is longer than 32
  bytes`, …) or what the change came to: `wrong WiFi password: the router
  rejected the key; the lamp is back on its old Wi-Fi`, `connected to the new
  WiFi, but the lamp's server could not be reached; the lamp is back on its
  old Wi-Fi`, or `…; the old Wi-Fi did not come back after 3 tries, and the
  lamp keeps trying it`. Also `a Wi-Fi change is already under way` (one at a
  time) and `the lamp is not on its broker`.
- **The result may come only after the lamp is back online:** the lamp's link
  to the app is over the very network that changes. The helper is waited on
  10 min for each line over a local socket the network change does not touch, and results
  are handed to the broker only while connected, at QoS 1 on `fd_channel`
  (as every reply, `keys.*`'s too), and kept until its PubAck. One handed and
  lost with the old connection goes again, first, on the next one, with the
  same `request_id`. A change that never joined (a wrong password, a network
  out of range) leaves the old link alive, and its failure goes out on it
  (Mobile on lamp-0c89, #1959: the result lost there was the app's); if the
  broker does not ack it within 5 s (`WIFI_RESULT_ACK_WAIT`), the link is
  made again and the failure goes on the new one, rather than waiting up to
  2 min for the keepalive on a link the radio's switch killed (follow-up 3). Kept in
  `lamp-cloud`'s memory only: a `lamp-cloud` restarted mid-change loses its
  result (the change itself finishes in root's helper either way).
- **The journal (#1959):** one line per step of each change's result: `a
  change's result, KIND STATUS, request_id ID: handed to the client at QoS
  1`, `…: sent as packet N`, `…: acked by the broker, packet N`, or `…: no
  ack before the connection ended (packet N), said again on the next`. The
  kind, the status and the app's id only, never the data or the error.
- **Confirmed and the way back:** root's Wi-Fi helper (`setup.md`, "The
  Wi-Fi changed after setup") takes the new network only once it is joined
  with an address and a TCP connection to the broker `lamp-cloud` is on (sent
  in its line) is made, within 60 s, and the app's `wifi.confirm` within
  120 s of the broker's ack of `wifi.joined`; else the old network comes back, `wpa_cli reconfigure` first and up
  to 3 tries of the radio, and never the hotspot. The lamp says what happened aloud, never
  the network's name or password. **The socket's group is the gate**
  (`lamp-setup-wifi.socket`, group `lampos`, 0660; the unit always passes
  `--enable`): once installed, any `lampos` process can ask for a Wi-Fi
  change, and only one whose broker the new network reaches is kept.
- **The password** goes from the app's message into the helper's line and
  the lamp's network file, and nowhere else: never a reply, an error or a
  journal line (`lamp-cloud` journals no payload). **The backend passes the
  password through to the lamp and never stores or logs it** (the backend's
  side, to be confirmed by them).

### A person's own keys (`keys.*`, #2094)

Off unless `LAMP_OWN_KEYS=on` beside `LAMP_SETTINGS_CHANGE=on` and
`LAMP_CLOUD=on`. `lamp-cloud` hands one line (`{"set", "key", "base_url"?}`,
`{"remove"}` or `{"which": true}`) to root's keys helper on
`/run/lampos-setup-keys/keys.sock`, on a thread of its own, waiting 15 s at
most (the helper's one check is 5 s). The helper answers only a peer in
`lamp-cloud.service` at `/lampos.slice/lampos-parts.slice/`, by its cgroup, since every part runs as `lampos`. The
key goes from the app's message into the helper's line and the key's file,
and nowhere else: never a reply, an error or a journal line. **The backend
passes the key through to the lamp and never stores or logs it** (the
backend's side, to be confirmed by them). The rest is in
[own-keys.md](own-keys.md).

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

The three Wi-Fi reconnect fixtures keep their setup-helper socket open
until the scenario has stopped and awaited `lamp-cloud`, then signal the
helper to finish and join it. A fifteen-second bound diagnoses a missing
finish signal; it is not a successful test's duration. The helper's former
three-second hold is gone. The real 300/1,600 ms join delays, one-second
broker acknowledgement delay, 2,400 ms backoff gate and every original
assertion remain: reconnect promptly, start the confirmation window only
at the acknowledgement, reset backoff, report rollback, and retain the
offline will on the replacement connection.

The no-join failure fixture observes the Wi-Fi worker's completion before
checking whether a result needs a relink. It keeps both acknowledged and
unacknowledged branches and the five-second result-ack contract. A
5.5-second watchdog bounds the event wait in place of the former fixed
5.5-second settling delay. The acknowledged branch ends when its worker
completes; a fixed sleep no longer determines either branch's end.

The reports (#1683): each kind's fields as sent; the canary; each of the
three gates closed sends nothing (no setup file, a no, one unreadable,
`"true"`, `1`, a newer wording; a later no of the lamp's own); the bus only
turning reports off; a no deleting the queue and its files with nothing
after but `stopped`; a no given offline kept across a restart, sent on the
next connection and once more on the one after; the bounds (12 an hour, 50
a day, 64 reports, healths before crashes, 7 days) and the 4 KiB cut; exit
notes (a success and a unit not lampOS's write nothing, 8 per fixed unit,
each taken once across a restart, a new boot starting again); a listed
template instance reduced to its fixed name in the note and report,
unrelated or empty instances refused, and a local note granting no
reporting consent; the drop-in's line; the lists against the product's
units; `reports.get` and
`reports.set` with and without the switch; and against the test broker, a
report on `fd/…/report` with `info`'s id leaving the queue on the ack, and
sent again after a dropped connection. `tests/it/report_contract.rs` pins the
fields by name and type, the topic, and `sent_at` kept on a send again.
The failed-unit fixtures cover a failed oneshot becoming inactive while its
restart count stays zero; one shared restart/state query, cached success
and failure, no query without consent or before the due voice wait ends;
ambiguous records and an unknown-name canary; canonical names, recovery,
unchanged waits, retransmission, schema `/1` and the 4 KiB limit. The source
fixture runs a local stand-in command through the production query path;
it does not exercise a live systemd or a lamp.
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
why; the switch for a skill that loads and the off file; the bounds; a folder that can't be read; the
switch's two gates and the refusal's exact words. `skills.remove`: a core
skill refused and left whole; a made-here skill removed, the reply the page
as it now is, and asked again, not there; an installed skill removed, and
one that does not load; an unknown name; every path trick (`..`, `/`, NUL,
a hidden name, 65 bytes, not a string) refused with the folder and the
folder beside it untouched; a link left as it is; a folder that can't be
changed leaving the skill whole; no helper, or one answering nonsense,
removing nothing; and off, `not on this lamp: skills.remove`. Each remove
goes through the manage helper's own code on a real Unix socket on a short
`/tmp` path. `tests/it/skills_contract.rs` pins the fields as sent, and
removes Desk tutor from a copy of `lampOS/skills/` through `Real`, the
socket and the envelope. `skills.off` and `skills.on` (#2815): an installed
skill turned off, the page as it now is with it off and the off file
saying so, then on again; Night sleep, granted a core operation, offered
no switch and refused with why; a name that is not a skill's
(`a_skill_is_turned_off_and_on_and_one_of_the_lamp_itself_is_refused`).

Changes after setup (#1959): the switch's two words; each kind refused as
one this lamp lacks without it, `device.rename` included; each kind's line for
the helper; the ack `starting` within 100 ms while a stand-in helper takes
300 ms, then its result; a refusal, a helper not running and one that never
answers; through the test broker, `starting` then `success` on `fd_channel`;
and `host.device_name` from the helper's file, the host's name without one.

`tests/it/app_contract.rs` holds the fields the Autonomous app's device page
reads, by name and type (Fourth on #1396): `versions.firmware`,
`network.private_ip` and `host.hostname` of `system.info`; `volume`, `raw`
and `max_volume` of `volume.get` and `volume.set`; `muted`,
`hw_switch_muted` and `available` of `mic.get`. Each is checked in the real answer, as sent.
The app reads them leniently, so a renamed or retyped field would otherwise
go blank on its page unnoticed. `hw_switch_muted` is null while nothing reads
the switch (the app takes that as not known, so not blocked by the switch), and `versions.openclaw` and `versions.openclaw_detected` stay out
of the reply: only V1's "Starting" rule waits for them.
