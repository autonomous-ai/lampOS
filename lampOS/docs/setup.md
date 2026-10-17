# Setup for a home (#806)

A lamp in a home is set up by its person, with a phone and no engineer: the
home's Wi-Fi, the voice's service, the person's name and time zone. Holding the
reset button for 5 s takes it back to setup and wipes the person's data. The
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
   its password, press Setup. The page's admin password defaults to the
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
| The backend's link: the MQTT four (`mqtt_endpoint`, `mqtt_port`, `mqtt_username`, `mqtt_password`), `fa_channel`, `fd_channel`, `device_id`, `llm_base_url` | **Keep** (17 Oct, #1396) | The Autonomous app sends them in the setup request (`ecm-sds-mobile` v3). They are kept so a lamp set up now can come online in the app later (`lamp-cloud`, #1396) without being set up again. Nothing reads them yet, and the MQTT link and the backend ping are not built. |
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
| The person's name | Read by the voice with `LAMP_PERSON_NAME` (the product unit sets it, #806): told to the model after its character, and the hello's name when no face is known (`crates/voice/src/person.rs`) | A private file, one line: `/etc/lampOS/person/name`. |
| The backend's link | Nothing | `/etc/lampOS/setup/backend.json` (`backend` in the settings), root's, 0600, written after the zone and before the key. It holds the fields above when the request carries a whole link: an endpoint, a port (a number, or digits), both channels, each field at most 512 bytes with no space or control character (the password may hold spaces). A missing or broken link writes nothing; when the request named one (an `mqtt_endpoint`) the journal gets one line naming the fields that were missing or not valid, never their values. The device's id, the endpoint, the port and both channels are needed; the base, the username and the password may be empty. Neither a broken link nor one that cannot be written fails setup: the lamp joins the Wi-Fi and talks, and only its place in the app waits. The password never appears in a debug line, and a crashed write's temporary file is removed at start. |

## The person's data, which reset wipes

From lamp-4ace's units: memory `/var/lib/lampos-memory`, faces
`/var/lib/lampos-eyes/people.json`, tasks `/var/lib/lampos-tasks/store` (and its
targets), schedule `/var/lib/lampos-schedule/store`, voice sessions
`/var/lib/lampOS/voice/sessions`, reminder delivery
`/var/lib/lampOS/voice/schedule-delivery`, the conversation thread
`/run/lampos-voice/thread.json`, the voice's first-output log
`/var/lib/lampOS/voice/first-output`, what the voice has told of tasks
`/var/lib/lampOS/voice/tasks-told` (its default, `LAMP_TASKS_TOLD`), the
policy's log `/var/lib/lampos-policy`, the person's parts of Harness under
the tasks part's home (`/var/lib/lampos-tasks/.harness/` `auth`,
`connections`, `computer-id`, `cli/data`, `agent-messages`, `hook-routes`
and `logs`: the sign-in, the pairings and what they left; not `.harness`
whole, which is Harness itself, its CLI and its own Node, and not the
Harness service `install` set up, #934; Nine on #1016), the user's key
(`/etc/lampOS/voice/credential` and `/etc/lampOS/policy/jev.key`, #806,
16:13), the name, the backend's link (`/etc/lampOS/setup/backend.json`, with the broker's password, #1396), the Wi-Fi configuration and the setup marker. Kept: the
software, its releases, the update key, the touch calibration and other
per-lamp facts, and physical privacy.

## The reset (built, off unless set)

