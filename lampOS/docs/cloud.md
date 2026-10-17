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
  reply carries `device` (`lamp`), `type`, `version` (the installed build,
  `<sequence>+<commit>`), `id`, `mac` (the lamp's `lamp-XXXX` name), `time`,
  `wakeword_enabled` and `voice_input_mode`, and `timezone` and `local_ip`
  when known.
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
| `system.info`, `system.version`, `system.network` | V1's shapes: the versions (`build`, `lamp-cloud`); the default route's interface, its address, MAC, Wi-Fi name and gateway; the host's name, device id, uptime and zone |
| `volume.get`, `volume.set` (`{"volume": 0..100}`) | through `lamp-actions`' socket (`volume_level`, `volume_to`, `actions.md`), its only writer: `{"volume", "raw", "max_volume"}`, a set read back |
| `mic.get` | `{"muted", "hw_switch_muted", "available"}`: the voice's own mute (`voice/mute`), the privacy switch (`voice/privacy`, `null` while unknown) and whether the voice is running |
| `lamp.health` | `lamp/health`'s `level` and `message`, and `stale` |
| `mic.set` | refused: "on this lamp the microphones are muted and unmuted with the button or the switch". The person's own switch and button are the only way |
| `device.soft_reset` | refused until the owner says otherwise: a destructive act from anywhere is the owner's call |
| `device.rename` | refused for now; step 2, through setup's record |
| anything else (`add_channel`, `skills.*`, `face.*`, `tts.*`, `led.*`, `system.reboot`, …) | V1's `failure`, `"not on this lamp: <kind>"` |

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
  three are there and setup is done. It conflicts with V1's
  `os-server.service`, which holds the same client id.
- **Never printed:** the broker's password and the key appear in no journal
  line and on no bus topic; the account's debug form hides them.
- **The reset** empties `/var/lib/lampos-cloud` and removes
  `/etc/lampOS/setup/backend.json` with the rest of the person's data.

## Not yet known, for the backend (#1396)

- Whether the backend's `connected` comes from MQTT, the ping, or both, and
  where the app's `lan_ip` comes from.
- Whether a `lamp-v2` record's replies must say `"device": "lamp-v2"`.
- Whether the issued port is plain MQTT (V1's `mqtt://`) or TLS, and v5 or
  3.1.1. The client is built with rustls, unused while the port is plain.
- `version` is the build when `lamp-cloud` started: `state.json` is handed
  in at start, so after an update it says the new build once `lamp-cloud`
  restarts with it.

## Tests

`cargo test -p lamp-cloud`: the account and its rotation, V1's shapes, each
kind's answer and refusal, the ping's URL, body and reply, and, against a
broker in the test built from `rumqttc`'s own codec: coming online as V1
(client id, keepalive, clean start, expiry, login), `info` on connect and
answers on `fd_channel`; a refused login backing off and said once; a dropped
connection made and subscribed again. A backend in the test checks the ping's
bearer and body, a failure said once, and a rotated broker kept. Not yet run
against the company's broker or on a lamp.
