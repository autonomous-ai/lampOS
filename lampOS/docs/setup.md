# Setup for a home (#806)

A lamp in a home is set up by its person, with a phone and no engineer: the
home's Wi-Fi, the voice's service, the person's name and time zone. Holding the
reset pinhole (RESET, on the back of the base) with a paperclip for 5 s and
letting go takes it back to setup and wipes the person's data. The
first builder decided (2026-10-15) that V2 ships with **V1's setup flow**, not
a new one. This page studies that flow and says, part by part, what V2 takes.

The study read V1 on `origin/main` at `be040af6704d`. Paths below are V1's,
on that commit. Nothing here was run on a lamp.

## What a person does in V1

1. **Power on.** The light breathes dim white, then shows "booting". The Go
   server (`os-server`) reads `set_up_completed` in `/root/config/config.json`;
   while it is false it starts the hotspot and turns the light solid white
   (`system/server/config_watch.go`). The lamp says nothing during setup.
2. **The phone joins the open hotspot** `lamp-xxxx` (no password; the suffix is
   the last four characters of the board serial or the Ethernet address). It
   gets an address in `192.168.100.50–150`; the lamp is `192.168.100.1`.
3. **The setup page.** Normally the autonomous.ai account page opens it as
   `http://192.168.100.1/setup?llm_api_key=…&device_id=…&parent_origin=…`.
   Any address also reaches the lamp: the hotspot's DNS answers every name with
   the lamp, and `/` redirects to `/setup`. The phones' sign-in checks
   (`/generate_204`, `/hotspot-detect.html`, …) get 204, so no sign-in sheet
   pops up; the person opens a browser.
