# #4863 station-back: test script for lamp-52e6 (Seventh for Sixth, 2 Nov). DRAFT

Kit: `kit/st4863.sh` (`phase0`, `go`, `run`, `back`, `dry`). Dry output: `kit/st4863-dry.txt` (full run) and `kit/st4863-dry-phase0.txt` (`PHASE0_ONLY=1 … dry`). v9 (`kit/st4863-v9.sh`): `kit/st4863-v9-dry.txt`, `kit/st4863-v9-dry-phase0.txt`, and the LAUNCH DRY `kit/st4863-v9-launch-dry.txt` (`kit/st4863-v9-launch-dry.sh`: the exact launch command line through go's fork; see the L-rows at the end). **No lamp time before the lead's yes.**

**The first lamp step is `LIMIT=HHMM kit/st4863.sh phase0`, and it needs its own yes.** It takes the lamp lock and posts PLAYING/QUIET on #3336 like `go`, then runs preflight, setup, phase 0, and the full put-back with the guard removed. It never runs an arm and never leaves station mode (one reboot, as a station). Its own MINLEFT is (5 + GUARD_MIN + 6 + 6) min, 22 min at GUARD_MIN=5. Same as `PHASE0_ONLY=1 LIMIT=HHMM kit/st4863.sh go`. `go` (the arms) is a separate yes, after phase0 has passed.

Two safety reviews (2 Nov) said NOT SAFE; every finding and where it is fixed is in the table at the end.

## What is tested

Sixth's fix, PR #4931 (`sixth/recover-station-back`, merged), changes two things in `lamp-setup recover`:

- **Close.** When recover stops with its hotspot open, it returns the radio to station mode (`device-sta-mode`, then `sync`).
- **Boot repair.** At every start of `lamp-setup-recover`, before the switch is read, a set-up lamp left in AP mode (`wpa_supplicant@wlan0` masked, `denyinterfaces wlan0`, or, new in `00cf852f7c`, no `/etc/systemd/system/multi-user.target.wants/wpa_supplicant@wlan0.service` link; 52e6 has that link now) is returned to station mode. So it runs whatever `LAMP_WIFI_RECOVERY` says.

The fix is merged and in stable **9582** (lamp-voice `ab6ef495ce`; `8dd46590d` is its ancestor, train 521), installed on 52e6 tonight. The kit tests **the installed `lamp-setup`**; nothing is staged. The preflight reads the build from `/var/lib/lampos-update/state.json` (`EXPECT_SEQ=9582`, `EXPECT_COMMIT=ab6ef495ce` by default, overridable by env, as in sw2b).

### The build gate

Recover's unit (`deploy/product/lamp-setup-recover.service`) starts `ExecStart=/opt/lampOS/setup/current/lamp-setup recover --config /etc/lampOS/setup/serve.json --enable`. lamp-update installs each part as `/opt/lampOS/<folder>/<sha256>/<binary>`, the folder named by the binary's SHA-256, with `current` a link to it (docs/updates.md, "What changes").

- **At preflight:** the kit reads recover's effective `ExecStart` program (`systemctl show -p ExecStart`), resolves it (`readlink -f`), and takes its release folder's name. The program must hash to that name, and must hold the fix's repair and close lines. Otherwise the kit refuses: it is not the build's own file, or not a build with PR #4931. The path, sha256 and folder name go into `backup/recover.exec`; `check_backup` refuses unless the sha256 equals the folder name.
- **Mid-run, the kit refuses and logs** if the program changes (lamp-update moved it):
  - before each arm (`buildok`, Mac): path, sha256 and folder name as at preflight, and no `switching to build` in lamp-update's journal for the last 10 min (also a preflight gate);
  - inside each arm (lamp): the program's path and sha256 against the backup, and the running recover's `/proc/<pid>/exe` against the preflight sha256. A mismatch is `VOID build-moved`, in the lamp log;
  - after each window (`report`'s `build` line): a change is `VOID recover's lamp-setup changed in the window`, and the next arm's `buildok` aborts;
  - in every put-back (`st.sh xcheck`, the guard's and the kit's): the program must be the backup's path and sha256, or the put-back is `PUTBACK FAILED`.
- **The switch** is the kit's only recover drop-in: B's `LAMP_WIFI_RECOVERY=on` in /etc (on at boot); A's and C's in /run (off at boot).
- **Proof the fix ran at boot:** the repair line in this boot's journal. The preflight requires the line in the installed program.

**The put-back's check (`st.sh xcheck`, in both the guard's put-back and the kit's `back`):**
1. The kit's switch drop-ins (`zz-seventh-st4863-on.conf` in /etc and /run) are removed.
2. Any other `zz-seventh-*` drop-in of recover or `/opt/lampOS/releases/seventh-*` (an older run's staging) is a leftover: logged, removed, and **this pass is refused** (`XCHECK FAILED`, so `PUTBACK FAILED`; the guard retries in 60 s, and the kit's `back` can be run again). The preflight also refuses to start while one exists.
3. Synced, `daemon-reload`. Check: `DropInPaths` of recover has no `seventh`, and recover's program is the backup's path with the backup's sha256 (the build's).
4. `XCHECK OK …` or `XCHECK FAILED …`.

The DRY runs the lamp side under dash on a fake root on the Mac, with systemd-252-shaped fakes on PATH. With `SIM=1` it takes every branch the real lamp takes (ROOT empty: systemctl's views, the selftest's unit half), runs selftest, pet, facts (fed to the real preflight), build, arm A, arm B, report, put-back, the no-LAN path, the expiry and disarm under `dash -x`, and lists every st.sh function that ran (T-P1); none may be left out. It also runs `xcheck` on the fake root: the build in `setup/<sha256>/` with `current` linked to it, B's switch drop-in plus an older run's ExecStart drop-in and staging folder. The first `xcheck` removes them and refuses; the retry is OK; a third is a no-op. Then lamp-update "moves" the build (a new folder, `current` re-pointed), and `xcheck` refuses.

The DRY also shows the wants link: the fake root has it (`-> /lib/systemd/system/wpa_supplicant@.service`), the backup records it, AP mode's removal shows as `DIFF … now ABSENT`, `restore` recreates it, and a wrong target (`/dev/null`) is also caught and put back.

## The lead's rules and where the kit enforces them

1. *"No step may leave 52e6 without its client network unless something brings it back by itself. A guard installed as a real systemd unit plus timer in /etc (not /run) must run device-sta-mode at boot and after N minutes, whatever happens. It's removed only at the verified put-back."* The lead accepted "at boot" as boot + GUARD_MIN; GUARD_MIN is now 5 min (was 6), so the fix gets the first 5 min to do its job.
   - **The service:** `/etc/systemd/system/seventh-st4863-guard.service` runs `st.sh putback force guard`. **First, past the expiry (below), it removes its own units and exits 0, before the lock and before any restore** (R-P1). **On a clean station with no arm unit active and no kit drop-in it stands back (F-P2): nothing to put back, so it touches nothing, exits 0 and logs `PUTBACK OK (guard): stood back: …`, the line phase 0 and B/C read as the boot leg's OK run.** **A partial backup (`manifest`, `recover.exec`, `wants` or `units` missing) is refused before anything is touched: no recover stop, no reboot (F-P3-2).** Otherwise it runs under the kit's one lock (`/var/lib/seventh-st4863/lock`, `flock`; also held by every kit put-back and by each arm; the guard waits up to 660 s, then goes on without it, because bringing the station back beats exclusion):
     - recover stopped;
     - the kit's drop-ins off and recover's program checked against the build (`xcheck`; a program moved by lamp-update is logged, not a put-back failure);
     - the files back by hash;
     - `device-sta-mode` **only when the lamp is not already a clean station** (never on a healthy lamp), **before** the bring-up (the lead's condition 1; the DRY checks the order). It runs under `set -e` and stops at its first failed step, for example `iw dev wlan0 set type managed`; it is its own process, so that cannot skip what follows;
     - the files again; the unit states (nginx included) and the hostname as found;
     - nginx stopped if it was inactive at the backup and the reg domain back to the backup's (`iw reg set`; 52e6 is `00`, AP mode sets `US`), **even on a clean station** (F-P3-4: neither touches the Wi-Fi);
     - **its own station bring-up**, without relying on `device-sta-mode`, **skipped on a clean station** (R-P2-1, the lead's condition 2: a healthy lamp gets no wpa/dhcpcd restart and never leaves its LAN; the DRY counts zero restarts): hostapd and dnsmasq stopped and disabled, the AP address removed, wlan0 up and `managed` (down/up around it when it was not; a failure is tolerated and logged), the Wi-Fi file re-checked by hash (restored if it differs), `wpa_supplicant@wlan0` unmasked/enabled if needed and restarted, `wpa_cli -i wlan0 enable_network all`, `systemctl restart dhcpcd`, then up to 90 s for a LAN address;
     - **still no LAN: a counted reboot.** The counter is `/var/lib/seventh-st4863/reboots`, at most 2 a kit run; each is logged (lamp log and loans.txt) and synced before `systemctl reboot`. The files and units are back by then, so the boot comes up a station and the boot leg runs again at boot + GUARD_MIN. With the count used up: logged loudly (`NO REBOOT. LOOK NOW`), no reboot.

     On a failure it retries every 5 min (`Restart=on-failure`, `RestartSec=300`) for ever: `StartLimitIntervalSec=0`, no `StartLimitAction` or `FailureAction` (R-P2-3). Reboots come only from the counted `creboot`; a missing or broken `st.sh` only fails the unit. `TimeoutStartSec=1800` covers the worst put-back with every blocking step at its full 90 s (R-P3): lock 660 + stop recover 90 + its ExecStopPost 90 + stop of arm units 90 + xcheck/restore 5 + device-sta-mode 120 + bring-up 460 (stops 180, restarts 180, enable_network 10, LAN wait 90) + start recover 90 + checks 10 = 1705 s. The lock wait (660 s) covers an arm's longest hold at worst: 603 s.
   - **Hard expiry (R-P1):** setup stores setup + 3 h (setup + 1 h for `phase0` alone: T-P2) in the backup (`expires`), and every pet pushes it to now + 3 h (now + 1 h for `phase0`; F-P2). An empty or non-numeric `expires` counts as expired (F-P3-1). A guard run past that date removes its units now, then `/var/lib/seventh-st4863` once no unit is left (T-P3-2), and exits 0 at once: no lock, no restore, no reboot. So a guard left behind never brings back the old Wi-Fi file over a network changed since. A clock behind after a boot without NTP (no RTC) only delays it.
   - **It never fights a person (T-P2):** while `lamp-setup` is active, a `.lamp-previous` Wi-Fi change is pending, or the done marker is gone (a re-provision), a guard run logs `GUARD STANDS BACK` and exits 0, touching nothing. The Wi-Fi file is never copied or restored (see below), so a network changed through the app or recover is left as it is.
   - **loans.txt:** the guard appends one line per run, and per counted reboot, to `~orangepi/loans.txt` on the lamp.
   - **The boot leg:** `/etc/systemd/system/seventh-st4863-guard.timer`, `OnBootSec=5min`, enabled for `timers.target`. It fires at boot + 5 min at every boot.
     - It is enabled at setup but not started. An `OnBootSec` already in the past fires at once when its timer is started.
     - The kit never restarts it.
   - **The pet leg:** `/etc/systemd/system/seventh-st4863-pet.timer`, `OnActiveSec=5min`, `Unit=` the guard service, no `[Install]`. The kit restarts it before each window, so the guard fires 5 min after the last pet. A reboot drops it, and the boot leg covers that boot.
   - **Independent of lamp-setup:**
     - The units have no `After=`, `Requires=`, `BindsTo=` or `Wants=` lines of their own, only systemd's default deps.
     - At every pet, `selftest` reads the effective `After/Requires/Requisite/BindsTo/PartOf/Wants/RequiresMountsFor` of the service and both timers. It fails on any `lamp-setup` unit, anything under `/opt` (where lamp-setup lives) or an `opt-*.mount`.
     - The guard runs `/bin/sh` and the image's own tools. `selftest` checks each of these resolves and is not under `/opt` or `/var/lib`: `systemctl journalctl busctl flock ip iw wpa_cli timeout sha256sum hostname awk sed grep cp rm sync cut tr`, plus `/usr/local/bin/device-sta-mode`. The preflight also requires `flock` and `busctl`.
     - If lamp-setup hangs or fails at boot, nothing in the guard waits on it.
   - **The LAN watcher:** `/etc/systemd/system/seventh-st4863-lan.service`, enabled, is observation only. It polls `ip -4 -o addr show wlan0` every 1 s for a LAN address (not 192.168.100.x) and logs `LAN <addr>` with the uptime. This image's dhcpcd logs no `wlan0: leased`.
   - `fin` removes all four units as soon as the station, the files by hash and no kit drop-in hold (`st.sh disarm`), and checks that 0 unit files are left before it removes `/var/lib/seventh-st4863`.
   - Every reboot waits for the boot leg's run before the next pet. Otherwise the boot leg, which a pet does not move, would fire inside the next window.
2. *"Back up every file AP mode touches … to /var/lib with hashes, and restore by hash. A lamp-side DRY step, before any AP step, checks that the restore works … It also checks the guard unit is active and its timer armed."*
   - `st.sh backup` copies these to `/var/lib/seventh-st4863/backup/`, with a sha256 manifest. Missing files are recorded as `ABSENT`.
     - Files: `/etc/dhcpcd.conf`, `/etc/dnsmasq.d/99-lamp.conf`, `/etc/hostapd/hostapd.conf`, `/etc/hosts`, `/etc/hostname`, and the Wi-Fi file's **hash only** (no copy: T-P2).
     - **The Wi-Fi file is never restored.** The kit never writes it: the cut is `wpa_cli disable_network all` (runtime only) and `device-ap-mode` does not edit it (scripts/imager/build-orangepi-v2.sh), so no arm needs it back. `check` reports it as `info … same|CHANGED`, never as a difference; an arm whose Wi-Fi file changed since setup ends `VOID wifi-file-changed-since-setup`. The bring-up keeps `wpa_cli enable_network all` for the runtime-disabled case.
     - Enable/active states of `wpa_supplicant@wlan0`, `hostapd`, `dnsmasq`, `dhcpcd`, `avahi-daemon`, `nginx` and `lamp-setup-recover`, plus the hostname, the reg domain (`iw reg get`) and the guard's expiry (setup + 3 h, pushed at each pet).
     - Recover's program path, sha256 and release folder name (`recover.exec`; the sha256 must equal the folder name).
     - The wants link `/etc/systemd/system/multi-user.target.wants/wpa_supplicant@wlan0.service`: whether it exists and its `readlink` target (`wants`, `ABSENT` when missing). The fix counts a missing link as a left hotspot, so the preflight requires it (`wants=1`), `check` compares it like a file hash, and `restore` puts it back (recreated with the backup's target when AP mode removed it, `daemon-reload`). The guard's put-back and the kit's `back` both end in `check`, so each requires the link to be the same as before.
   - Writes are synced, because the root is mounted `commit=600`. A later run never overwrites the backup.
   - `st.sh selftest` is the lamp-side DRY, run in `setup()` and again at every pet. It prints `SELFTEST ARMED` only when all of these hold:
     - the restore is proved on a scratch copy: the backup is copied out, `device-ap-mode`'s edits are made on it (the wants link left out, as AP mode removes it), it is restored by hash and every hash compared;
     - the boot leg is enabled with an `OnBootSec=`;
     - the LAN watcher is enabled;
     - the pet leg is active with a next elapse (`NextElapseUSecMonotonic` read raw with `busctl`; none, 0 or UINT64_MAX is no elapse);
     - systemd has loaded all four units (`LoadState=loaded`); `systemd-analyze verify`'s notes are logged, not a gate (R-P3);
     - the deps and tools checks above pass.
3. *"No real power cut. A reboot stands in for it, and only after (1) is checked active."*
   - B and C use `sync; systemctl reboot -ff`, launched by the lamp-side arm only after `pet` saw `SELFTEST ARMED`. Sixth confirms `reboot -ff` still calls sync() unless `--no-sync` is passed, and the kit syncs first anyway.
   - `-ff` stops no units. Recover gets no SIGTERM, so the close does not run, and the boot finds the AP state just as it would after a power cut.
   - What this does not model: unsynced writes. A real power cut can also roll back `device-ap-mode`'s own edits. That is the milder case, because the lamp would then boot as a station.
4. *(Dropped by the lead: no lamp can take a cable.)*
   - **52e6 has no wired path.** Once wlan0 is lost, the guard is the only way back.
   - There is one partial way in during an A window: the Mac's `en0` can join the hotspot `lamp-52e6` at 192.168.100.1. That works only while the hotspot has its address, which is gone after a reboot in AP mode.
5. *"Reuse KITS.md pieces. The test script comes first; no lamp time before the lead's yes."* Reuses are in the kit's header, with shas:
   - `lib.sh` b569b4e63c44: otherplaying, rs. L/LI are kept but without ControlMaster.
   - fifth `kit3697/stage.sh` 6828e3f45899: the boot-id wait and the DRY shape.
   - `sw2b.sh` 91554330cd9a: pin, LIMIT, PLAYING/QUIET, fin.
   - `lamp-lock.sh` a55b878c0665.
   - Not reused: `lib.sh`'s `deadman_arm`, a systemd-run timer under /run that is gone at a reboot.

## Phase 0: the boot leg proved (the lead's check 1)

Phase 0 runs in station mode, before any AP step:
1. Pet.
2. `sync; systemctl reboot` (a plain reboot: the lamp is a station).
3. Wait for a new boot id.
4. Wait up to boot + GUARD_MIN + 5 min (10 min) for the boot leg's run, then read `st.sh report`.

It passes only if all of these hold. **Any one missing: the kit aborts, and no AP step runs.**
- **The journal lines:** this boot's journal of the guard service, `journalctl -b -u seventh-st4863-guard.service -o short-monotonic`, has its first line between boot + 300 s and boot + 330 s, and `PUTBACK OK (guard): …` (the report's `gj=<monotonic s>`). On phase 0's healthy station that line is `PUTBACK OK (guard): stood back: …` (F-P2): the service ran and logged OK, and the timer below shows the boot leg fired.
- **The timer:** the boot leg's `LastTriggerUSecMonotonic` is between boot + 300 s and boot + 330 s (`gtimer=`). systemd 252's `systemctl show` prints `*USec*` properties as a timespan (`5min 204.511ms`), so the kit reads it raw: `busctl call … LoadUnit s seventh-st4863-guard.timer`, then `busctl get-property … org.freedesktop.systemd1.Timer LastTriggerUSecMonotonic` → `t <usec>`. The DRY's fakes print the 252 shapes and show the old read would have aborted on a good lamp.
- **Not the pet leg:** `seventh-st4863-pet.timer`'s `LastTriggerUSecMonotonic` (busctl) is 0 (`gpet=0`).
- **The service result:** `Result` of the guard service is `success` (`gres=`).
- **The lamp after:**
  - the files equal the backup;
  - a clean station with a LAN address;
  - `seventh_dropins=0 oldstage=0`.

The PASS line records the boot leg's time and the LAN time of a normal boot, from the watcher. That LAN time is the baseline for B and C's 120 s bar.

## Why the cut writes nothing

The cut is `wpa_cli -i wlan0 disable_network all`. It only changes the running wpa_supplicant and saves nothing. Any `wpa_supplicant@wlan0` restart reads the real file again. That happens in `device-sta-mode`, in recover's own retry, in the guard, and at boot. The real Wi-Fi file is never edited, copied or restored. Its hash is checked before every arm (a change voids the arm) and reported after.

## Order and minutes (`go`: about 46 min; `phase0`: about 22 min; LIMIT must leave that)

| step | what | how long |
|---|---|---|
| preflight | **Mac:** LIMIT, otherplaying. **Lamp (read-only):** pin 9582 `ab6ef495ce`, no lamp-update switch in 10 min, marker, recover **enabled**, setup inactive, no `.lamp-previous`, switch not already on, no leftovers (an older run's staging included), recover's ExecStart arguments as expected, **the build gate** (recover's lamp-setup hashes to its release folder's name, has the repair and close lines), clean station (wpa enabled/active, hostapd disabled, no `denyinterfaces`, managed, LAN address), lamp units active, voice unmuted | 2 min |
| setup | st.sh installed (sha checked); backup; guard units written; boot leg and LAN watcher enabled; pet leg started; lamp-side DRY | 3 min |
| **phase 0** | as above. **Any fail: the kit stops, no AP step** | 9 min |
| A | build check; switch on (/run): cut, hotspot, 10 s, `systemctl stop lamp-setup-recover` | 6 min |
| B | build check; switch on in **/etc** (on at boot): cut, hotspot, 10 s, `reboot -ff`, wait for the boot leg (a window that ends VOID without a reboot is read on the same boot: T-P3-3) | 11 min |
| C | build check; switch in /run (off at boot): cut, hotspot, 10 s, `reboot -ff`, wait for the boot leg | 11 min |
| back | put-back (`xcheck`), then the guard removed | 4 min |

Before each window, the kit does four things:
- It checks the build (`buildok`): the same lamp-setup as at preflight, no lamp-update switch in 10 min.
- It checks `otherplaying`, because the lamp says its hotspot line ("I can't reach my Wi-Fi…").
- It pets and sees `SELFTEST ARMED`.
- It renews the lock.

The lock is held by `lamp-lock.sh acquire` and renewed every 60 s; `go` posts PLAYING only once it holds the lock, and always posts QUIET with the run's exit (F-P3-3). The lock is a 600 s lease on the lamp: it cannot be renewed while the lamp is off its LAN, so a B/C window plus boot can outlive it; the lamp-side flock still keeps the kit's steps apart. Each window is about 4 min (at worst ~5 min: 20 s watching, 30 s cut, 150 s hotspot, 13 s hold, A's 90 s LAN wait). GUARD_MIN must be 5–7, which the kit checks.

**Worst-case time off the client network (GUARD_MIN=5):**
- A: the close fails → the pet leg (pet + 300 s; the cut is at about pet + 40 s) waits for the arm's lock (≤ 603 s from the arm's start at worst) (≤ 303 s from the arm's start), then puts back in ≤ 230 s: **≤ ~9 min** typical; with every systemctl call blocking 90 s, ~28 min.
- B/C: the fix fails → the boot leg at boot + 300 s (no lock at boot) puts back in ≤ 230 s: 30 + 150 + 13 + 60 (reboot) + 300 + 230 s = **≤ ~13 min** typical (~27 min with every call blocking). If the bring-up finds no LAN, each counted reboot adds ≤ 60 + 300 + 230 s (~10 min): **≤ ~33 min** with both.
- A lamp that hangs without rebooting fires neither leg: the boot leg needs a boot, the pet leg a running systemd, and 52e6 has no hardware watchdog.

## Verdicts (the kit's `ST4863 …` lines; everything read after the lamp is back, from `st.sh report`)

Sources:
- the lamp side's log `/var/lib/seventh-st4863/log.txt`, every line stamped with the boot id and uptime;
- `arm-X.result`, read by `st.sh report X` for arm X only (never another arm's file);
- `journalctl -b -u lamp-setup-recover -o short-monotonic` (`rec=`), with the lines confirmed by Sixth (tag `lamp-setup-recover`):
  - repair: `lamp-setup: recover: the radio was left a hotspot: back to the station`;
  - its failure: failed tries log nothing; only the give-up does, `… the radio was left a hotspot and the station could not start: <error>; the next boot tries again if it is still one`, at worst under 4 min from recover's start (before the guard's boot + 5 min). It is a FAIL;
  - close: `lamp-setup: recover: the hotspot closed: back to the station`;
  - its failure: `… the hotspot closed and the station could not start: <error>`;
- the LAN watcher's first `LAN` line this boot (`lan=`, uptime s);
- `jfirst=`: this boot's oldest journal entry (monotonic s; F-P3-5). The kit also reads `report X` at once after the boot and keeps its `rec=` lines if the later read lost them;
- the guard's lines: from the log (`guard=`) and from its own journal (`gj=`), plus `gtimer`/`gpet`/`gres`;
- `state`: `is-enabled`/`is-active` of wpa and hostapd, the `denyinterfaces` count, `iw dev wlan0` type, the wlan0 LAN address, wpa_state;
- the file hashes against the backup;
- `build` (recover's program, its sha256, its release folder, lamp-update's switches in 10 min), `seventh_dropins`, `oldstage`;
- from the Mac: `ping` plus a fresh `ssh` and the boot id.

**A (close)**
- **PASS:**
  - after `systemctl stop`, a LAN address is back within 90 s;
  - the close line is in the journal and no "could not start" line;
  - the files equal the backup;
  - `guard_in_window=0`, counted from the guard runs in the log at the arm's start and end.
- **FAIL:** no LAN address in 90 s, the close's "could not start" line, no close line, or a file differs.
- **VOID:** any of these:
  - the files differed before the arm;
  - recover's program or the running recover's sha256 is not the build's (preflight's);
  - the cut did not take;
  - there was no hotspot in 150 s;
  - the guard fired inside the window.

**B (switch on at boot) and C (switch off at boot)**
- **PASS, all of these:**
  - a new boot id;
  - the repair line in this boot (proof that the fix ran at boot);
  - no "could not start" line;
  - a LAN address by **boot + 120 s** (BOOT_LAN_MAX, confirmed by Sixth), from the watcher;
  - no `opening setup's hotspot` in this boot;
  - the boot leg's `found` shows no AP state (`wpa=masked`, `hostapd=enabled`, `deny=1` or `wants=0`);
  - the files equal the backup;
  - for C only, `off (LAMP_WIFI_RECOVERY is not on)` too.
- **FAIL:** the "could not start" line, no repair line, no LAN address by boot + 120 s, the hotspot opened again, or the guard found AP state.
- **VOID:** no new boot, C's switch was not off, recover's lamp-setup changed in the window (also for A), or no `rec=` line while this boot's journal starts after boot + 30 s (`jfirst`, rotated: F-P3-5; with the journal from boot it is the FAIL above).

**The baseline: recorded, not played.** The old C0 arm (switch off at boot, the installed lamp-setup without the fix) no longer exists: 9582's installed lamp-setup has the fix, and the kit stages nothing. The baseline is the record:
- **0c89's incident (2 Nov):** a set-up lamp lost power with recover's hotspot open (the test's backup and guard in /run went with it), came back up still a hotspot, off its client network (the app showed it Offline), and nothing on the lamp brought it back;
- **#4863's diagnosis comment 6573158451** (Sixth, from code at `bb3eef0eb6`): `device-ap-mode` leaves `wpa_supplicant@wlan0` masked, `denyinterfaces wlan0` and hostapd/dnsmasq enabled across a reboot; only `device-sta-mode` undoes them, and the old lamp-setup ran it only on a join; with the switch off, recover logs `off (LAMP_WIFI_RECOVERY is not on)` and exits. The comment calls this the likely cause, read from code, not reproduced on a lamp.

C is that case on the fixed build: the same AP state at boot with the switch off. Its PASS needs the repair line, which only the fix logs, and the lamp back by boot + 120 s, before the guard (boot + 5 min) could have done it.

**Why B's bar is boot + 120 s.** Without the fix, B (switch on at boot) also comes back by itself, only later:
1. At boot, recover sees no network.
2. 90 s later it opens the hotspot.
3. 120 s after that it retries with `device-sta-mode`.

So the lamp would be back about 3.5 min after boot. That is why B's PASS needs a LAN address by boot + 120 s and no reopened hotspot.

**D (fresh-lamp guard): host tests only, not played.** Without the done marker, setup itself takes over at the next boot and opens its first-setup hotspot by design. That would take 52e6 off its client network on purpose. What covers D instead: the unit's `ConditionPathExists=/var/lib/lampos-setup/done`, the code's own marker check, and PR #4931's host tests (no marker, setup active, or `.lamp-previous` → no station).

## Put-back (`fin`, also `kit/st4863.sh back`, safe twice)

1. It waits up to GUARD_MIN + 7 min for the lamp. If nothing of the kit is on it (no `/var/lib/seventh-st4863`, no guard unit, no seventh recover drop-in), it says `already put back` and stops: a second `back` is not an alarm. Otherwise it copies the lamp-side log to `~/seventh-watch/st4863-*.txt.lamplog`.
2. The pet leg is **restarted** (R-P2-2), not stopped: if this put-back loses the LAN, a leg is still there. The flock keeps the pet leg's run and the put-back apart; a guard run in progress is never stopped; disarm removes the leg at the end.
3. `st.sh putback ifneeded kit-back-<id>` runs **on the lamp as its own unit**, `systemd-run --wait --collect --unit=seventh-st4863-putback-<id>`, output appended to `/var/lib/seventh-st4863/putback.out`. The bring-up restarts the Wi-Fi, which drops the kit's ssh; the unit goes on. The kit then reads the result line by its tag from the lamp log over a fresh ssh. A counted reboot inside it: the kit waits for the new boot and runs it again (3 tries). The same `kputback` does the tidy after each arm. Its body is the guard's (above).
4. `lamp-voice` is restarted through lib.sh's `rs` only after a full run, and only when a restart-guard mute exists (a `reboot -ff` can leave one; `rs` sets it aside). `phase0` alone never touches lamp-voice (T-P3-1).
5. **The guard is removed as soon as these hold:** a clean station with the wants link, the files equal to the backup by hash, no kit drop-in and no `/opt/lampOS/releases/seventh-*`. `st.sh disarm`: both timers disabled and stopped first; a guard run still going is waited for (up to 6 min), never stopped; the lamp must be a clean station; then the four units removed, 0 unit files left checked, the station checked again. Only when disarm returns 0 does `rm -rf /var/lib/seventh-st4863` run (`disarm && rm -rf`), so units are never left without their script.
6. **Reported apart, never keeping the guard:** recover's program moved by lamp-update (`build=MOVED`), a lamp unit not active, voice/mute not false. These print `REPORTED APART … LOOK`.

**A setup that stops before the guard's units are written** leaves only `/var/lib/seventh-st4863` (the backup only reads the live files): `fin` removes it once no guard unit is confirmed (R-P3).

**A closed terminal (R-P1):** `go` runs the kit under `nohup` with its output only in the log (shown by `tail`), ignores HUP itself and passes Ctrl-C/TERM to the run. A run started directly traps HUP like TERM: `fin` puts back, and the tee and `fin` ignore HUP and PIPE. `log()` on the lamp never recreates `/var/lib/seventh-st4863` after the put-back removed it, and `systemd-analyze verify`'s notes are logged, not a gate (the gate is that systemd loaded all four units).

If the station, files or drop-in check fails, the kit prints `PUTBACK NOT OK … LOOK NOW`, **leaves the guard armed** and restarts the pet leg.

## Risks left

- The guard depends on systemd starting the boot leg at boot, and on wpa_supplicant/dhcpcd. Phase 0 proves the boot leg before any AP step. Its own bring-up no longer depends on `device-sta-mode` finishing.
- **A lamp that hangs without rebooting fires neither leg** (no hardware watchdog on 52e6). A boot that fails for another reason would strand the lamp, as any reboot would.
- The counted reboots are capped at 2 a kit run: a network that stays away longer (the router down) leaves the guard retrying its put-back every 5 min, without a reboot.
- `reboot -ff` skips `lamp-body`'s stop, so the head is not put to rest first. A power cut does the same.
- If `update_config=1` and something calls `save_config` during a window, the disabled network would be saved. Nothing in recover, lamp-setup (it never calls `save_config`, per the review) or `device-*-mode` does that, and the hash check after each arm, and in every bring-up just before wpa_supplicant restarts, would catch it.
- A healthy lamp is never restarted off its LAN: the bring-up is skipped on a clean station.
- **Rule 1's wording:** the lead's rule says the guard "must run device-sta-mode". The guard runs `device-sta-mode` when the lamp is not a clean station, then its own bring-up; on a healthy lamp neither. The lead's agent accepted this on those two conditions (2 Nov).

## Open points

Answered by Sixth (2 Nov): the journal lines, the build and its sha256, boot + 120 s, recover and setup both enabled on 52e6, no `wlan0: leased` on this image (hence the watcher), and `reboot -ff` syncs. The lead accepted boot + 6 min; GUARD_MIN is now 5.

New for Sixth:
1. At_start waits up to 30 s for `wlan0` before the repair. On B and C, is a repair line at about boot + 10–40 s what you expect, within the 120 s bar?

## The reviews' findings: where each is fixed (`kit/st4863.sh` line) and how the DRY shows it (`st4863-dry.txt` line)

| finding | fixed at (st4863.sh) | the DRY shows it (st4863-dry.txt) |
|---|---|---|
| B1 no last resort on a failed put-back | `putback` :328 (device-sta-mode only when not `clean` :173, before the bring-up), `bringup` :284, `creboot` :305 | :32–:38 bring-up after device-sta-mode exits 240, PUTBACK OK; :40 "B1 order: device-sta-mode's iw … before the bring-up's wpa restart"; :43–:49 counted reboots 1, 2, then none |
| B2 fin/disarm killed a guard run | `disarm` waits, never stops, station checked before and after :398 | :57 waited 15 s; :58 refused after 6 min, units kept; :59 refused on a non-station |
| S1 systemd 252 timespans | `usec` (busctl raw) :176, `report` :391, selftest next elapse :273, `p0check` :552 | :54 PASS; :55 "the old read … would have aborted phase 0 on a good lamp: caught"; :56 negative refused |
| S2 put-back over `ssh -n` | `kputback` :532 | :214, :242, :270 "systemctl is-active seventh-st4863-putback-…" then "PUTBACK OK (kit-after-X-…)" |
| S3 one flock | `lk` :179, `arm` :358, `LOCKW` :153 | :52 kit refused while held, guard goes on, LOCKW=10 waits 4 s |
| S4 guard removal / build | `fin` :693 (station + files + drop-ins only), `xcheck` XB :191/:200 | :109 "PUTBACK OK (kit-moved) build=MOVED" |
| S5 GUARD_MIN 5, worst case | default :107; header :71 | "OnBootSec=5min", phase 0 at boot + 300.2 s |
| S6 own arm's result only | `report X` :386, `verdict` :677 | each arm's report shows only its own `result=X` |
| note: `back` twice | `fin` :706 | :330–:334 "already put back" |
| note: `disarm && rm -rf` | `fin` :727, `disarm` :398 | "guard 0 unit files left … REMOVED" |
| note: arm A wait unbounded | `armrun` :650 | (no fake for a timeout) |
| P3: hang, loans.txt | header :61–:62 | :50 "loans.txt (guard lines): 8" |
| new entry `phase0` | :101, MINLEFT :122, no arm :1068 | st4863-dry-phase0.txt :149 "ARMS=none (phase0 only) … needs 22 min" |
| R-P1 expiry restored an old Wi-Fi file | `putback` :329 → `expire` :319 | :67 "expiry passed, Wi-Fi file changed: exit 0 … restores: 0, reboots: 0, units left: 0, … removed" |
| R-P1 HUP kills the run / fin | `runtraps` :763 (loops drop traps :767), `go` nohup :749 | :60–:66 "putting back … PUTBACK OK … run exit 143; its loops left: 0" |
| R-P2-1 bring-up restarted a healthy lamp | `bringup` :285 (skipped when `clean`) | :41 kit tidy, :42 phase 0's guard run: "wpa/dhcpcd restarts and link downs: 0, LAN kept" |
| R-P2-2 fin stopped the pet leg | `fin` :711 | :309 "systemctl restart seventh-st4863-pet.timer" |
| R-P2-3 StartLimitAction reboots | unit :430/:439, no StartLimitAction | :51 "broken st.sh: exit 2, missing st.sh: exit 2, reboots issued: 0" |
| R-P3 TimeoutStartSec, LOCKW | unit :437, `LOCKW` :153 | "== guard units:" :110 |
| R-P3 phase 0 plain reboot | :629 | :176 "systemd-run -q --on-active=3 systemctl reboot'" |
| R-P3 setup abort leaves $D | `GSET` :616, `fin` :696 | (no case) |
| R-P3 log() recreated $D | `log` :165 | :103 "SIM expiry: … removed" (logging after the removal did not recreate it) |
| R-P3 verify noise aborted | selftest :275 | :92 "the verify note was logged, not a gate" |
| **T-P1** `ld` used before set (selftest's real half died under `set -u`) | `ld` set :270 before its use | :91 "SIM selftest: SELFTEST ARMED" (dash, real half) |
| **T-P1** the real halves never ran in the DRY | `real` :158 (true on the lamp and under SIM=1); the SIM harness `SIMRUN` :1010 | :72–:105: facts fed to the real preflight (:82 build ok, no WOULD ABORT), selftest, pet, build, arm A, put-back, arm B (reboot -ff issued), phase0 TTL, disarm, no-LAN creboot, expiry; :104 every function listed, :105 "not run: none" |
| **T-P1 (found by the harness)** facts printed `addr=` (empty) when 99-lamp.conf is absent: preflight would abort a real station | `facts` :414 (`cat … | grep -c` prints 0) | :72–:82 facts → preflight passes |
| **T-P2** a leftover guard fights a person | (a) the Wi-Fi file hashed only, never copied or restored :207, `check` :227, `restore` :232, bring-up logs only :295, arms VOID on a changed file :361; (b) stand back :334; (c) phase0 expiry 1 h :612/:212 | :68 re-provisioned lamp: Wi-Fi file kept, 0 restarts, PUTBACK OK; :69–:71 setup active / .lamp-previous / no marker: "GUARD STANDS BACK", 0 systemctl writes; :100 "expires in 3599 s (setup + 1 h)" |
| **T-P3-1** fin restarted lamp-voice in phase0 | `fin` :714 | phase0 :201 "lamp-voice not restarted (phase 0 only …)"; full run :313 checks the restart-guard mute first |
| **T-P3-2** expiry left $D and the units | `expire` :319, `rm -rf $D` after 0 units :323 | :67 and :103 "units left 0, … removed" |
| **T-P3-3** B/C VOID without a reboot waited 12 min | `armrun` :652 | :273–:301 "arm B: VOID without a reboot: B VOID no-hotspot-150s", then "ST4863 B VOID" |
| **T-P3-4** stale docs | phase 0 comment, script rule 2 (verify), Wi-Fi file text | this file |
| **F-P2** (a) expiry 24 h; (b) the guard ran a put-back on a healthy lamp | (a) `TTL` :105, `backup $TTL` :626, `push` :316 called by `pet` :533; (b) stand-back `putback` :342–:345 | :110 "pet pushed the expiry: now + 10799 s"; :62 stand-back, 0 writes; :63–:65 its own line through `report`, waitguard and `p0check`: PASS; :66 no stand-back with an arm unit active; phase0 :207 "phase0 PASS"; st4863-dry-phase0.txt :65, :110 (TTL 3600) |
| **F-P3-1** empty/non-numeric `expires` | `expire` :325 | :78–:79 "expiry '' / 'garbage': … units left 0, … removed" |
| **F-P3-2** partial backup | `putback` :347 | :80–:83 each of four missing: exit 1, writes 0, recover stop 0, reboot 0 |
| **F-P3-3** QUIET lost, PLAYING before the lock | `go` :757 (`rc=0`), lock :761 before PLAYING :767 | :127 "go exit 2; lock acquired (call 3) before PLAYING (call 4); QUIET … (exit 2)" |
| **F-P3-4** nginx/reg skipped on a clean station | `bringup` :288–:290 | :44 "nginx inactive, reg 00, wpa/dhcpcd restarts … 0" |
| **F-P3-5** recover lines lost to rotation; lock lease | `report` jfirst :406, `armrun` :674, `verdict` :698; lease note :762 | :323 "VOID no recover line kept … 1712.3 s"; :324 jfirst 0.0 → FAIL |

## F2: the targeted review's findings, in `kit/st4863-v9.sh` (st4863.sh left as it was: phase 0 may run from it)

Lines are `st4863-v9.sh`'s; the DRY lines are `st4863-v9-dry.txt`'s (phase0: `st4863-v9-dry-phase0.txt`, same line numbers for these rows). Both DRYs: 0 would-aborts.

| finding | fixed at (st4863-v9.sh) | the DRY shows it (st4863-v9-dry.txt) |
|---|---|---|
| **F2-P2** (a) the stand-back ran before the backup-completeness loop | F-P3-2's loop moved above the stand-back `putback` :351 (after the person stand-back, which touches nothing) | :86–:89 (F-P3-2 cases unchanged: exit 1, writes 0) |
| **F2-P2** (b) the stand-back did not compare the files: AP mode's 99-lamp.conf/hosts edits stayed until the expiry when the kit died after arm C | stand-back condition `check >/dev/null 2>&1 && clean` :357 (`check` counts every file and the wants link; the Wi-Fi file is only reported as `info`, never counted :231) | :67 "clean station, hosts edited: … restored /etc/hosts PUTBACK OK (guard): device-sta-mode=skipped-clean, hosts back to its hash"; :62 a clean station with equal files still stands back, 0 writes |
| **F2-P2** (c) nginx/reg left on a stand-back | `tidyap` :293 (F-P3-4's two lines), called by `bringup` and by the stand-back before its OK line :358 | :68–:71 "stand-back, nginx active, reg US: … stood back, nginx inactive, reg 00, restarts 0"; :72 its line through `report`, waitguard's regex and `p0check`: PASS; :44 F-P3-4 via `bringup` unchanged |
| **F2-P3** INT/TERM between the lock and the run left the lock (and PLAYING) | `go` :775 trap set just before the acquire (lamp-lock.sh releases only our own holder+session, so an unheld release is a no-op): releases the lock, posts QUIET when `PLAYED` (set before the PLAYING post :782), exit 130; replaced by the run's trap after nohup | :134 TERM as the acquire returns: exit 130, release, no post; :135 TERM during PLAYING: exit 130, release, QUIET |
| **F2 (found while fixing)** `go` started the run by `bash "$0"` after `cd "$K"`: a relative `$0` (`bash kit/st4863.sh phase0` from ~/seventh-watch; the file is mode 644) does not resolve, so the run exits 127 at once (lock, PLAYING, QUIET "exit 127"; nothing on the lamp) | `ME=$K/$(basename "$0")` :98, used by `go` :787 and the DRY's huptest/go cases | checked by hand: `bash kit/st4863.sh go` (fakes, GO_RUN=nosuch) exit 127; `bash kit/st4863-v9.sh go` exit 2 |
| **F2 (DRY only)** after 22:00, `date -v+2H +%H%M` wraps past midnight and preflight read a negative time left | `dl` :131 (held at 2359) for the DRY's LIMITs | :104 SIM preflight on the real facts: build ok, no WOULD ABORT |

## L: the launch path (22:05's exit 127), in `kit/st4863-v9.sh` 5be8e6278be3

Tonight's phase 0 died at once: `cd ~/seventh-watch && LIMIT=2250 bash kit/st4863.sh phase0` cd's into `kit/`, then `go` forked `nohup bash "$0" run` with `$0` = `kit/st4863.sh`, which does not exist from inside `kit/` (exit 127; lock, PLAYING, QUIET "exit 127", nothing on the lamp). No DRY and no review caught it: MODE=dry never forks, and the one `go` DRY case used an absolute path and a fake run. The L-rows close that gap with a **LAUNCH DRY**: `kit/st4863-v9-launch-dry.sh` (output `kit/st4863-v9-launch-dry.txt`) runs the exact production command line from the launch dir and the kit's whole real path (go → lock → PLAYING → nohup fork of run → preflight → setup → phase0 [→ arm A] → fin → QUIET). Only the outside world is stubbed, at the PATH boundary (ssh, gh, ping) and HOME (a scratch home: its `seventh-watch/kit` is a link to this kit folder and its `seventh-watch/lamp-lock.sh`, the kit's default `ST_LOCK` path, is the fake lock; nothing lands in the real ~/seventh-watch). The kit has no LAUNCH_DRY branch and no time knob: its own sleeps and clock are real (a launch takes 2–3 min).

The fake lamp: each ssh command runs under dash on a fake root (ROOT=, SIM=1, the kit's own `fakebin()` tools, as the DRY's SIM harness does), so st.sh really runs. The lamp's absolute paths in the kit's command text go under the fake root (quoted heredoc bodies and a ROOT-aware script on stdin pass unchanged) and come out of the output again. The harness adds sudo, systemd-run (`--wait`, `--on-active`, `append:` output, the unit's active state) and a reboot: 5 s off the network, a new boot_id, /run's units gone, timers stopped, a new journal and monotonic clock, the LAN watcher started, then the boot leg (the guard service's own `ExecStart` from the unit file the kit wrote) after 60 s real. The lamp's monotonic clock runs ×5, so that is boot + 300 s lamp time. It also writes recover's journal lines: watching on a restart, the hotspot on the cut, the close on its stop.

Lines: `st4863-v9.sh` 5be8e6278be3 (the F2 table above has cb0e8e448907's line numbers); `st4863-v9-launch-dry.txt` 4809f3522eba; `st4863-v9-dry.txt` 176a45874348 / `st4863-v9-dry-phase0.txt` 76bd59aa1be6 (both: 0 would-aborts).

| finding | fixed at (st4863-v9.sh) | shown by |
|---|---|---|
| **L1** the run's path: `ME` must be absolute for every launch (with `bash kit/x.sh`, `bash ./kit/x.sh`, `cd kit && bash x.sh`, an absolute path, a symlink). cb0e's `ME=$K/$(basename "$0")` held for the first four, but through a symlink to the file, `K` was the link's folder: `. ./lib.sh` was not found there | `ME=$(realpath "$0")`, `K=$(dirname "$ME")`, `MN` from `ME`, then `cd "$K" && . ./lib.sh` :98; `go` forks `nohup bash "$ME" run` :794 | all five forms resolve `ME` to `/Users/…/seventh-watch/kit/st4863-v9.sh` and reach the usage (exit 2, lib.sh sourced); launch-dry :4: **L0**, the same launch on a copy with cb0e's line put back as the 22:05 kit had it (`ME=$0`), gives QUIET "exit 127" and the run never starts, so the harness catches tonight's failure |
| **L1 sweep** every other use of `$0`, a relative path or the cwd after the cd | none breaks. `$0` is left only in the usage text :800 and in st.sh's `sha < "$0"` (an absolute `$D/st.sh` on the lamp); `LOG`, `ST_LOCK`, `DD`, the verdicts and lamplog files are `$HOME`/`$K` absolute; lib.sh is sourced after `cd "$K"` (its `CLIPS` is from `BASH_SOURCE`; `.vol-52e6`/`.speaker-pin` are relative to `K`, unused here); the lamp paths are absolute | launch-dry :25–:35 and :63–:73 |
| **L2** QUIET listed `ls -t ~/seventh-watch/st4863-*.txt.verdicts \| head -1`: a run that wrote no verdict (an abort, tonight's 127) posted the newest **older** run's verdicts as its own | QUIET reads this run's own `$LOG.verdicts` (exported to the run) :800 | launch-dry :35 / :73: QUIET carries this run's `ST4863 phase0 PASS` (/ `A PASS`) |
| **L3** a LIMIT after midnight (`LIMIT=0030` at 23:10) read as today's 00:30: "too little time" abort | `left` :134: HHMM in seconds of the day; earlier than now is tomorrow's when that is within 12 h, otherwise passed; a malformed LIMIT reads -1 (passed). `dl` :131 is plain now + 2 h again | dry :136 "at 23:10 for 0030 2300 1200, at 00:10 for 0030, LIMIT 9999: 4800 -600 -40200 1200 -1"; dry :104 SIM preflight with a LIMIT past midnight: build ok, no WOULD ABORT; every launch (22:32, LIMIT=0032) passed preflight |
| **L4 (found by the launch DRY)** arm A/B/C `mkdir -p` both recover drop-in folders (`/etc/…/lamp-setup-recover.service.d`, `/run/…`); the put-back removed the switch drop-in but left the folders: an empty `/etc` folder stayed on the lamp after a full run | `xcheck` :208 `rmdir` both once empty (a folder holding a product drop-in stays; one that was there empty before the kit also goes, and systemd reads no difference) | launch-dry :70 (go, ARMS=A): the fake root byte-identical after the run. Before the fix, the same case failed with `D ./etc/systemd/system/lamp-setup-recover.service.d` left |
| **L5** the LAUNCH DRY's checks | `kit/st4863-v9-launch-dry.sh` 1f7ee4a13d91 | phase0 :25–:36, go ARMS=A :63–:75: the run started (no 127), preflight passed, setup put st.sh and the 4 guard units in the fake root (listed at the reboot), one reboot, the boot leg's `PUTBACK OK (guard)` at boot + 305 s (lamp time), `ST4863 phase0 PASS`, arm A PASS and its tidy (go), fin's put-back and disarm (`REMOVED /var/lib/seventh-st4863`), the fake root byte-identical (paths, modes, links, contents; loans.txt only appended, 0 lines here), the lock acquired then released, PLAYING then QUIET (exit 0); :77 "0 failed check(s)" |

Not stubbed, or not sure:
- The lamp's own systemd is fakes. The pet leg never fires, there is no `Restart=on-failure` retry, a reboot does not kill an old boot's processes, `DropInPaths`/`Environment` are static, and `systemd-analyze verify` is a fixed note. The real ordering and timing at boot are not modelled.
- st.sh reads `/proc/uptime` and `/proc/sys/kernel/random/boot_id` directly (not under ROOT). On the Mac its log lines say `b=mac up=`: `report`'s per-boot `guard=` filter matches every boot, and `lan=` is empty ("LAN at boot + s" in the phase0 line). The kit side's `bootid` does see the fake boot_id. Neither changes a verdict here.
- The path rewrite is the harness's. A lamp path the kit wrote outside its list (`/var/lib /etc /run /opt /proc /usr/local`) would hit the Mac's real path, which is root-owned, so the command fails loudly and nothing gets written. The fake world's arm A opens and closes the hotspot at once (`open=0`); B and C are not launched (no reboot -ff path through the launcher).
- The real ssh (keepalives, a dropped connection mid-command), the real lamp-lock.sh lease and the real gh are not exercised.

## V10: the arms review's two P2s, in `kit/st4863-v10.sh` 0a743c092ec6 (v9 5be8e6278be3 untouched; lib.sh b569b4e63c44 unchanged; st.sh, the lamp side, unchanged)

`diff st4863-v9.sh st4863-v10.sh` shows only these (and their DRY cases).

| finding | fixed at (st4863-v10.sh) | shown by |
|---|---|---|
| **P2-1** every `renew` ignored its status: the 600 s lamp lease can expire while 52e6 is off its network (B/C with the fix failing: ~10–13 min), another session can take 52e6, and the kit would still reboot it with the next arm and post QUIET "FREE" | `renew` :558 returns lamp-lock.sh's status (0 ours; 1 taken by another or released; 3 the lamp not asked). lamp-lock.sh's renew of an expired lock that is still ours rewrites it (exit 0): fine; one another session took is exit 10 -> 1. `renew \|\| abort "the lamp lock was lost (lease expired while 52e6 was off the network): no further arm (lamp-lock exit N)"` before each window (armrun :683, before the lamp start; phase 0 :669, before its reboot) and after each wait (armrun :704, after waitreach/waitguard, before the verdict and the tidy; phase 0 :673). The loops (waitguard, the 60 s renewer) still renew without aborting. The abort goes to fin, which starts no arm and reboots nothing from the Mac (its put-back only). QUIET :810: go's release refuses a lock that is not ours and prints it; its `holder:`/`session:` make QUIET say "lock lost to <holder> (<session>)" instead of FREE | dry :335 (renew fails before B's window: ABORT, B started 0, then fin's kit-back put-back, PUTBACK OK, no arm unit, no reboot), dry :336 (renew fails after B's waitguard: B started 1, no tidy after B, fin's put-back only), dry :136 (the release refuses, holder fifth: QUIET "lock lost to fifth (st-99)." and no FREE); rv8 V1 (launch: another session takes the lock as B's window starts: ABORT after B's wait, no arm C, 2 reboots only, PUTBACK OK, QUIET "lock lost to fifth (st-77)", fake root identical) |
| **P2-2** `kputback`: a systemd-run that never started (its ssh failed at that moment) reads "inactive" with no tag line, which fell through and stalled 15 min | `kputback` :569/:581: inactive (or failed) with no line for this call: started again, 20 s apart, at most 3 restarts, then "PUTBACK unknown (…): its unit never ran (3 restarts)" (return 1: armrun aborts, fin keeps the guard) | dry :337 (first start lost: 1 restart, PUTBACK OK), dry :338 (never runs: 3 restarts, unknown, exit 1); rv8 V2 (launch: the first put-back start's ssh fails: "started again", PUTBACK OK (kit-after-A), every launch check passes) |

Runs: `st4863-v10-dry.txt` 0c0452771635 (ARMS="A B C") and `st4863-v10-dry-phase0.txt` 7dcdab56806e: 0 would-aborts each; every kept lamp script passes `sh -n` and `dash -n`; `bash -n` clean. `st4863-v10-launch-dry.txt` 0a7554962f9d (`kit/st4863-v10-launch-dry.sh` bb4501f4fc88: v9's harness with v9 -> v10; its go case is ARMS=A as in v9): 0 failed checks. `st4863-v10-rv8.txt` b4f4e92f4d4a (`kit/st4863-v10-rv-launch.sh` 7454aaeb6eba: the arms reviewer's rv-launch.sh with v9 -> v10, plus V1/V2): the fakes resolved before every run (10/10); R1 go A B C 17, R2 15, R3 13, R3b 13, R4–R7 2 each, V1 5, V2 12 checks: all PASS, the same counts as the reviewer's v9 run.

Not sure:
- After a lost lock, the window's verdict is not recorded (the abort comes after the wait, before `verdict`); fin still puts back, and fin may still restart lamp-voice (lib.sh's rs, only with a restart-guard mute) under the other session's lock. The lamp-side put-back can still make a counted reboot when it finds no LAN (st.sh's own, unchanged).
- A renew that cannot ask the lamp (exit 3) before a window or after a wait also aborts (no further arm) though the lock may still be ours.
- A QUIET after a release that cannot reach the lamp still says FREE (no holder known).
