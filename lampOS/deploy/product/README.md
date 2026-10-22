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

| File | What |
|---|---|
| `lampos.target` | The lamp. Starts only once setup's marker `/var/lib/lampos-setup/done` is there (#806) |
| `lampos-voice.slice`, `lampos-parts.slice` | As in `deploy/units/` |
| `lampos.conf` | `/etc/tmpfiles.d/lampos.conf`: the bus root and the servo and camera locks |
| `settings/` | What no build carries and every lamp shares: Expression's catalogue (from lamp-4ace's `expression-alive-name-speech`) and the controls' two files, installed in `/etc/lampOS/<part>/` |
| `lamp-<part>.service` | Each part, `PartOf=` and `WantedBy=lampos.target`, in keeper.md's order |
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
| `lamp-setup-keys.socket`, `lamp-setup-keys@.service` | Root's helper for a person's own DeepSeek and Deepgram keys from the app (#2094, `docs/own-keys.md`): its own socket, answering only a peer in `lamp-cloud.service`, with the network for the key's one check. In `install`'s list (#2846): the socket enabled, the template never |
| `exit-note.conf` | Not a unit: `lamp-update install` writes this reserved drop-in at `/etc/systemd/system/UNIT.d/exit-note.conf` for the 33 fixed reporting services, including templates. `lamp-cloud exit-note %n` runs as their `ExecStopPost=` and keeps bounded local notes; Cloud’s reporting switches and consent still gate transmission (#1683, `docs/cloud.md`) |

`lamp-update install` (docs/install.md) carries all of these inside it and puts them on a lamp.

Not here yet: the keeper's unit (asked of the fifth on #811), and
`lamp-surround`, which no lamp runs.

Checked with `systemd-analyze verify` (systemd 252, Debian bookworm, arm64,
in Docker on the development Mac), with an empty executable at each
`current/` path and the two users made: no complaint. The same check finds
an unknown key and a missing binary in a unit made wrong on purpose.