4. **The wizard** shows one step in normal use: pick the home network, type
   its password, say whether to send health and crash reports (a box left
   unticked, #2862), press Setup. The page's admin password defaults to the
   sticker suffix. The AI, channel, language and voice steps appear only with
   `?debug=true`.
5. **"Your device is joining Wi-Fi…"** The light blinks blue. About 2 s after
   Setup the hotspot drops (the lamp has one radio). The page follows the lamp
   to its new address if it caught it.
6. **"Preparing your robot's voice…"**, up to 5 minutes. Then the light goes
   to rest and the agent greets the person.
7. Optional: record "My Voice" (a name and a recording) and a face.
   **Time zone is not in setup**: it is in Settings, behind the admin login.

## How V1 does it

| Part | V1 code and files |
|---|---|
| Server | `system/server/device/delivery/http/handler.go` (`Setup`, `SetupStatus`, `WifiProvision`), `system/device/setup.go`, `system/network/{service,recovery,setup_failure}.go`. Behind nginx on port 80, which serves the web bundle and proxies `/api` to `127.0.0.1:5000`. |
| API | `POST /api/device/setup` (open until setup is done; body `SetupRequest`: `device_id`, `llm_api_key`, `llm_base_url`, optional `ssid`/`password`, …), `GET /api/device/setup/status` (`phase: idle/connecting/connected/failed`, `lan_ip`, `error`, `mac`, `run`, `set_up_completed`, `runtime_phase`), `GET /api/network` (scan), `POST /api/device/wifi-provision` (hotspot clients only), `POST /api/system/factory-reset`. |
| Page | `system/web/src/pages/setup/*`, `hooks/setup/useSetupStatusPolling.ts`, `pages/wifi-provision/WifiProvision.tsx`. |
| Radio | Image scripts written by `scripts/imager/build-orangepi.sh`: `device-ap-mode` (hostapd, open 2.4 GHz channel 6, dnsmasq answering every name with `192.168.100.1`), `device-sta-mode`, `connect-wifi SSID [PASS]` (rewrites `wpa_supplicant-wlan0.conf`). The image removes NetworkManager. |
| Done | `set_up_completed: true` in `config.json`, written only after the agent runtime is ready. Also there: the Wi-Fi name and password in plain text, `llm_*`, `device_id`, the admin password's bcrypt hash. |
| Time zone | `system/device/timezone.go`: writes `/etc/localtime` and `/etc/timezone`, runs `timedatectl` when present. |
| Reset | Orange Pi: a separate button on chip 0 line 100 (header pin 37), pull-up, active low, held 5 s (`robots/lamp/gpio_button.json`). HAL says "Factory reset starting", waits 3 s, releases the servos, then calls `POST /api/system/factory-reset`, which wipes `config.json`, the agent's state, users and strangers, snapshots, the Wi-Fi configuration, logs and every runtime's persona and memory, keeps `bootstrap.json` and the image, and reboots. At most once in 5 minutes. |

**The Wi-Fi switch.** After Setup: 2 s, `connect-wifi`, then up to 60 s for the
named network **and** the Internet; two wrong-key reports from wpa_supplicant
end it early as `wrong_password` (others: `ssid_not_found`, `no_dhcp`,
`no_internet`). On failure nothing is saved and the hotspot comes back in about
5–8 s. **The stuck-screen fix:** while the hotspot is still up, a loop
publishes the new LAN address every second; the page polls status every
600 ms, calls the hotspot lost after 5 s of silence, tries the new address
first, then `<host>.local` every 2 s, and shows failure at 80 s. On a first
setup the hotspot often drops before DHCP finishes, and on routers that block
mDNS nothing can redirect: the page can show failure after a join that worked.
After setup, 90 s without Wi-Fi brings back a recovery hotspot, which retries
the saved network every 2 minutes only while no phone is on it.

## Verdicts for V2

| Part | Verdict | Why |
|---|---|---|
| Open hotspot `lamp-xxxx`, `192.168.100.1`, every name answered with the lamp | **Take as is** | The same on a V2 lamp, which runs on V1's image. |
| `device-ap-mode`, `device-sta-mode` on the image | **Take as is** | Field-tested on these boards; NetworkManager is not on the image. |
| `connect-wifi SSID PASS` | **Take with a change** | It puts the password on a command line. V2 writes `wpa_supplicant-wlan0.conf` itself, private, then runs `device-sta-mode`. |
| 204 answers to the phones' sign-in checks | **Take as is** | Keeps phones from fighting the page; the person opens a browser, as now. |
| The new address published early, the `run` counter, the status reply's shape | **Take as is** | It is the fix for the stuck screen. |
| The page's redirect order and its 600 ms / 5 s / 2 s / 80 s timings | **Take as is** | Field-tested. |
| Join rules: 60 s, two wrong keys end it, named failures, hotspot back, nothing saved on failure | **Take as is** | They handle the most common failure. |
| 90 s loss to a recovery hotspot, retry every 2 min only with no phone on it | **Take as is** | `lamp-surround` already has the same numbers. |
| The setup page (React bundle served by nginx) | **Take with a change** | The same Wi-Fi step and progress screens, served by V2 from the same paths with the same API shapes; the agent, channel and MQTT parts removed; a name and a time zone added. |
| Credentials only through the account page's URL | **Decided: the user's key, as in V1** | The owner, 2026-10-16 16:13 (#806, relayed by Leo; it replaces the first builder's 12:36): the key is the user's, and the factory has none to write. The account page (`/setup?llm_api_key=…&device_id=…`) or the app posts it down in `POST /api/device/setup`, and setup writes it to the voice's credential and the Policy's `jev.key`, root's, 0600, never logged or echoed. `install` still copies V1's key on a lamp that ran V1 (#1102). A reset wipes it with the rest of the person's data. |
| Admin password = sticker suffix | **Take as is** | The person has the sticker; refuse to start if the hardware ID is unreadable. |
| `set_up_completed` | **Take with a change** | One marker of V2's own, written only after the voice is healthy (not an agent); `lampos.target` starts only when it exists. |
| Agent runtime, channels | **Leave** | V2 has none of them. |
| The backend's link: the MQTT four (`mqtt_endpoint`, `mqtt_port`, `mqtt_username`, `mqtt_password`), `fa_channel`, `fd_channel`, `device_id`, `llm_base_url` | **Keep** (17 Oct, #1396) | The Autonomous app sends them in the setup request (`ecm-sds-mobile` v3). They are kept so a lamp set up now can come online in the app later (`lamp-cloud`, #1396) without being set up again. `lamp-cloud` reads them when it is on (`LAMP_CLOUD=on`, `cloud.md`, #1418): the MQTT link and the backend's ping. |
| Name and time zone | **Take with a change** | V1 does not ask during setup. V2 asks for both; the zone is written the way `timezone.go` does it. |
| Reset button: line 100, 5 s, a spoken notice, the wipe, a reboot | **Take with a change** | Same button and hold. V2 wipes its own stores (below) and the Wi-Fi configuration, then restarts into setup. |
| MQTT soft reset | **Leave** | Cloud only, and it keeps the Wi-Fi. |
| Running V1's whole `os-server` for setup | **Leave** | After Wi-Fi it waits up to 120 s for an agent runtime and drives the light through HAL; neither exists beside lampOS, and both would need V1 changes. |

## What setup must write for lampOS

What lamp-4ace's units read today (`deploy/lamp-4ace/`):

| Item | Today | What setup writes |
|---|---|---|
| Wi-Fi | V1's `wpa_supplicant-wlan0.conf` | The same file, mode 0600. |
| The voice's service | `/etc/lampOS/voice/provider.json`, which names a credential file | The user's key, in that file and in the Policy's `/etc/lampOS/policy/jev.key` (#806, 16:13); `provider.json` comes with `install`. |
| Update trust | `/etc/lampOS/update/key.pub` | Nothing: a trust root belongs in the image, not on a page anyone near the lamp can open. |
| Time zone | Schedule takes `--zone Asia/Ho_Chi_Minh`; Memory, Expression and the policy take `--utc-offset-minutes 420` | `/etc/localtime` and `/etc/timezone`, and `/etc/lampOS/zone.env` again (#820). The product's memory and Expression take `--zone system`, which follows `/etc/localtime` across daylight saving and without a restart; schedule (`LAMP_ZONE`) and skills (`LAMP_UTC_OFFSET_MINUTES`) read `zone.env`. A zone the parts' own zone data lacks leaves `zone.env` as install wrote it, since schedule would not start on it. The voice reads `/etc/localtime` itself. |
| The person's name | Read by the voice with `LAMP_PERSON_NAME` (the product unit sets it, #806): told to the model after its character, and the hello's name when no face is known (`crates/voice/src/person.rs`). The instruction says the lamp belongs to the person who set it up and gives that name as the answer to "who owns this lamp?" when the name is available to the turn. If the file is absent, the character tells the model to say it cannot verify the owner rather than claiming it has none. With `LAMP_PERSON_NAME_OWNER_ONLY=on` (#2602, off unless set) it is not in the character: a line is told it only when it is the owner's own known face (the face kept under the typed name, alone in view); never nobody in view, an unknown face, another known face or two faces. Just after setup no face is kept yet, so the owner facing the lamp is an unknown face and is not told their own name until they ask the lamp to remember their face, under the name they typed | A private file, one line: `/etc/lampOS/person/name`. |
| The owner's language (#4473) | Read by the voice at each start with `LAMP_LANGUAGE_FILE` (the product unit sets it): `en`, `vi`, `ja`, `zh` or `ko`; no file, empty, `en` or unknown is English, as before ([second-voice.md](second-voice.md), "The owner's language"). Written by setup when the request names one (below) and by the app's `language.set` ("Changed after setup"); `lamp-cloud` reads it back as `system.info`'s `host.language` | `/etc/lampOS/setup/language` (`language` in the settings), one line, root's, 0644. A reset keeps it, as the zone. |
| The backend's link | `lamp-cloud`, on in the product (`LAMP_CLOUD=on` in its unit, #1418, #1815) | `/etc/lampOS/setup/backend.json` (`backend` in the settings), root's, 0600, written after the zone and before the key. It holds the fields above when the request carries a whole link: an endpoint, a port (a number, or digits), both channels, each field at most 512 bytes with no space or control character (the password may hold spaces). A missing or broken link writes nothing; when the request named one (an `mqtt_endpoint`) the journal gets one line naming the fields that were missing or not valid, never their values. The device's id, the endpoint, the port and both channels are needed; the base, the username and the password may be empty. Neither a broken link nor one that cannot be written fails setup: the lamp joins the Wi-Fi and talks, and only its place in the app waits. The password never appears in a debug line, and a crashed write's temporary file is removed at start. |
| The person's answer on health and crash reports | `lamp-cloud`, with `LAMP_REPORTS=on` (#1683, `cloud.md`, "Health and crash reports") | `/etc/lampOS/setup/reports.json` (`reports` in the settings), root's, **0644** so `lamp-cloud` (as `lampos`) reads it where it is: `{"share": bool, "asked": 1, "at": "<RFC 3339 UTC>"}`, written after the backend's link and before the key, at every setup, a no included. `share` is true only for the JSON `true` in the request's `share_reports`; absent, `false`, `null`, `"true"`, `1` or anything else is no, so V1's page, this crate's own page and an app that doesn't ask all mean no. `asked` is the question's wording, so a later wording asking for more starts at no. Best effort: a write that fails is a no and never a failed setup. |

## The person's data, which reset wipes

From lamp-4ace's units: memory `/var/lib/lampos-memory`, faces
`/var/lib/lampos-eyes/people.json`, its exact interrupted-write siblings
`people.json.new` and `people.json.repair.new` in the same directory, and
what the eyes set aside beside it
(`/var/lib/lampos-eyes/set-aside`, #1505), tasks `/var/lib/lampos-tasks/store` (and its
targets), schedule `/var/lib/lampos-schedule/store`, voice sessions
`/var/lib/lampOS/voice/sessions`, reminder delivery
`/var/lib/lampOS/voice/schedule-delivery`, the conversation thread
`/run/lampos-voice/thread.json`, the voice's first-output log
`/var/lib/lampOS/voice/first-output`, what the voice has told of tasks
`/var/lib/lampOS/voice/tasks-told` (its default, `LAMP_TASKS_TOLD`), the
near rule's bar measured at setup `/var/lib/lampOS/voice/near-call.json`
(#753), the
skills made by voice and kept unconfirmed `/var/lib/lampOS/voice/made-skills`
(`LAMP_MAKE_SKILL`, #2011), the
person's light kept across a restart `/var/lib/lampOS/light` (emptied)
(`LAMP_LIGHT_KEEP`), the
voice's software mute and sleep from the head (`/var/lib/lampOS/voice/mute`,
`--controls-mute`, #711), the
policy's log `/var/lib/lampos-policy`, the person's parts of Harness under
the tasks part's home (`/var/lib/lampos-tasks/.harness/` `auth`,
`connections`, `computer-id`, `cli/data`, `agent-messages`, `hook-routes`
and `logs`: the sign-in, the pairings and what they left; not `.harness`
whole, which is Harness itself, its CLI and its own Node, and not the
Harness service `install` set up, #934; Nine on #1016), the user's key
(`/etc/lampOS/voice/credential` and `/etc/lampOS/policy/jev.key`, #806,
16:13), the name, the lamp's name in the app given after setup (`/etc/lampOS/setup/display-name`, #1959), the backend's link (`/etc/lampOS/setup/backend.json`, with the broker's password, #1396) and `lamp-cloud`'s rotated login (`/var/lib/lampos-cloud`, the same), the answer on reports (`/etc/lampOS/setup/reports.json`) and `lamp-cloud`'s later record of it and its queue of reports (in `/var/lib/lampos-cloud` too, #1683), the Wi-Fi configuration and the setup marker. Kept: the
software, its releases, the update key, the touch calibration and other
per-lamp facts, and physical privacy.

Onboarding uses `/var/lib/lampOS/voice/onboarding/progress.json`; the privacy
checkpoint and transient handoff files share its root-owned 0700 directory.
The room sounds yes (#1722 item 1) is `/var/lib/lampOS/consent/room-sounds.json`,
0640 root:lampos so lamp-ears can read it; both reset plans wipe that file.
The product unit names that canonical file without enabling `LAMP_ONBOARDING`.
Both reset plans wipe the entire directory's contents, including pending writes,
temporary files and mailbox descendants, retaining the empty private container.
The installed core skill in `/var/lib/lampOS/skills/onboarding` is kept.
A known older deployment using another progress path requires an explicit
migration; arbitrary legacy environment paths are not imported, and old progress
alone is not proof that the privacy exercise was completed.

## The reset (on for the product)

`crates/setup` builds `lamp-setup`. Its first slice is the reset. The safe
shutdown (`lamp-setup shutdown`, #3697) reuses its stop and its own unit, and is
shutdown.md.

```sh
lamp-setup check --plan PLAN.json
lamp-setup resume-reset --plan PLAN.json --enable      # at start, before the lamp
lamp-setup watch-reset --plan PLAN.json --bus-root /run/lampos-bus --enable
```

Controls already turns the reset button (chip 0 line 100) held 5 s or more and
then released into one `controls/touch` event, `source: reset`,
`gesture: hold`, intent `factory_reset` (`docs/controls.md`; it needs the
`--touch` path with a `reset` line in the pins file). The product's
`lamp-controls.service` serves `--touch`, and its `controls-pins.json` has
`reset` on line 100, active low, pulled up (El on #705, 2026-10-16: before,
a product lamp's reset button did nothing). Install brings both to a lamp
whose pins are still what install wrote; a running `lamp-controls` keeps
its old command line until it starts again (a reboot, or `systemctl restart
lamp-controls`). A lamp whose pins were changed by hand keeps them, and
without a `reset` line its button stays dead; install's only word for it is
"kept …: this lamp's own" (Nine on #1167). `watch-reset` acts on the
first such event that is valid, comes from an **enabled** Controls (a shadow
Controls never resets a lamp), is on this boot's clock and is less than 500 ms
old. It does not reset in its own process: it is part of `lampos.target`, which
the keeper's stop ends, so it hands the reset to a unit of its own with the
plan's `detach` (`systemd-run --no-block --collect --unit=lamp-setup-wipe …
lamp-setup reset-now`). That unit, in this order:

1. it writes a journal, durably (temporary file, sync, rename, directory sync);
2. it removes the setup marker, so that from here any restart comes back into
   setup, never into the lamp with half its data;
3. it runs the plan's stop command, the keeper's `lamp-keeper stop lamp`
   (#767): the voice pauses, `lampos.target` stops, the body folds the lamp.
   A plan's `systemctl stop` of a target is run with every part of the
   target named too (its `ConsistsOf`, and theirs for a part that is a
   target; `reset::stop_and_wait`), so it returns only once each part has
   stopped: the target alone returns as soon as its own job is done
   (systemd 252: 0.04 s, a part's write on stop 3.1 s later), and on
   lamp-29e7 on 26 Oct Voice's onboarding, written as it stopped, outlived
   the wipe, so the customer got no first five minutes. Then every part must
   read `inactive` (or `failed`), and the parts read again must be the same.
   A stop of `lampos.target` also stops the product's writers of wiped
   paths outside it (`reset::WRITERS`): `lamp-setup.service`, which serves
   for 600 s after setup, its recovery, the loaded `lamp-setup-change@`,
   `-keys@` and `-wifi@` instances, `lamp-skills-manage@` instances,
   `lamp-records`, the updater with its apply, and the boot's Wi-Fi
   restore. What starts them (`reset::STARTERS`: the helpers' and the
   skills' sockets, the records' timer, the updater's way back, its timer
   and a worker already running, which can restart the updater for a pending
   self-switch, and the keeper's restart socket) is stopped first, so none
   starts after they are listed, and they
   are read stopped too (reviewer-three, the reviewer-lead and Second on
   #3579). Only units whose LOAD column says `loaded` are named: systemd
   refuses to stop a unit it does not have. Parts that cannot be read, a part not stopped, or one that
   joined during the stop are as a stop that fails;
4. it wipes each path in the plan: a file is removed, a directory keeps itself
   (systemd units name some of them and would not start without them) and
   loses everything in it. It walks by handle, one component at a time from
   `/`, following no link anywhere: a part that swaps a directory for a link
   mid-wipe makes the wipe fail instead of reaching outside the plan. A
   wiped `wpa_supplicant-wlan0.conf` goes with the copies Wi-Fi keeps beside
   it (`.conf.lamp-previous`, the old network a change or the boot's
   `lamp-setup-wifi-restore` puts back, and setup's `.conf.lamp-setup` and
   `.conf.lamp-setup-tmp`), matched by name so plans installed before them
   wipe them too, and `wifi-restore` puts no old network back while a reset
   is journalled (Third on #3579). Every wiped path's staging copy from
   setup (`.lamp-setup-tmp`, left by a write cut mid-way) goes with it;
5. it removes the journal, syncs, and **starts setup again in place, with no
   reboot** (#4500), on a product lamp: one whose stop is `systemctl stop
   lampos.target` and which has `lamp-setup.service`. It runs `systemctl
   start` on what it stopped that a boot starts outside the target and was
   running (`reset::STARTERS`, but not the updater's way back, whose timer
   starts it, and `lamp-update.service`), and `lamp-setup.service`
   (`reset::stop_and_wait_keeping`). First it empties what a reboot
   emptied by being memory, `reset::VOLATILE`: every folder a part keeps in
   `/run` past its stop, the tmpfiles' `/run/lampos-bus` (the bus's latest
   messages, which can say a person's name or words) and
   `/run/lampos-photos`, and the voice's and the records' own folders
   (`RuntimeDirectoryPreserve=yes`: the thread's set-aside words, the room).
   Every other part's `RuntimeDirectory=` goes as its unit stops. If they
   can't be emptied it reboots instead. Then, best effort, it forgets what
   else a reboot dropped from memory (`reset::FORGET`): `wpa_cli -i wlan0
   disconnect` and `remove_network all`, so the lamp is off the old owner's
   network before setup's hotspot takes the radio; and `journalctl
   --rotate` then `--vacuum-time=1s`, since the product's journal is a RAM
   disk that names the networks joined. Each is killed after 10 s
   (`QUERY_TIMEOUT`, as setup's other calls of them), so a stuck journald
   never holds the start back. A lamp without those tools still starts.
   Any that failed are named in one line after the vacuum, `reset: not
   forgotten, going on: …`, so the vacuum doesn't take it; the start's own
   line, `reset: started in place, no reboot: …`, comes next. **Not emptied:** `/tmp`, which a reboot emptied too.
   The product's lampOS parts write none of the person's words there, but
   `lamp-tasks` runs agents' tools, so its unit's `PrivateTmp=` is the
   tasks owner's to settle (#4513). With the marker gone, setup serves:
   its start brings up the hotspot from the station live, as Wi-Fi's
   recovery does. Every part of `lampos.target` still waits for the marker,
   and `lampos-setup-done.path` starts the target once setup writes it
   again, so the lamp comes up as on a first boot. **Why:** the Orange Pi 4
   Pro's A733 hung in its boot firmware on a warm reset one time in three
   (lamp-cb94, 30 Oct, stable 8670), and a pinhole reset that rebooted left
   a lamp dark, with no ring and no hotspot, that looked dead. A keeper's
   plan, or a lamp without setup's unit, reboots as before. If the wipe
   fails, or the start in place fails, it reboots, and if the reboot
   command fails it forces one: a lamp left dark with no marker would wait
   for a hand. What a person is told after a reset, for the user guide:
   "After a factory reset, if the lamp's Wi-Fi doesn't appear within a
   minute, unplug it and plug it back in."

Read-only reset queries retain at most 64 KiB of stdout and have five seconds
per query, matching the updater's setup-unit query allowance. A timeout,
oversized output or unsuccessful command follows the existing failed-stop
path: keep the journal and personal data, and reboot into setup. The query
runner has null stdin and keeps stderr out of the reset response. It owns
its child process group and cleans it on errors, including a descendant
holding stdout after the leader exits. An unsuccessful exit also stops
descendants that closed output, before the leader is reaped. It does not
claim to stop descendants that detach or close stdout before successful
completion. Stop commands keep
their existing allowances: this query deadline does not shorten Body folding
or Wi-Fi rollback, and is not an overall reset or hard kernel-I/O deadline.

With the keeper's `--setup-marker` (the fifth builder's, agreed on #702), a
lamp with no marker starts only `lamp-setup.service`. So if the stop fails,
`lamp-setup` reboots at once without wiping; if the power goes during the
wipe, the lamp also comes back with no marker and a journal. Either way
`resume-reset`, the first thing `lamp-setup.service` runs, finishes the wipe
with nothing of the lamp running, without a second reboot. A power cut between
the journal and the marker's removal leaves both, so the lamp starts; then
`watch-reset` finds the journal at its start and carries on with the reset. A plan is refused unless every path is absolute and
plain (no `.` or `..`, read from the text itself), at least two levels below a
top directory, and outside the software (`/opt/lampOS`), the update trust
(`/etc/lampOS/update`) and the system (`/usr`, `/boot`, `/home`, …); the
journal must not be inside a wiped path. With `--enable`, `watch-reset`
refuses a plan without `detach` at its start: its unit is read-only but the
bus (#3710), so a reset run inside it would wipe nothing; every shipped plan
hands the reset to `systemd-run`. Without `--enable`, `watch-reset`
only logs a valid reset and `resume-reset` only says whether one is unfinished.
`crates/setup/fixtures/reset-plan-example.json` is the plan for lamp-4ace's
paths; the name and Wi-Fi entries are where setup writes them, and the voice's
key is not in it.

V1 says "Factory reset starting" before it reboots; V2 does not yet speak: the
voice is among the parts stopped. What the person sees is the lamp folding and
going dark, then the hotspot coming back with the setup light (below). Where `resume-reset` and
`watch-reset` run in the boot order, and with what rights, waits on the
keeper's owner (the fifth builder, asked on #702).

`serve.json` is read one version ahead (#1680, `docs/updates.md`): a field
this build doesn't know is dropped and named in the journal. The reset plan
follows the rule agreed on #2060, so a reset is all or nothing:

- **Anything to wipe is a plain absolute path's text in `wipe`**, which every
  build wipes. A newer plan's extra path is wiped here too, and is still
  checked against what is never wiped (`PROTECTED`).
- **Keeping something back goes only in `keep_back: [...]`.** This build
  knows no `keep_back` entry yet, so each one is dropped with a journal line
  and what it would keep is wiped: the privacy-safe side, a pairing lost at
  worst. A `keep_back` that isn't a list is refused as a bad value.
- **A `wipe` entry that isn't a path's text, or any other field this build
  doesn't know, refuses the reset** (`reset::Refused::Ahead`): nothing is
  journalled, the marker stays and nothing is wiped. `reset-now`,
  `resume-reset` and `check` exit 2 with the reason in the journal.
  `watch-reset` keeps watching instead: each 5 s hold is logged as refused and
  the person is told "Nothing was wiped. Update me, then reset again." through
  the voice's say service (skill `reset`, `docs/speaking-first.md`). That is
  the reset's only line to the person: words for the voice's model, so said
  only while the lamp has a network and `LAMP_SAY=on`, with no recording.
- A bad known value, an empty `wipe`, or JSON that doesn't read is still
  refused (`reset::Refused::Bad`), as before.

Host tests (`crates/setup/tests/it/reset.rs`): the order of the steps, a failed
stop wiping nothing, a wipe that fails halfway keeping the journal and marker
until `resume-reset` finishes, a start with no journal touching nothing,
refused plans, every reason an event is not a reset, the watcher on a real bus
acting once and only logging with the switch off, the watcher of a refused
plan telling each hold and wiping nothing, a plan one version ahead (an
unknown `wipe` kind or field refused with nothing touched, an extra path
wiped, an unknown `keep_back` entry dropped and wiped), the command line, and the
example plan covering every store of the person's listed above.

## Joining the home's Wi-Fi (served by the setup server, on for the product)

`lamp_setup::wifi` is V1's join (`system/network/service.go`, `SetupNetwork`;
`setup_failure.go`) in Rust, with V1's numbers: up to 60 s for the named
network **and** the Internet (ping `8.8.8.8`, else, as V1's `tlsReach`, a TLS
handshake with the voice's service), checked every second; on every third
check, two keys rejected by the router end it early as `wrong_password`. A
failure is named as V1 names it (`wrong_password`, `association_rejected`,
`ssid_not_found`, `no_dhcp`, `no_internet`, `ssid_too_long`,
`wifi_connect_error`, `unknown`), from
wpa_supplicant's journal since the join began and `wpa_cli status`, with key
evidence first. The radio is switched by the image's own `device-sta-mode` and
`device-ap-mode`.

Three changes from V1. The network's name and password are written to
`wpa_supplicant-wlan0.conf` by this code (private, synced, renamed into place),
not passed to `connect-wifi` on a command line; a name or password holding a
line break, a password outside 8–63 printable ASCII characters, or a name over
32 bytes is refused before anything is written (V1 would have written a line
break into the file). The name is written in hex and the password raw between
quotes: wpa_supplicant takes everything between the first and last quote as it
is, so V1's `\"` and `\\` escapes made a name or password holding `"` or `\`
never match. A password holding `"` is written as its 64-hex PSK instead
(`wifi::psk`, as `wpa_passphrase` derives it): wpa_supplicant 2.10 ends a line
at a `#` after a closing quote, so `pa"ss#word` would be cut (#2617's P3 7).
And a failed join puts the previous file back before the hotspot
returns, so nothing of it is kept. `wpa_cli status` prints the joined network's
name with C escapes ("Nhà Lan" as `Nh\xc3\xa0 Lan`); it is decoded before it is
compared with the name asked for. V1's eight failure cases, from its
own captured wpa_supplicant lines, are tests here (`crates/setup/tests/it/wifi.rs`),
with the join's timing against a fake lamp. One rejected key is not the early
stop, since a router can refuse a right password's first handshake: the join
keeps waiting, a join at 20 s is kept, and one that never joins gets the whole
minute, then `wrong_password` (#4348).

**The TLS reach.** A TCP connection alone is not the Internet: anything that
accepts and stays silent, or answers in plain text, would pass. So, as V1, the
handshake must complete, and the name's lookup, the connection and the
handshake together take at most 3 s (V1's `cloudProbeTimeout`). The lookup
runs on a thread of its own, since the system's has no time limit and cannot
be stopped; one that has not come back is waited on by the next check, never
started twice, and the whole service shares that one (`LookupSlot::process`),
so a lookup that outlived a failed attempt is the next attempt's (#845). As in
V1, the certificate is not checked: the probe sends nothing, and right after
the join the lamp's clock can make every certificate look not yet valid. The
handshake's own signature is still checked, as V1's Go does. Tests: a
listener that never speaks TLS and one that answers in plain text are not the
Internet, a real handshake on this machine is, a lookup that never answers
holds no check past its time, and three attempts in a row start one lookup.

## The setup server (on for the product)

Its `GET /api/device/setup/status` answers V1's fields, plus one of its own: `"generation": "lamp-v2"`. The backend keeps one device type, `lamp`, for both generations, so the Autonomous app sets both up as one Lamp product and learns which Lamp it set up from this field; `lamp-cloud` says the same on every MQTT reply (#1396, decided 19 Oct, replacing the two products decided on 17 Oct).

It also says `"share_reports"`, read only: true only when the last setup kept the person's yes to health and crash reports (#1683), so the app can show it after a reconnect. The app changes it later through `lamp-cloud` (`reports.set`, `cloud.md`), never here.

And `"takes_own_keys"`, always said: true only when this setup takes a person's own keys (`LAMP_OWN_KEYS=on`, `own-keys.md`; never while recovering), so the app asks its own-keys question only on a lamp that will use the answer. An older lamp has no such field, which the app reads as false (#2094).

```sh
lamp-setup serve --config SERVE.json --plan PLAN.json --enable
```

Without `--enable` it only reads its config, says `{"enabled": false,
"reason": "--enable was not set"}` and exits. The product's
`lamp-setup.service` passes it (`deploy/product/lamp-setup.service`), so on a
product lamp the server, and the Wi-Fi join it serves, are on.

The keeper starts it, as `lamp-setup.service`, only when the setup marker is
missing (#702). In order, it finishes an interrupted reset, refuses to run on
a lamp that is already set up, raises the hotspot with the image's
`device-ap-mode` (as V1's `os-server` does at a first boot), runs the
configuration's `free_port` and `web_start` commands, reads the lamp's name,
and answers V1's setup API with V1's shapes and wrapper
(`{"status","data","message"}`). The name is read only after the hotspot: on a
fresh image `device-ap-mode` is what renames the host `lamp-xxxx`, and a name
read before it would send the page to the wrong `.local` address.
`device-ap-mode` is given 4 minutes (`wifi::HOTSPOT_TIMEOUT`) before it is
killed with its children: it restarts hostapd up to twice, each up to systemd's
90 s, so the radio's 45 s would cut a slow hostapd and leave no hotspot
(#2617's P3 6). `device-sta-mode` keeps 45 s (`RADIO_TIMEOUT`): it starts no
hostapd, sleeps about 7 s of its own, and the Wi-Fi change's bounds are built
on it. The routes:

| Route | What it does |
|---|---|
| `GET /api/device/setup/status` | V1's `phase` (`idle`/`connecting`/`connected`/`failed`), `lan_ip`, `error`, `mac` (the kernel's host name, `lamp-xxxx`, which avahi announces), `run`, `set_up_completed`, `runtime_phase`; V2's own `generation`, `share_reports`, `takes_own_keys` and, when keys were given, `own_keys`. |
| `GET /api/network` | `iw dev wlan0 scan`, parsed as V1 does, in V1's `domain.Network` fields. |
| `POST /api/device/setup` | V1's `SetupRequest`; its other fields are accepted and ignored. Checked before anything is touched: the network's name and password as for the join; `llm_api_key`, the user's voice key (at most 512 printable characters; none keeps the key already there, as on a lamp that ran V1, but a lamp missing either key file is refused with "the lamp needs its voice key", since it would have Wi-Fi and no voice); V2's own `name` (up to 64 characters) and `timezone` (an IANA name whose file, links followed, is a `TZif` zone file inside the zone data; one the lamp lacks is skipped, not a reason to refuse setup); the optional `language` (#4473: exactly `en`, `vi`, `ja`, `zh` or `ko`, trimmed, as `language.set` checks it; absent writes nothing, which is English, and any other value is skipped with a journal line, not a reason to refuse setup, as an unknown zone). Replies at once, then, 2 s later as in V1, joins; one run at a time; refused once the lamp is set up. |

While joining it reads the station address every second and publishes it as
`lan_ip`, V1's fix for the page that lost the lamp, and once more when the join
is done. That watcher ends the moment the join does, so the save and "set up"
never wait out its second. A new run clears the last run's address at once, so V1's page cannot
follow an old one. Once joined, it saves the name, the time zone
(`/etc/localtime` linked to the zone, `/etc/timezone` naming it, as V1's
`timezone.go`, and `/etc/lampOS/zone.env` for schedule and skills, #820), the owner's language when the request named one (`/etc/lampOS/setup/language`, 644, #4473) and then the user's key (to the voice's credential and the
Policy's `jev.key`, both written before either is renamed into place, never
logged, never in a reply), each private and synced, and only then the
marker; the keeper starts the lamp. If anything cannot be saved, there is no
marker, the previous Wi-Fi file goes back, and the hotspot comes back; what
was saved before the failure stays, and the next run writes it again. The
key is saved last, so a name or zone that fails leaves the old key as it
was. A temporary file a crash left behind is removed at the next start.
One network scan runs at a time, and its list answers for 5 s. The radio may
refuse to scan while it is the hotspot (V1 lists networks in station mode
only; lamp-06c1's phone said "Could not look for networks" until it asked a
few times), so setup looks once before it raises the hotspot (at most 4 s,
its count and time in the log, so a stuck `iw` never holds the hotspot back),
scans up to three times in the hotspot's first 10 s before anyone asks, and
answers a refused scan, or one that comes back empty as some radios' does as
the hotspot, with the last list a scan gave (`http::list_networks`); only
with no list seen at all is a refusal the page's error. Every scan is killed
after 10 s, so none holds the one-at-a-time scan for long. After setup it goes on
answering for 10 minutes, so the page can follow the lamp onto the home network
and see it done.

**V1's page, through V1's nginx** (the first builder's answer to question 3,
read on lamp-4ace: nginx 1.22.1 installed but stopped; V1's built page still in
`/usr/share/nginx/html/setup`). `crates/setup/fixtures/serve-behind-v1-nginx.json`
answers on `127.0.0.1:5000`, where `os-server` did, starts nginx once the
hotspot is up and stops it when serving ends, 10 minutes after setup is done
(the unit's `ExecStopPost` stops it however serving ends). `crates/setup/fixtures/nginx-setup.conf` is V1's site with what
a V2 lamp has not removed (`hal`, `openclaw`, and V1's shell, exec, openapi,
buddy and harness routes): V1's page and cache rules, `/api/` to `lamp-setup`,
and the 204 sign-in answers. It keeps no access log, since the account page's
link carries the user's key in its URL and no reset wipes nginx's log. A home
lamp's image (#811) must carry nginx, that
config as its default site, and V1's page bundle.

After the join, `runtime_phase` stays `preparing`. V1's page then keeps its
"Preparing your robot's voice…" screen; at `ready` it would reload into its own
settings, which need V1's server. The lamp's greeting, as in V1, is how the
person learns it is done. V1's page takes the key from the account page's URL
(`?llm_api_key=…&device_id=…`) or, with `?debug=true`, in its AI step.
`serve-own-page.json` answers on port 80 with
`page: true`: this crate's own page (`src/page.html`, one step: network,
password, the voice's key unless the account page passed one, the person's
name; the time zone comes from the phone), and 204 to the phones' sign-in
checks, as V1's nginx does. `device-ap-mode` restarts nginx whether or not it
is used, and nginx would hold port 80, so this configuration's `free_port`
(`systemctl stop nginx`) runs after the hotspot is up and before answering; on
a lamp without nginx it fails, is logged, and setup goes on. A hotspot raised
again after a failed join restarts nginx once more, which then finds port 80
taken by `lamp-setup` (read from the script, not run on a lamp). Its progress
screen keeps V1's timings: status every 600 ms, the hotspot lost after 5 s of
silence, then the new address first and `<name>.local` every 2 s, and a
failure shown at 80 s. Each fetch
gives up after 2 s, so a request to a hotspot that has just gone cannot hold
those checks back.

Host tests (`crates/setup/tests/it/serve.rs`): every refused request, a run that
joins and saves in order with the address published while joining, a wrong
password that saves nothing, a save that fails and leaves no marker, the
private files and the zone link, V1's routes and shapes, one run at a time, no
second setup, the page and the 204s only with `page`, the scan with iw's
escapes, the name read after a hotspot that renames the lamp, and the start's
order with both configurations: own page, hotspot then nginx stopped (a failed
stop only logged); behind V1's nginx, hotspot then nginx started, never
stopped. No phone, router or radio was used.

V1's page also asks `/api/network/check-internet`, `/api/network/current`,
`/api/system/info`, `/api/device/config` and the voice lists. `lamp-setup` does
not answer them (404); V1's page has a fallback for each failure (read in its
source: not online, no current network, no capabilities so no voice or face
step, an empty form), so its one Wi-Fi step still works. A lamp run must
confirm it.

## The setup light (on for the product)

From the reset to the lamp's first words the voice is not running, so
without this the lamp is dark and silent, and a person would think it broke
(the first builder, #806, 17 Oct 14:44). With `"light": true` in setup's
settings, `lamp-setup serve` draws a light on the ring itself, with no voice
and no Controls. The product's `serve-own-page.json` has it on; left out, it
is off (`serve-behind-v1-nginx.json`).

| Setup's phase | The ring |
|---|---|
| `idle`: the hotspot is up, waiting for the person | soft white (24, 20, 14 of 255), breathing once every 6 s |
| `connecting`: joining the home's Wi-Fi and saving | soft blue (4, 12, 24), breathing once every 3 s, busy but not hurried |
| `failed`: the join failed, the hotspot is back | amber (24, 8, 0), breathing once every 6 s, until the next try |
| `connected`: set up | dark, and the ring is the lamp's |

The whole ring is one colour. A breath goes from 35 hundredths of the colour
to all of it and back, never dark, so it reads as on; no channel is ever over
24, the product voice's own ring ceiling (`--ring-channel-ceiling 24`), so
setup is no brighter than the lamp after it. It fades in over 1.5 s, one look
turns into the next over 0.8 s, and the breath runs on through a change, so
nothing jumps or blinks: a calm thing breathing, quicker while it works.

It starts once `serve` knows the lamp is not set up, before the network look
and the hotspot, so the light comes up about as soon as the service does.
It follows the status the page reads, one frame every 50 ms, so it begins to
change within 50 ms of the phase. Setup turns `connecting` the moment it
accepts Setup, so blue begins within 50 ms of the press; the join itself
starts 2 s later, as in V1. Just before the marker is written, the light
fades out over 0.6 s, sends black and closes the ring device (waiting at
most 2 s, and saying so if that was not enough): the marker starts the lamp,
and the voice's ring worker must find the device free, or the voice runs
with no ring. From then on the ring is opened only while a look is wanted,
so a retry that comes due during the fade out never takes the device after
the marker. If saving then fails, the hotspot comes back and so does the
ring, amber, and the next press shows blue at once. When serving ends,
after setup or on a stop (`systemctl stop`, a shutdown: SIGTERM ends serving
the same way), the light ends with black. The stop is listened for before
the light starts: one during the first network look ends setup before the
hotspot, and one while the hotspot comes up ends serving as soon as it has
begun, so a stop in the first seconds also ends in black (the reviewer on
#1325). systemd's stop reaches the whole unit, so it also ends the hotspot
script; setup then says it was asked to stop, with the script's error,
and exits cleanly, and the unit does not read failed (the reviewer on
#1336 and #1341). That choice is `serve::start_unless_stopped`, tested in
`tests/it/serve.rs` (Fourth on #1341).

It is painted by `lamp_setup::light` from a thread of `lamp-setup`, through
`lamp_ring::setup::SetupRing`, a writer like the asleep face's that needs no
permit: it can show only the setup colours over the whole ring, no brighter
than 24, and black. A ring that cannot be opened or written is said once in
the journal and tried again every 5 s; setup goes on either way. Only a
process killed outright (SIGKILL, a crash) cannot send black: its last frame
stays until something writes the ring again (after a crash, `lamp-setup`
itself, 20 s later).

Host tests: `crates/ring/src/setup.rs` (one colour over the ring, never over
24, black first and last); `crates/setup/src/light.rs` (each phase's look,
the fade in, a breath that never goes dark, the change, the fade out, joining
breathing twice as fast); `crates/setup/tests/it/light.rs`, the painter's
thread on a fake ring: white, blue, then black and the ring closed before the
marker; amber again after a save that failed; nothing drawn after the marker
of a run that finished; black at stop; a ring that cannot be opened said once
and letting go waiting for nothing; a ring that failed and is free again
during the fade out not opened after the marker (the reviewer's probe on
#1325; it fails without the fix); the press after a failed save blue at once,
never dark; and with the switch off no ring opened. Not shown on a lamp: the colours, the
brightness in a room and the timing on the ring itself are a lamp check.

The stop-order fixture uses the same frame-stepped `Fast` clock as the
adjacent fade tests. It still waits for white, stops the painter, then
checks that its last frame is black and the ring is closed. The product's
fade durations and the real two-second stop deadline are unchanged.

## A person's own keys (#2094, built, off unless set)

With `LAMP_OWN_KEYS=on`, the setup request may carry a person's own
DeepSeek endpoint and key and Deepgram key (`deepseek_api_key`,
`deepseek_base_url`, `deepgram_api_key`). Each is checked once against its
service after the join and before the marker, and kept only if it works, in
`/etc/lampOS/voice/own/` (root's, 0700, each key 0600), which the reset
wipes. None fails setup: the status's `own_keys` gives each answer. After
setup the app changes them through root's keys helper,
`lamp-setup keys` on `lamp-setup-keys.socket`. All of it is in
[own-keys.md](own-keys.md).

## Changed after setup (#1959, on for the product)

The person changes their name, the lamp's name in the app, the time zone or
the owner's language from the app, without a reset. The app's side is `lamp-cloud`'s
(`LAMP_SETTINGS_CHANGE=on`, `cloud.md`, "Changed after setup"); the writing is
here, in root's helper `lamp-setup change` (`src/change.rs`), so the files are
left as a setup with the new value leaves them. Wi-Fi is #1959's second piece.

**The helper.** systemd holds `/run/lampos-setup-change/change.sock`
(`deploy/product/lamp-setup-change.socket`) and starts
`lamp-setup change --config /etc/lampOS/setup/serve.json --enable` as root for
each connection (`lamp-setup-change@.service`), with the connection as its
standard input and output. It reads one line (at most 4 KiB), answers
`{"done": …}` or `{"refused": why}` on one line, and exits, unless a zone
or the language changed (below). It checks every request itself, whoever
connects: exactly one of `{"person_name": s}`, `{"display_name": s}`,
`{"timezone": s}` or `{"language": s}`.
Without `--enable` every request is refused, "changes after setup are off",
and nothing is written. **Only `lamp-cloud` may ask:** the socket's group lets
a `lampos` process connect, and the helper then takes its peer's credentials
(`SO_PEERCRED`) and refuses any peer outside `lamp-cloud.service`'s cgroup,
before its line is read, with `{"refused": "only the app's link may change the
lamp's settings"}` and nothing changed (`keys::socket_peer_refused`, as the keys
helper does). Run by hand, its standard input not a socket, it takes the line
as before. `LAMP_SETTINGS_CHANGE` decides whether `lamp-cloud` asks. A
refusal never holds the value asked, and the
journal says what changed, never a name ("the person's name was changed (8
characters)"); a zone is said.

**The app's restart and shutdown** (#2193, #3697) are asked on the same
helper, as the lines `{"restart":"lamp"}` and `{"shutdown":"lamp"}`
(`lamp_keeper::restart`'s requests, answers and update lock, one copy with
the keeper's own helper). Each takes lamp-update's lock
(`/var/lib/lampos-update/lock`, writable in the unit) without waiting: held
by an update, `{"refused":"an update is being installed: try again once it is
done"}`; one it cannot check is refused too, with why. Otherwise it answers
first, `{"done":{"restarting":true}}` or `{"done":{"shutting_down":true}}`,
and then acts: the restart holds the lock, waits for the voice to rest
between turns (5 s resting, nobody heard, nothing said; at most 45 s, then
all the same) and runs `systemctl restart lampos.target`, never the machine
(this connection's instance is not part of the target, so the restart does
not stop it); the shutdown lets the lock go and starts `lamp-setup shutdown`
(shutdown.md), which takes it again. Which of them `lamp-cloud` asks is its
own switches' (`LAMP_APP_RESTART`, `LAMP_SHUTDOWN`; cloud.md).

| Change | Checked | Written | Who follows, and how |
|---|---|---|---|
| The person's name | trimmed, at most 64 characters, no control character, line or paragraph separator or direction mark (the voice's own rule); `""` removes it | `/etc/lampOS/person/name`, 600 root, as setup writes it; removed for `""` | the voice reads it again at each hello and each new session: nothing restarts. A session already open keeps the old name until it ends |
| The lamp's name in the app | the same, and not empty | `/etc/lampOS/setup/display-name`, 644 root (`display_name` in the settings), one line | `lamp-cloud` reads it at each `system.info` as `host.device_name`. **It never changes what the lamp answers to: the wake word stays "Lamp"** |
| The time zone | an IANA name the lamp's zone data has (`zone_known`, as setup) **and** the parts' bundled zone data has, else refused, so `zone.env` and `/etc/localtime` never disagree | `/etc/localtime`, `/etc/timezone` and `/etc/lampOS/zone.env`, as setup writes them | the voice, memory, Expression and skills follow `/etc/localtime` at each reading. `lamp-schedule` takes `LAMP_ZONE` at start, so after a zone that changed `zone.env` the helper waits for the lamp to be quiet, as an update does before it restarts the voice (`lamp_update::quiet`: resting, nobody heard and nothing said for 45 s), then for no update at work, taking lamp-update's lock and holding it through `systemctl try-restart lamp-schedule.service` (`change::quiet_and_unlocked`): a round puts its build back when a unit it restarted changes process in its watch (`updates.md` step 9), so the zone's restart never falls in one; while an update holds the lock it looks again 10 s later, after the quiet again. The zone's files are written before the wait, so a round running then reads the new `zone.env` at its own restart of `lamp-schedule`, which is the person's zone, with no extra restart in its watch; and a round `serve` starts while the helper holds the lock (a few seconds, as for the app's restart and shutdown) ends "another lamp-update is at work", shown on `update/status` until `serve`'s next round, though nothing failed (Second's and Reviewer-two's P3s on #4490). It waits at most 30 minutes in all; then schedule keeps the old zone until it next starts, and the journal says so. Reminders already set keep the zone they were set in (`schedule.md`) |
| The owner's language (#4473, `language.set`) | exactly `en`, `vi`, `ja`, `zh` or `ko`, else refused, "language must be one of en, vi, ja, zh, ko", with the file untouched. lamp-update's lock is taken first, as the app's restart takes it: held by an update, refused with the restart's words ("an update is being installed: try again once it is done") and nothing written, so no round's watch sees the voice restart; otherwise held through the write and the voice's restart, and let go at once when the language is the same | `/etc/lampOS/setup/language`, 644 root, one line, staged and renamed into place as setup writes it, before the answer, so `host.language` reads it at once | the voice reads it at start (`LAMP_LANGUAGE_FILE`), so after a language that changed the file the helper waits for the voice to rest between turns (5 s resting, nobody heard, nothing said; at most 45 s, then all the same, as the app's restart) and runs `systemctl try-restart lamp-voice.service`. The same language again restarts nothing. `lamp-cloud` keeps running, so `system.info`, `mic.get` and the link stay answered through it |
| The owner's place (#4333, [owner-place.md](owner-place.md); a set sent by `lamp-cloud` only with `LAMP_OWNER_PLACE=on`, a clear whatever it is, #4356) | `{"owner_place": {"name", "lat", "lon", "set_at"}}`: rounded again here to about a kilometre (`lamp_msgs::owner_place::fit`), the name as the person's name's rule, `set_at` ISO 8601 UTC; `{"owner_place": null}` clears it | `/etc/lampOS/person/place`, beside the name, 600 root, one JSON line; removed for `null`, and a clear with none kept is no error. The journal says only that it was set or cleared | the voice reads it again for each line that needs a place: nothing restarts |

Tests (`crates/setup/tests/it/change.rs`): each change's files, modes and reply;
a name removed; a rename to nothing refused; each language written and a
bad one refused with the file untouched, the same one restarting nothing,
and the voice's restart after the 5 s rest; a zone either zone data lacks,
or a path, refused with nothing changed; the same zone again restarting
nothing; the switch off, nothing written; requests that are not exactly one
change, refused without echoing them; the quiet wait (45 s resting, 45 s after
a turn, a conversation that never ends and a voice that says nothing given up
at the bound); the binary on standard input and output with a journal line
holding no name; the product's reset wiping the lamp's name and keeping
the language and the zone, and the units running the helper. Not run on a lamp.

## The Wi-Fi changed after setup (#1959, built, off unless set)

The person gives the lamp a new Wi-Fi from the app (`wifi.set`, `cloud.md`),
without a reset. Off unless `lamp-cloud` has `LAMP_SETTINGS_WIFI=on` beside
`LAMP_SETTINGS_CHANGE=on`: its own switch, since this is the one change that
can take the lamp off the network the app reaches it on.

**Root's helper, `lamp-setup wifi`** (`crates/setup/src/wifi_change.rs`), on a
socket of its own: `/run/lampos-setup-wifi/wifi.sock` (`lamp-setup-wifi.socket`:
root's, group `lampos`, 0660, `Accept=yes`, **one connection at a time**), each
connection starting `lamp-setup-wifi@.service` as root, with the names'
helper's sandbox (`NoNewPrivileges`, `ProtectSystem=strict`, `ProtectHome`)
less what the radio needs: the network (the broker's check), netlink, and
writes to `/etc` (the Wi-Fi file and V1's `device-sta-mode`, which edits
`/etc/dhcpcd.conf`), `/run` and the real `/tmp` (`wpa_cli`'s reply socket, so
no `PrivateTmp`). The names' helper has `PrivateNetwork=yes` and could not.
**Only `lamp-cloud` may ask, and so confirm:** a peer outside
`lamp-cloud.service`'s cgroup is refused before its line is read, as for the
change helper above. A change is kept only when its broker is reached on the
new network and the app confirms it. One line in,
`{"wifi": {"ssid", "password", "broker": "host:port"}}`, checked with setup's
own `wpa_conf` and its page's words (`serve::refused_words`); one line out at
the end, `{"done": {"ssid"}}` or `{"refused": why}`.

**It never strands the lamp:**
1. The old network's file is kept beside the live one
   (`wpa_supplicant-wlan0.conf.lamp-previous`, 0600) until the new one is
   confirmed. A lamp with no Wi-Fi file is refused, with nothing changed.
2. The new file is written as setup writes it, and V1's `device-sta-mode`
   restarts the radio, as setup's join does.
3. **Proven** = the new name is the one joined, with a station address,
   and a TCP connection to the broker `lamp-cloud` is on is made and closed
   (3 s at most, name lookup included), within 60 s (setup's own bound). Two
   rejected keys stop early, as setup's join does. **Kept** only once the app
   also confirms through it: the helper writes `{"joined": {"ssid",
   "online_within_ms", "confirm_within_ms"}}`, `lamp-cloud` makes its link
   again over the new network, says `wifi.joined` to the app on it and,
   once the broker acks that, says `{"online": true}` within the first
   (60 s, `ONLINE_WAIT`), and only then does the helper's window of 120 s
   (`CONFIRM_WAIT`) start; `lamp-cloud` hands back the app's `wifi.confirm`
   as `{"confirm": true}` within it (`CONFIRM_SLACK`, 500 ms, for the line to
   cross the socket). Without it (the app closed, the phone gone) the lamp
   goes back as if the join had failed (#1959's P1, #2617's P2). Once
   confirmed, `.lamp-previous` is removed and its folder synced; a removal
   that fails is tried 3 times, and then the file is made the new network's
   own, so no later restore can undo a confirmed change (#2617's P3 3).
4. Otherwise the old file goes back (its write tried 3 times a second apart
   when it fails, at once, not left to the stop's restore; #2617's P3 2),
   then `wpa_cli reconfigure` **first**, so
   wpa_supplicant never keeps the new network loaded, then V1's
   `device-sta-mode`. Each command is bounded (`wifi::RADIO_TIMEOUT` 45 s for
   the script, killed with its children; 10 s for `wpa_cli` and
   `journalctl`; a command that ends while a child of its own keeps its
   output open is waited on 1 s more, then its group is killed, #2617's P3
   6), and the reconfigure and the restart are tried up to 3 times
   (`BACK_TRIES`), each try waiting 60 s for the old network: its name the
   one joined (`ssid=` in `wpa_cli status`, against the old file's
   `ssid=`), with an address and the broker, not merely any address (#2617's
   follow-up 1). If all three writes of the old file fail, wpa_supplicant
   runs the old network from memory at once (its control socket:
   `REMOVE_NETWORK all`, `ADD_NETWORK`, the old file's `SET_NETWORK` lines,
   `SELECT_NETWORK`, never `SAVE_CONFIG`, and not `wpa_cli`, so the password
   is in no command line), with no radio restart, which would read the new
   file, up to 3 tries a second apart; `.lamp-previous` stays for the stop's
   restore, which writes it (follow-up 6). A hold that fails part-way may
   have left wpa_supplicant with no network, since its networks are removed
   first, so `wpa_cli reconfigure` follows it at once, and once more if the
   old network never came back: the lamp is then on the live file's
   network, the new one that worked, not on none (#2686's P2). A
   lamp whose old network does not come back either stays on it and keeps
   trying it: **never the hotspot**, since after setup there is no setup page
   behind it.
5. **Restore before any kill** (#1959's P1): `.lamp-previous` is written
   before the switch and removed only once the change has ended. A helper cut
   before that (a crash, `RuntimeMaxSec=25min`, a kill) is followed by the
   unit's `ExecStopPost=lamp-setup wifi-restore`, which puts the old file back
   and has wpa_supplicant re-read it and the radio restarted, the old
   network judged back by its name and an address, all within 90 s
   (`RESTORE_MOST`; a try starts only while its two commands' bounds, 55 s,
   fit). In practice that is **one** radio try, not the way back's three:
   after one try and its wait, 55 s no longer fits. A shutdown is held 90 s
   at most
   (`TimeoutStopSec=2min`, room for a poll past it; follow-up 4).
   `.lamp-previous` is removed only once that has worked; otherwise it stays
   for the boot's restore, the backstop (#2617's P3 1).
   `lamp-setup-wifi-restore.service` (at boot, before `wpa_supplicant@wlan0`
   and `dhcpcd.service`, only when `.lamp-previous` is there) puts the old
   file back after a power cut, so the lamp always boots on the network it
   had. Before dhcpcd too, since V1's `device-sta-mode` drops `nohook
   wpa_supplicant` from `/etc/dhcpcd.conf`, so dhcpcd's hook may start a
   wpa_supplicant of its own on the same file (#2617's P3 4). A
   `.lamp-previous` that cannot be read is an error (`Failed`, the command
   exits non-zero), never "nothing to restore" (#2617's P3 5). Install never
   lists the helper's units without the restore unit, enabled (a test on
   `install`'s list, `crates/update/src/install.rs`).

**Why lamp-0c89 stayed off (#1959's P1, build 4814):** the old file went back,
but the radio's one restart failed or hung, so wpa_supplicant kept running the
new network until a reboot. Nothing tried again. The lost line was silent
because the `wifi_lost` clip is not rendered yet. Now it is said in plain words
when the voice lacks the clip.

**The lamp says what happened**, through the voice's say service (`LAMP_SAY`,
`speaking-first.md`) from the skill `wifi`, never the network's name or
password. The two lines said while online go as `text`, words for the voice's
model: "I'm on the new Wi-Fi now."; "I couldn't join that one, so I've gone
back to the old Wi-Fi.". With neither network back, "I can't reach any Wi-Fi.
If you changed your Wi-Fi's name or password, push a paperclip into the hole
marked reset on the back of my base and hold it for five seconds. That erases
me and starts setup again." at most once a minute, for 10 minutes at most or
until a network is back (#3743). It names the reset, not the main button,
whose 5 s hold only mutes the microphones ([controls.md](controls.md)), and
where it is: the pinhole labelled RESET on the back of the base, beside the
round POWER button, held 5 s with a paperclip or SIM pin (the hardware lead on
#3743). It says the hold erases; the words are kept in one constant,
`wifi_change::SAY_LOST`. A lamp moved over from V1 has no reset pinhole
(`reset: none`, [cloud.md](cloud.md)), so the line's instruction doesn't apply
to it. It is first said only after the old network has not
come back in three 60 s waits, so 3 minutes at least after the lamp left it: a
router's restart (2-3 minutes) never hears it.

**The no-Wi-Fi line is a recording** (the first builder on #1722, 01:56): with
no network the voice cannot reach its model, so this one line is never words
for it. It is recorded once at build time in the lamp's own voice and played
by the voice:

- **Asked** as `{"skill":"wifi","recording":"wifi_lost","priority":7,"expires_in_ms":60000}`.
  The say service takes only the names it knows (`wifi_lost`), and only from
  the Wi-Fi helper: the socket's peer must be root (`SO_PEERCRED`, read on
  Linux), so a skill, under any name, is refused ("recording is asked by the
  Wi-Fi helper only") and cannot say there is no Wi-Fi while the lamp is
  online (`say::Caller`). Its samples are
  played as a skill's sound is (`play_skill_sound`): a turn of the lamp's own,
  on the head cue's speaker path at the recorded level, with the same gates
  (not paused, muted, speaking, listening or owing an answer; 15 s after the
  last turn; one sound in 30 s). Nothing goes to the provider.
- **Where it lives:** the failure notice `wifi_lost` (`failure-notices.md`) in
  `/opt/lampOS/voice/notices`, placed by `lamp-update install` from
  `deploy/product/models.json` (#1844): `manifest.json` names `wifi-lost.en-US.wav`
  with its SHA-256, frames, transcript, voice and approval, and the loader
  refuses anything that does not match. `lamp-voice.service` passes
  `--failure-notice-dir /opt/lampOS/voice/notices --failure-notice-language en-US`.
  It is read once at start; `failure_notice_capability` reports `wifi_lost`.
- **Rendered** by `tools/render-wifi-line.sh PROVIDER.json NEW_FOLDER [production|test] [lost|recovering]`
  (`lost` by default; `recovering` is the recovery hotspot's line, below):
  the voice's own route, model and voice (`gemini-3.8-live`, `Kore`), the line
  as one written turn, as the say service's `text` is
  (`crates/gemini/examples/render_line.rs`), kept as 24 kHz mono 16-bit with
  the silence at either end cut. The example itself hashes it and writes the
  manifest beside it (the kind, the words, the provider file's voice and
  model, the approval, the SHA-256 and the frames); the script only calls it
  and then checks the folder with `lamp-audio notices`. It needs the voice's key in its credential file; it
  never prints it. `test` unless `production` is given, after a person has
  listened to it.
  **The product's copy** is rendered in the second route's voice instead
  (ElevenLabs, with #2300's `render_line_second`), since every lamp now
  speaks with it, and comes with the other three lines from the
  `notices-2026-10-20` release ([failure-notices.md](failure-notices.md),
  "The product's recordings", #1844).
- **Missing:** with no folder, no manifest, or a file that fails a check, the
  voice refuses the ask at once with the loader's reason (`the recording is not
  there: not_provisioned`), nothing is heard, and the helper journals it:
  `lamp-setup: wifi: the recording wifi_lost was not played: refused, ...`. The
  helper waits at most 2 s for the voice's first answer, and a refusal within
  them (the lamp speaking or listening when the voice comes to it) is
  journalled the same way. After them the helper has hung up and counts the
  ask as waiting in the voice: a refusal that comes later is read by nobody
  and **not journalled**. Either way the next try is a minute later.
- **Owed:** the recording itself. No clip has been rendered (it needs the
  voice's key) and none is in `deploy/product/` or `lamp-update install`'s
  list; until it is, the line is silent and journalled.

**The password** is in the request line and the network file, and nowhere
else: no reply, error or journal line holds it, and the journal holds no
network name either, only its length.

Tests (`crates/setup/tests/it/wifi_change.rs`, with a fake network manager): a
network that joins and reaches the broker kept, and the old file gone; the old
file kept while waiting; a wrong password, a network out of range, one with no
address and one without the broker each back on the old network, never the
hotspot; one rejected key kept waiting, then joined and kept, or the whole
bound and back with the wrong password (#4351); an old network that does not come back kept, within 2 min; no Wi-Fi
file refused; the request's checks in setup's words; the password and the names
in no reply or journal line, for every outcome; the switch off, the radio
untouched; the three lines, and the lost line at most once a minute and given
up at its bound; the lost line always the recording, never words for the
model, and the other two always words; a recording not there journalled with
the voice's reason, and nothing said; the units, and the voice's unit loading
the fixed folder. The voice's side (`crates/voice/src/say.rs`,
`coordinator/notices.rs`): a recording asked by name reaches the voice as a
sound, one not provisioned is refused with why, and only known names are taken.
Not run on a lamp.

## On a lamp

For a builder with a lamp other than lamp-4ace (#806 asks for one), from #817's
ARM64 run and the keeper's `--setup-marker` (#816):

1. **Keep a way back.** Copy `/etc/wpa_supplicant/wpa_supplicant-wlan0.conf`,
   `/etc/nginx/sites-enabled/default`, the voice's `provider.json` and
   credential, and every path in the reset plan, which a reset wipes.
2. **Install.** `lamp-setup` under `/opt/lampOS/setup/<sha256>/`; the two units
   from `crates/setup/units/` with `BUILD` replaced; `/etc/lampOS/setup/serve.json`
   from `serve-behind-v1-nginx.json` (the lamp's country) and `/etc/lampOS/setup/reset-plan.json` from
   `reset-plan-example.json` (the lamp's own stores); `nginx-setup.conf` as
   nginx's default site, nginx left disabled; the keeper with
   `--setup-marker /var/lib/lampos-setup/done`.

   **On a lamp put on by `lamp-update install`** ([install.md](install.md),
   "Setup at first boot, without the keeper"), the units come with the
   product and there is no keeper: the reset plan is
   `reset-plan-product.json`, the example stopped by
   `systemctl stop lampos.target` (every part is `PartOf` the target), and
   nothing else changes. `install --setup on` writes both files
   (`SETUP_SETTINGS` in `crates/update/src/install.rs`); without it, both
   are put there by hand. The product's pair is `serve-own-page.json` with
   `reset-plan-product.json`: a test holds them to the same marker, and what
   setup saves of the person, the user's key in both its places and the name,
   is what the reset wipes. Settings that name no key files take the
   product's places. `serve-own-page.json` needs only a phone, and the
   account page's link fills the key.
3. **First boot.** With no marker, reboot. Expect the open hotspot `lamp-xxxx`
   and no lampOS part running. Join it with a phone and open
   `http://192.168.100.1/setup?llm_api_key=<key>&device_id=<lamp>` as the
   account page does (or `http://192.168.100.1/` and type the key in the own
   page). Time, separately: phone joined
   to page shown (target under 5 s); Setup pressed to the hotspot gone; to
   `connected` or a named failure (at most 60 s); marker to the lamp's first
   word. With the product's settings, the setup light: the service's start
   to the ring's first soft white (expect about 2 s, its fade in), white
   while waiting, blue at once after Setup, dark before the marker,
   and the voice's ring worker starting with no "ring startup" fault.
4. **Failures.** A wrong password (expect `wrong_password` and the hotspot back
   by about 40 s on the lamp: on lamp-0c89, 29 Oct, the router's second
   rejection came 26 to 27 s after the join began and the run ended at 38 s,
   in both runs; the tests' 5 s is the fake's clock. From the app, expect up
   to about 80 s, with the phone's rejoin of the hotspot), a network out of
   range (`ssid_not_found` at 60 s), a name with an accent and one with a `"`.
   Each failure must leave the previous Wi-Fi file, and the ring amber until
   the next try.
5. **Reset.** Controls with `--touch` and a `reset` line in its pins file. Hold
   the RESET pinhole 5 s with a paperclip and release: the lamp pauses, folds and
   starts setup again in place with no reboot, its hotspot up within 60 s, and every path in the plan is empty. Once, cut the power during the
   wipe: setup must finish it at the next start.

Most likely wrong on a lamp: what V1's image left of the radio (whether
`device-ap-mode`, dhcpcd and the `wpa_supplicant@wlan0` override still work
after V2 went on); `iw dev wlan0 scan` while the hotspot is up (refused at
times on lamp-06c1, below); V1's page meeting a 404 where this page reading says
it falls back; `systemd-run` and the keeper's path on the image; and the time
from the hotspot going to the page finding the lamp on the home network.

**First run: lamp-06c1, 2026-10-16** (El, with its person and a phone; build
1009, `6d2bea2e1`, from `lamp-update install`, the own page). The hotspot was
up 26 s after the reboot. The phone's first network lists failed ("Could not
look for networks") and "Look again" worked after a few tries (since fixed as
above, not yet run on a lamp). Setup pressed to `connected` and the marker:
14 s (12:40:12 to 12:40:26). The lamp back on the office network: 12:40:48,
22 s after the marker. The engineer's steps it needed are El's list on #705
and #811 (the two settings files, `provider.json`, the voice's sessions
folder, the lock folders' modes).

## A lamp moved from V1 (the generic package, #2937)

A V1 lamp moved to V2's generic lamp-os package (#2163, #2396) gets setup
as a product lamp has it, so a person who changes router can join the new
Wi-Fi without a put-back to V1:

- **The units** (`deploy/units/`): `lamp-setup.service` and
  `lamp-setup-recover.service`, the product's twins with the same commands,
  conditions and hardening; no reset unit (below). A test
  (`the_generic_package_sets_up_and_resets_as_the_product_does`,
  `crates/update/src/unit.rs`) holds them to the product's, and names
  setup's product units a moved lamp does not get yet (the app's Wi-Fi,
  settings and own-key changes after setup, and the marker's path unit).
- **The settings** (`lamp-keeper prepare`, [keeper.md](keeper.md)): where
  missing, `serve-own-page.json` and `reset-plan-product.json`, the pair
  `install --setup on` writes, and, with the plan, setup's marker. A moved
  lamp got the package over its home's Wi-Fi, so it is set up already, and
  the move never opens a hotspot. A lamp with the plan and no marker was
  put back into setup, and prepare leaves it so.
- **No 5 s reset on a moved lamp** (the first builder on #2975, after El's
  P1): the generic controls run without `--touch` and the pins never carry
  `reset`, whatever the lamp's own V1 wiring says. V1's wired head pad
  idles high and goes low on a touch, as a pulled-up button does, so a pad
  on line 100 can't be told from the reset button, and a hand on the head
  must never wipe a lamp. lamp-cloud's `system.info` says `"reset": "none"`
  for it, so the app sends its person to the hotspot instead. This stays
  until a real V1 pad on line 100 has been measured and a safe way to tell
  them apart is shown.
- **Its way to a new Wi-Fi** is setup's hotspot when the saved network is
  gone (#3020, below), with `LAMP_WIFI_RECOVERY=on` on its
  `lamp-setup-recover.service` (Fourteen's P2 on #2977). Off, the unit says
  so and exits at once; main's adapter (#2977) doesn't wait for it. The
  first builder signs off before the switch goes on, as for product lamps.

**What a moved lamp keeps of V1:** prepare never touches V1's own files,
and the product's reset plan names V2's stores only, so V1's person data
would outlive a reset; with no reset on a moved lamp, that waits for the
first builder's call when a reset comes back. **Wiping a moved lamp before
it is given away** has no way at the lamp yet: neither the hold nor
recovery wipes anything. Until a reset is allowed, that is a hand step for
the team (the first builder's call on #2937).

**On main:** #2977 carries setup and recovery in the generic package, and its
adapter starts setup alone on a lamp with no marker, then the lamp once
setup writes it. Not run on a lamp.

## When the saved network is gone (#3020)

With `LAMP_WIFI_RECOVERY=on` (off unless set, a drop-in on
`lamp-setup-recover.service`, which the product installs), a set-up lamp
whose saved network is gone (a new router, a changed password, a move)
opens setup's own hotspot after 90 s, as V1 did (`system/network/recovery.go`
in the parent repository), so the app's setup flow sets the new network
without a 5 s reset. A lamp moved from V1 has no reset (above), so this is
its only way back once its package carries the unit.

- **Watched** by `lamp-setup recover`, one look every 5 s: a station address
  on the Wi-Fi (online), `wpa_state` (joined to a known network's radio,
  waiting for an address: only slow), `lamp-setup.service` and a Wi-Fi
  change from the app (busy), and while the hotspot is open the phones on it
  (`iw dev wlan0 station dump`; one that can't be counted counts as some).
  This count and the station address (`ip -4 -o addr show`, also read every
  second while joining) use the reset queries' five-second bound below: a
  stalled or pipe-holding read is stopped and reads as none.
- **The policy** (`crates/setup/src/recover.rs`, pure, tested in
  `tests/it/recover.rs`): 90 s off every known network, not joining one and
  not busy, opens the hotspot. Only slow or busy, the 90 s start again. The
  saved network in reach but never joining (a changed password) is gone. From
  the hotspot the saved network is tried every 2 min, 45 s each, never during
  a setup run, and from under a phone only once it has held the retry 10 min
  (a phone that remembers the open hotspot may join it by itself while the
  router reboots); not sooner when it shows in a scan, since with a changed
  password it always would. A try that joins the radio and waits for an
  address, or that a Wi-Fi change takes, is never cut back to the hotspot:
  it counts as lost afresh, the 90 s again once that clears (reviewer-two
  on #3024). Online at any time ends recovery, and a 5 s reset
  (the marker gone) ends the watcher, setup's own then serving.
- **The hotspot** is setup's own (`device-ap-mode`), with its page and API,
  the marker and the person's data kept, and no setup light (the ring is the
  voice's). Its status says `runtime_phase: "recovering"`, which tells the
  app it is recovery and not a first setup. A setup request there joins its
  network and **saves nothing else** (`serve::NetworkOnly`, El on #3024):
  the key, the name, the zone, the backend's link, the reports answer and own
  keys in it are not kept, so the app's one full request works and nobody on
  the open hotspot can change the settings the owner kept. They are still
  checked as at first setup, so a malformed one refuses the request before
  any join. The status's `share_reports` is the answer the last setup
  kept, read as lamp-cloud reads that record, never the request's. **The
  network is the one thing such a request sets.** Joined, recovery ends.
  **What it cannot stop** (the Reviewer-lead on #3024): the hotspot is
  open, as at first setup, so someone in radio range while it is up can
  set **their own** network; the key, the backend link and the
  person's data stay, but the lamp's traffic then crosses their network.
  The first builder signs off before `LAMP_WIFI_RECOVERY` goes on anywhere
  (the sticker's password on a recovery request is the proposed guard).
- **Said:** the journal (`lamp-setup-recover`) says when it opens and why,
  each retry, and when recovery ends; lamp-cloud's `system.info` says
  `"recovery": true` while it runs with its switch on, so the app names the
  hotspot only to a lamp that will open one (Fourteen's ecm-sds-mobile#50).
- **Spoken** (#3743, the words agreed with Sixth): as the hotspot first opens
  for a loss, `recover` asks the voice's say service for the recording
  `wifi_recovering`, "I can't reach my Wi-Fi, so I've opened setup. Set my
  Wi-Fi again in the app." (`recover::SAY_OPENED`). It names no control and
  sends nobody to the reset. Once a loss (`Recovery::says_opened`): not at a
  reopening after a failed try of the saved network, nor after a try that
  went back to lost (only slow, or busy; Second's P3 on #4157), and again
  only after the lamp was back online. The voice stays up while the hotspot is open, as
  `recover` stops no unit. With no network there is no model, so it is the
  recording only, never words. One the voice doesn't hold (`not_provisioned`
  until its notices release, [failure-notices.md](failure-notices.md)) is
  journalled and the hotspot opens all the same.

Not shown on a lamp: the check is a lamp with its person, the router's SSID
changed, the hotspot open within 2 minutes, the app setting the new network,
and the voice back.

## After a setup from the Autonomous app

A checklist to run on the lamp once its setup from the app has finished (the
first builder on #705, 18 Oct), read from the product's own settings
(`crates/setup/fixtures/serve-own-page.json`, which install writes as
`/etc/lampOS/setup/serve.json`) and units. **Read each file as present or
missing, with its mode and owner, never its contents:** several hold the
person's key or the account's password.

```sh
for f in /var/lib/lampos-setup/done /etc/lampOS/voice/credential \
         /etc/lampOS/policy/jev.key /etc/lampOS/person/name \
         /etc/localtime /etc/timezone /etc/lampOS/zone.env \
         /etc/lampOS/setup/backend.json /etc/lampOS/setup/reports.json \
         /etc/wpa_supplicant/wpa_supplicant-wlan0.conf; do
  sudo stat -c '%a %U %n' "$f" 2>/dev/null || echo "missing $f"
done
readlink /etc/localtime
```

**What setup writes, and what a 5 s reset does with it:**

| Path | After setup | After a reset |
|---|---|---|
| `/var/lib/lampos-setup/done` (the marker) | present, written last | removed (the plan's `marker`) |
| `/etc/lampOS/voice/credential` (the voice's key) | 600 root | wiped |
| `/etc/lampOS/policy/jev.key` (the Policy's key) | 600 root | wiped |
| `/etc/lampOS/person/name` (the name typed) | 600 root, when a name was typed | wiped |
| `/etc/lampOS/person/place` (the owner's place from the app, #4333) | absent; 600 root once the app sets one with `LAMP_OWNER_PLACE=on` | wiped |
| `/etc/localtime` | a link to the phone's zone under `/usr/share/zoneinfo`, **when the request carried a zone the lamp's zone data has**; else as install left it | kept (decided on #806, 17 Oct 23:39) |
| `/etc/timezone` | the phone's zone, on the same condition | kept |
| `/etc/lampOS/zone.env` | `LAMP_ZONE` and today's offset, 644, on the same condition and only for a zone the parts' own zone data has too; else as install wrote it | kept |
| `/etc/lampOS/setup/language` (the owner's language, #4473) | one code, 644 root, when the request named one the lamp speaks; else absent (English) | kept, as the zone, and with it the voice's `/var/lib/lampOS/voice/language-said`, so the first start after a reset announces no change |
| `/etc/lampOS/setup/backend.json` (the app's link) | 600 root, **only when the app's request carried a whole link** | wiped |
| `/etc/lampOS/setup/reports.json` (the answer on reports, #1683) | 644 root, at every setup, a no included, unless the write failed | wiped |
| `/etc/lampOS/setup/display-name` (the lamp's name in the app, #1959) | not written by setup; 644 root after a `device.rename` from the app | wiped |
| `/etc/wpa_supplicant/wpa_supplicant-wlan0.conf` | 600 root | wiped |

Setup saves them in this order: the name, the zone, the language, the backend's link, the
answer on reports, the key, then the marker. So a marker means all the others were written, but
the backend's link and the answer on reports: those are best effort, and a marker with no
`backend.json` is what a failed write leaves (its journal line is below). The
reset plan (`reset-plan-product.json`) also wipes the person's stores
(memory, faces, a labelled session's face reads (`seems.log`) and the glance log (`glance.log`), tasks and Harness's sign-in, schedule, the Policy's and
`lamp-cloud`'s folders, the voice's sessions). It also empties
`/var/lib/lampos-skills-state` (including daily routine claims) and
`/var/lib/lampos-output-caps` (durable light and volume restrictions), even
when those features are switched off. These stores belong to the previous
owner; the private service directories remain for the next start. The zone
is the one setup setting it keeps. The installed plan comes from
`crates/setup/fixtures/reset-plan-product.json`; its wipe list matches the
keeper example in `reset-plan-example.json`. It follows the build (#806):
each install, and each update round with `LAMP_UPDATE_APPLY=on`, writes the
build's over any plan the lamp has, the old one kept in lamp-update's
records, so a store a later build adds is wiped by a lamp set up earlier.
A lamp with no plan gets none (docs/updates.md, "Setup's reset plan").

**The units the marker starts.** `lampos-setup-done.path` starts
`lampos.target` once the marker exists. Every part that is `PartOf` it waits
for the marker (`ConditionPathExists=/var/lib/lampos-setup/done`): actions,
air and its clock, body, cloud, controls, ears, expression, eyes, health,
memory, name, policy, schedule, the self-test's timer, the reset's watcher,
skills, tasks and voice. `lamp-setup` itself stops being started once the
marker is there; `lamp-update` and `lamp-records` don't wait for it.

```sh
systemctl is-active lampos.target lamp-voice lamp-setup-reset lamp-policy
systemctl is-active lamp-setup        # active for up to 10 minutes after setup, so the page can follow the lamp; inactive after that
systemctl status lamp-cloud --no-pager -n 3
```

**`lamp-cloud` is on in the product (#1815).** It is enabled and starts
with the target once the marker, `backend.json`, the voice's credential and
`/var/lib/lampos-update/state.json` all exist. Its unit carries
`LAMP_CLOUD=on` and stops V1's `os-server` as it starts, so on a lamp set up
from the app the journal says `online on the broker …` straight after
setup, and again after a reboot. A lamp masked to keep it off
(`systemctl mask lamp-cloud`) shows it masked. A lamp set up under V1, with no
`backend.json`, shows it skipped by its conditions, and its `os-server` is
left running. Don't paste the address. A failing 15 s ping says
`the backend's ping fails: …`, once for each new reason, and `the backend's
ping answers again` when it comes back.

**The setup journal, one line for each step** (`journalctl -u lamp-setup -b`;
the board keeps its journal in RAM, so read it before a reboot, or follow it
live):

| Step | Line |
|---|---|
| The radio looked before the hotspot | `lamp-setup: N networks seen before the hotspot, in M ms`, or `… no networks seen …` |
| The hotspot is up | `lamp-setup: the lamp's hotspot is up; answering on 0.0.0.0:80` |
| The app's request named a link that can't be kept | `lamp-setup: the request named a backend link, but <fields> … missing or not valid; setup goes on without it` (field names only) |
| The link couldn't be written | `lamp-setup: the backend's link could not be saved (…); setup goes on without it` |
| The run ended | `lamp-setup: run N ended Connected`, or `… ended Failed: …` with the reason |
| Already set up, at a later boot | no line from `lamp-setup`: systemd skips its start for `ConditionPathExists=!/var/lib/lampos-setup/done`, and `journalctl -u lamp-setup -b` shows only systemd's own line saying so. (`this lamp is already set up; nothing to serve` is printed only when it is run by hand.) |

Then the voice, once the target is up: `journalctl -u lamp-voice -b | grep
"person's name"` gives `the person's name from /etc/lampOS/person/name is used
(N characters)`, or `the person's name is not used: …` when the file can't be
read. With no name typed there is no file, and the voice says nothing of it.
It never prints the name.

## Units

`crates/setup/units/` holds the two units, for the image (#811) and the
keeper's release to carry; `BUILD` stands for the build's directory.
`lamp-setup.service` is started by the keeper alone, when the marker is
missing, and is never part of `lampos.target`. `lamp-setup-reset.service` is
part of `lampos.target`: it watches for the reset while the lamp runs. Both run
as root: the radio scripts, wpa_supplicant's file, `/etc/localtime` and nginx
for the first; the person's stores, which belong to several parts, the keeper's
stop and the reboot for the second.

## Open questions (asked on #806)

1. Where does a home lamp's voice credential come from? **Answered** (the
   owner, 2026-10-16 16:13, replacing the first builder's 12:36): the user's
   key, posted down to setup as in V1; a reset wipes it.
2. Is `key.pub` put in the image at the factory (proposed), not by setup?

## Not shown

No lamp, phone, router or hotspot was used for this page. The V1 behaviour
above is read from source and V1's own documentation, not measured.
