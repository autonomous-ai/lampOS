# The product units

The units every lampOS lamp runs, the same on every lamp
([install.md](../../docs/install.md), task #811). Unlike
[`deploy/units/`](../units/README.md) they have no `@NAME@` to fill: each
part's binary is `/opt/lampOS/<part>/current/lamp-<part>`, `current` being
a link to the build's folder, so an update needs no edit of a unit.

What is one lamp's own is outside them:

- `/etc/lampOS/body/lamp.env`: `BODY_POSES="--gaze .. --stand ..
  --folded-arm .. --home .."`, measured on that lamp ([body.md](../../docs/body.md)).
  Required: the body does not start without it.
- `/etc/lampOS/zone.env`: `LAMP_ZONE=` (schedule) and `LAMP_UTC_OFFSET_MINUTES=`,
  from the system's zone, written again by setup with the person's zone
  (#820). Memory, Expression and skills follow `/etc/localtime` instead.
- A switch a lamp must have off: a drop-in,
  `/etc/systemd/system/lamp-<part>.service.d/<why>.conf`.

The voice's folder holds two more binaries the voice starts from its own
folder: `lamp-turn-end`, always, and `lamp-same-voice` when the build
carries it ([same-voice.md](../../docs/same-voice.md)). The latter runs
only when `LAMP_SAME_VOICE_MODEL` is set, which `lamp-voice.service` sets
since 4 November with `LAMP_SAME_VOICE_FOR=services`: the prints are for
the linked services alone, and no conversation rule reads them.

| File | What |
|---|---|
| `lampos.target` | The lamp. Starts only once setup's marker `/var/lib/lampos-setup/done` is there (#806) |
| `lampos-voice.slice`, `lampos-parts.slice` | As in `deploy/units/` |
| `lampos.conf` | `/etc/tmpfiles.d/lampos.conf`: the bus root and the servo and camera locks |
| `settings/` | What every lamp shares, from `install` (`install::SETTINGS`, and the voice's `PROVIDER` and `SECOND`): Expression's catalogue `expression.json` (from lamp-4ace's `expression-alive-name-speech`) and the controls' two files `controls.json` and `controls-pins.json`, in `/etc/lampOS/<part>/`; records' bound `records.json`, in `/etc/lampOS/records.json`; the voice's two services `voice-provider.json` and `second.json`, in `/etc/lampOS/voice/provider.json` and `second.json`, private and kept up to date while they are install's own. The Store's files have their own row below, and `var/lib/lampOS/skills/store-key.pub` is the Store's key a build carries (`install::STORE_KEY_SOURCE`) |
| `models.json` | Not a unit: the models no build carries (#811), each a path, a release URL, a size and a SHA-256. `install` fetches each one missing and checks it before placing it ([install.md](../../docs/install.md), "What was not shown") |
| `lamp-<part>.service` | Each part, `PartOf=` and `WantedBy=lampos.target`, in keeper.md's order |
| `lamp-waking.service` | The waking face (#1681, [ring-choreography.md](../../docs/ring-choreography.md), "The waking face"): outside the target, `WantedBy=sysinit.target` and only once setup's marker exists, so the ring lights soon after power; on in the product (`LAMP_WAKING_FACE=on`) |
| `lamp-setup-recover.service` | Setup's hotspot again when a set-up lamp's saved network is gone (#3020, [setup.md](../../docs/setup.md), "When the saved network is gone"): outside the target, `WantedBy=multi-user.target`, only once the marker exists; it exits at once unless a drop-in sets `LAMP_WIFI_RECOVERY=on` |
| `lamp-update-apply.service` | Outside the target (#1680, [updates.md](../../docs/updates.md), "Settings and tmpfiles"): writes a build's settings and tmpfiles at a round's switch, from install's fixed path, never `update/current`; started only by a round with `LAMP_UPDATE_APPLY=on`, never enabled |
| `lamp-eyes-camera.socket`, `lamp-eyes-camera@.service` | The eyes' camera helper (#2892, [eyes.md](../../docs/eyes.md), "The camera's helper"): root's, re-authorizes the camera's USB device when it stops opening, at most once a minute. In `install`'s list: the socket enabled (`WantedBy=sockets.target`), the template never |
| `lamp-update.service` | Outside the target; `--apply on`; the channel on GitHub |
| `lamp-update-back.service`, `lamp-update-back.timer` | Outside the target (#1680): lamp-update's own previous build put back, from the fixed path `/usr/local/lib/lampos/lamp-update-back`, when a switch of its own (`LAMP_UPDATE_SELF=on`) is not confirmed; started by `lamp-update.service`'s `OnFailure=` and every 2 min by the timer. The service has no `[Install]` |
| `lamp-records.service`, `lamp-records.timer` | Hourly, outside the target |
| `lamp-selftest.service`, `lamp-selftest.timer` | The passive self-test (#812): the timer is in the target and waits for setup's marker; it runs the service 5 min after the lamp starts and every 6 h. The service has no `[Install]` |
| `lamp-setup.service` | First boot only: while the marker is missing and setup's settings exist (#806, #811) |
| `lampos-setup-done.path` | Starts `lampos.target` once the marker exists ([install.md](../../docs/install.md), "Setup at first boot, without the keeper") |
| `lamp-setup-reset.service` | In the target: the reset button held 5 s |
| `lamp-setup-change.socket`, `lamp-setup-change@.service` | Root's helper for names and the zone changed from the app after setup (#1959, `docs/setup.md`, "Changed after setup"): the socket is in the target and waits for setup's marker; each connection from `lamp-cloud` starts one service. In `install`'s list (#2853): the socket enabled, the template never |
| `lamp-keeper-restart.socket`, `lamp-keeper-restart@.service` | The keeper's helper for a restart from the app (#2193, `docs/keeper.md`, "A restart asked from the app"): each connection from `lamp-cloud` starts one `lamp-keeper restart-asked`, which refuses while an update installs, else answers and restarts `lampos.target` once quiet. The socket is wanted by the target but not part of it. Not yet in `install`'s list, so no lamp gets them until their lamp check |
| `lamp-setup-wifi.socket`, `lamp-setup-wifi@.service` | Root's helper for the Wi-Fi changed from the app after setup (#1959, `docs/setup.md`, "The Wi-Fi changed after setup"): its own socket, one change at a time, with the network the names' helper has not. In `install`'s list (#2853): the socket enabled, the template never |
| `lamp-setup-wifi-restore.service` | The Wi-Fi helper's boot half (#1959's P1, `docs/setup.md`, "The Wi-Fi changed after setup"): a oneshot before `wpa_supplicant@wlan0`, outside the target, that runs only while `/etc/wpa_supplicant/wpa_supplicant-wlan0.conf.lamp-previous` exists and puts the old network's file back after a change a power cut interrupted. In `install`'s list with the helper it belongs to, and enabled (#2853) |
| `lamp-skills-manage.socket`, `lamp-skills-manage@.service` | Root's helper that removes an installed or made-here skill for the app's skills page, and installs a Skills Store package signed with the separate Store key at `/etc/lampOS/skills/store-key.pub` (#1722, `docs/skills.md`, "The manage helper"; `docs/cloud.md`, "The skills page"). In `lamp-update`'s install list: the socket (group `lampos`, 0660) is enabled, the template never; each connection starts `lamp-skills asked` as root with only the skills and skill-state folders writable. Store installs require `LAMP_SKILLS_STORE=on` and a valid Store key; remove, off and on need neither. There is no update-key fallback |
| `settings/store.conf`, `settings/store.env` | Install writes the helper's `/etc/systemd/system/lamp-skills-manage@.service.d/store.conf` and the shared `/etc/lampOS/skills/store.env`, preserving lamp edits. These are `STORE_SETTINGS`, excluded from an update round's settings manifest. Cloud and the helper read the shared switch, initially `LAMP_SKILLS_STORE=off`. The helper drop-in prepares IPv4 and IPv6 even while off; its code constructs no HTTP client until the switch and key checks pass. Factory step 3 provisions the separate key with `--store-key FILE` without enabling the Store; see `docs/install.md`, "The Store, prepared but off" |
| `settings/models.env` | The models switch file, `/etc/lampOS/models.env` (`install::MODELS_SETTING`, `manifest::MODELS_ENV`): `LAMP_UPDATE_MODELS=on`, carried among every build's settings from updater 11 and read by lamp-update itself, never by systemd, when its environment doesn't set `LAMP_UPDATE_MODELS`. Install doesn't write it. See `docs/updates.md`, "Models in the manifest" |
| `lamp-setup-keys.socket`, `lamp-setup-keys@.service` | Root's helper for a person's own DeepSeek and Deepgram keys from the app (#2094, `docs/own-keys.md`): its own socket, answering only a peer in `lamp-cloud.service`, with the network for the key's one check. Not yet in `install`'s list, so no lamp gets them until their lamp check |
| `lamp-services.service` | The person's linked services (Queue #4710): root's store of their tokens in `/var/lib/lampos-services` (0700, a 0600 file each, wiped by the reset plan), `admin.sock` for `lamp-cloud` and the tools folder `tools/` (`tools.json`, `call.sock`) for the voice under `/run/lampos-services`, each answering only its one unit by cgroup. Reads, and sends with `LAMP_SERVICES_WRITE=on`, set in the unit since 4 November (a drop-in with `LAMP_SERVICES_WRITE=off` leaves reads only). A made skill's one fixed write is sent with `LAMP_SKILL_WRITES=on`, set since 11 November in this unit, `lamp-skills.service`, `lamp-skills-manage@.service` and `lamp-voice.service`, only after the owner's yes to its content, at most three a day, never a public post. In `install`'s list and enabled, with `LAMP_SERVICES=on` (the same line as in `lamp-cloud.service` and `lamp-voice.service`); a drop-in with `LAMP_SERVICES=off` turns it off, and it then exits at once with no socket opened |
| `own-keys-voice.conf` | Not a unit and not installed by `install`: the voice's drop-in for `LAMP_OWN_KEYS=on` (#2094, `docs/own-keys.md`), `/etc/systemd/system/lamp-voice.service.d/own-keys.conf`, with the switch and the `LoadCredential=` of the keys' folder |
| `lamp-control.service` | The developers' door (#3504, `docs/control.md`): as `lampos` in `lampos-parts.slice`, installed and enabled on every lamp, and off (exits 0, nothing listening) unless a drop-in sets `LAMP_CONTROL=on`, which the owner's app setting will write. It passes the body's `--lease-socket` (opened by lamp-body's unit, #4568) and lamp-actions' `--actions-socket` for `volume`, and `--manage-socket` at root's skills manage helper (`/run/lampos-skills-manage/manage.sock`), so skill push is on: a paired computer with the `skills` grant pushes, lists and removes its own skills (`docs/control.md`, "Skills pushed from a computer"; test `the_door_passes_the_skills_manage_socket_it_listens_on`) |
| `exit-note.conf` | Not a unit: `lamp-update install` writes this reserved drop-in at `/etc/systemd/system/UNIT.d/exit-note.conf` for the 37 fixed reporting services, including templates (`install::EXIT_NOTE_UNITS`). `lamp-cloud exit-note %n` runs as their `ExecStopPost=` and keeps bounded local notes; Cloud’s reporting switches and consent still gate transmission (#1683, `docs/cloud.md`) |

`lamp-update install` (docs/install.md) carries all of these inside it and puts them on a lamp.

Not here yet: the keeper's unit (asked of the fifth on #811), and
`lamp-surround`, which no lamp runs.

Checked with `systemd-analyze verify` (systemd 252, Debian bookworm, arm64,
in Docker on the development Mac), with an empty executable at each
`current/` path and the two users made: no complaint. The same check finds
an unknown key and a missing binary in a unit made wrong on purpose.