`crates/setup` builds `lamp-setup`. Its first slice is the reset:

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
   (#767): the voice pauses, `lampos.target` stops, the body folds the lamp;
4. it wipes each path in the plan: a file is removed, a directory keeps itself
   (systemd units name some of them and would not start without them) and
   loses everything in it. It walks by handle, one component at a time from
   `/`, following no link anywhere: a part that swaps a directory for a link
   mid-wipe makes the wipe fail instead of reaching outside the plan;
5. it removes the journal, syncs, and reboots. If the wipe fails it still
   reboots, and if the reboot command fails it forces one: a lamp left dark
   with no marker would wait for a hand.

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
journal must not be inside a wiped path. Without `--enable`, `watch-reset`
only logs a valid reset and `resume-reset` only says whether one is unfinished.
`crates/setup/fixtures/reset-plan-example.json` is the plan for lamp-4ace's
paths; the name and Wi-Fi entries are where setup writes them, and the voice's
key is not in it.

V1 says "Factory reset starting" before it reboots; V2 does not yet speak: the
voice is among the parts stopped. What the person sees is the lamp folding and
going dark, then the hotspot coming back with the setup light (below). Where `resume-reset` and
`watch-reset` run in the boot order, and with what rights, waits on the
keeper's owner (the fifth builder, asked on #702).

Host tests (`crates/setup/tests/reset.rs`): the order of the steps, a failed
stop wiping nothing, a wipe that fails halfway keeping the journal and marker
until `resume-reset` finishes, a start with no journal touching nothing,
refused plans, every reason an event is not a reset, the watcher on a real bus
acting once and only logging with the switch off, the command line, and the
example plan covering every store of the person's listed above.

## Joining the home's Wi-Fi (built, not yet served)

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
never match. And a failed join puts the previous file back before the hotspot
returns, so nothing of it is kept. `wpa_cli status` prints the joined network's
name with C escapes ("Nhà Lan" as `Nh\xc3\xa0 Lan`); it is decoded before it is
compared with the name asked for. V1's eight failure cases, from its
own captured wpa_supplicant lines, are tests here (`crates/setup/tests/wifi.rs`),
with the join's timing against a fake lamp.

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

## The setup server (built, off unless set)

Its `GET /api/device/setup/status` answers V1's fields, plus one of its own: `"generation": "lamp-v2"`. The Autonomous app keeps V1's Lamp and this one as two products, as it does Intern's two, and stops before the setup `POST` when the lamp on the hotspot is not the product the person picked (#1396, decided 17 Oct).

```sh
lamp-setup serve --config SERVE.json --plan PLAN.json --enable
```

The keeper starts it, as `lamp-setup.service`, only when the setup marker is
missing (#702). In order, it finishes an interrupted reset, refuses to run on
a lamp that is already set up, raises the hotspot with the image's
`device-ap-mode` (as V1's `os-server` does at a first boot), runs the
configuration's `free_port` and `web_start` commands, reads the lamp's name,
and answers V1's setup API with V1's shapes and wrapper
(`{"status","data","message"}`). The name is read only after the hotspot: on a
fresh image `device-ap-mode` is what renames the host `lamp-xxxx`, and a name
read before it would send the page to the wrong `.local` address:

| Route | What it does |
|---|---|
| `GET /api/device/setup/status` | V1's `phase` (`idle`/`connecting`/`connected`/`failed`), `lan_ip`, `error`, `mac` (the kernel's host name, `lamp-xxxx`, which avahi announces), `run`, `set_up_completed`, `runtime_phase`. |
| `GET /api/network` | `iw dev wlan0 scan`, parsed as V1 does, in V1's `domain.Network` fields. |
| `POST /api/device/setup` | V1's `SetupRequest`; its other fields are accepted and ignored. Checked before anything is touched: the network's name and password as for the join; `llm_api_key`, the user's voice key (at most 512 printable characters; none keeps the key already there, as on a lamp that ran V1, but a lamp missing either key file is refused with "the lamp needs its voice key", since it would have Wi-Fi and no voice); V2's own `name` (up to 64 characters) and `timezone` (an IANA name whose file, links followed, is a `TZif` zone file inside the zone data; one the lamp lacks is skipped, not a reason to refuse setup). Replies at once, then, 2 s later as in V1, joins; one run at a time; refused once the lamp is set up. |

While joining it reads the station address every second and publishes it as
`lan_ip`, V1's fix for the page that lost the lamp, and once more when the join
is done; a new run clears the last run's address at once, so V1's page cannot
follow an old one. Once joined, it saves the name, the time zone
(`/etc/localtime` linked to the zone, `/etc/timezone` naming it, as V1's
`timezone.go`, and `/etc/lampOS/zone.env` for schedule and skills, #820) and then the user's key (to the voice's credential and the
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

Host tests (`crates/setup/tests/serve.rs`): every refused request, a run that
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
`tests/serve.rs` (Fourth on #1341).

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
breathing twice as fast); `crates/setup/tests/light.rs`, the painter's
thread on a fake ring: white, blue, then black and the ring closed before the
marker; amber again after a save that failed; nothing drawn after the marker
of a run that finished; black at stop; a ring that cannot be opened said once
and letting go waiting for nothing; a ring that failed and is free again
during the fade out not opened after the marker (the reviewer's probe on
#1325; it fails without the fix); the press after a failed save blue at once,
never dark; and with the switch off no ring opened. Not shown on a lamp: the colours, the
brightness in a room and the timing on the ring itself are a lamp check.

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
4. **Failures.** A wrong password (expect `wrong_password` in about 5 s and the
   hotspot back), a network out of range (`ssid_not_found` at 60 s), a name with
   an accent and one with a `"`. Each failure must leave the previous Wi-Fi file,
   and the ring amber until the next try.
5. **Reset.** Controls with `--touch` and a `reset` line in its pins file. Hold
   the reset button 5 s and release: the lamp pauses, folds, reboots into the
   hotspot, and every path in the plan is empty. Once, cut the power during the
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
