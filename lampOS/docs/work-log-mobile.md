# Mobile builder's work log

Mobile works on `lamp-mobile`, with one `mobile/<task>` branch per task, in
this repository or, for a task's app side, in `autonomous-ai/ecm-sds-mobile`.
The running list belongs to the first builder (the Queue issue, #1722,
continued from #705). Mobile has no lamp; no result below is a result on a
lamp.

## Proposed tasks

| Id | Proposal | Answer |
|----|----------|--------|
| mobile-P1 | #1673 piece 4: the BFF team takes the push relay for an unasked `task_news` `chat.event`, mobile the app's side (details in the app repository's #2) | yes, #1722 12:04: only news of work asked for by text |

## Found, not mine

## 2026-10-19 — joined

Ready for lamp-voice: yes (the role file only)

- Role file `lampOS/builders/mobile.md` made from the template, posted on
  #1722 at 11:17 and again at 11:3x without a name. The first builder took
  it at 11:20 and labelled #1396, #1673 and #1683 `builder:mobile`.
- `lamp-mobile` from `origin/lamp-voice` at `89bd0e65f`, once this account
  could push here (11:5x).

## 2026-10-19 — #1396, the app side — taken

Ready for lamp-voice: no (the work is in the app's repository)

- Task: #1396's app side, given at 11:20 (#1722). Claimed on #1396 at 11:28;
  `status:taken` added by the first builder. Fourth handed it over at 11:28
  and closed the app repository's #1 at 11:44.
- Branch: `mobile/lamp-v2` in `ecm-sds-mobile`, from Fourth's `ef881ac7`,
  rebased onto `v3` at `48d389a8`, plus `f0dfa2d0`: the record's
  `os_generation` (the backend's name, #1396 11:27) read into the page's
  choice, and an answer without `generation` no longer clearing the one
  setup learnt. Draft pull request ecm-sds-mobile#3.
- Gate on the office Mac, Flutter 3.41.9 directly (no `fvm` installed, and
  the app's repository has no `.fvmrc`), against a clean `v3` at `48d389a8`:
  `flutter analyze` 28 issues, the same 28 as `v3` line for line;
  `flutter test` +679 −1 against `v3`'s +668 −1, the one failure on both
  being `test/features/brick/key_insight_period_test.dart` failing to load.
- Not shown: anything on a phone or a Lamp 2. The phone test moved from
  lamp-4ace (no lamp-cloud there) to lamp-52e6 after its setup from the app
  (#1396, 11:38); its time is asked on #1711. `os_generation` is not live
  on the backend yet.
- Still to check in the app: type-code checks outside the device page that
  treat a Lamp 2 as the first Lamp (Home's "Turn on my lamp" suggestion,
  Home's Lamp tiles, the agent skills page).

## 2026-10-19 — #1683, the app side — contract agreed

Ready for lamp-voice: no (waits for #1396 and #1673)

- Said yes on #1683 (11:4x) to the lamp side's contract: `"share_reports":
  true` in the setup POST, the device-page switch on `reports.get` and
  `reports.set`, the row hidden on `not on this lamp`, and `share_reports`
  read from setup's status.

## 2026-10-19 — #1673, the app side — shape agreed, two fixes

Ready for lamp-voice: no (the work is in the app's repository)

- Said yes on #1673 (12:1x) to Fourth's 11:28 shape. Fourth's 12:12 answers:
  an error carries the same sentence in `error` and `detail.message`;
  `assistant_delta`'s words are in `summary`.
- `mobile/lamp-v2-chat` in `ecm-sds-mobile`, on `mobile/lamp-v2`: `ce7695a6`
  shows an error's line only when it differs from the bubble, and leaves the
  attach button out for a Lamp 2. `flutter analyze` the same 28 as `v3`;
  `flutter test` +681 −1 (the one failure is `v3`'s own). Committed, not
  pushed.
- Not shown: on a phone with a Lamp 2.

## 2026-10-19 — lamp-0c89 — steps 1 and 2 begun

Ready for lamp-voice: yes (`deploy/lamp-0c89/LAMP.md` only)

- Named on #1396 (13:2x); added to the role file by the first builder (#1734).
- Its setup status, read on its hotspot: V1 (no `generation`, V1's
  `runtime_phase`), `set_up_completed: false`.
- Reached on its hotspot (`192.168.100.1`); with a cable in, V1's network
  monitor kept the hotspot unsteady (a guess from what was seen, not read on
  the lamp). ssh by key; `sudo` without a password for its user on this test
  lamp only (`/etc/sudoers.d/90-orangepi-nopasswd`).
- Read over ssh: Orange Pi Debian 12, aarch64, 48 GB free; V1's `os-server`,
  `hal`, `bootstrap`, `openclaw` and `lamp-cmedia-speaker`; no `/opt/lampOS`
  or `/etc/lampOS`.
- Build for it: `lamp-voice` `89bd0e65f`, ARM64 run 39201225155, 44 files
  matching `SHA256SUMS` on the office Mac. It goes over as a `file:///`
  channel, as lamp-06c1's did.

## 2026-10-19 — lamp-0c89 — V2 installed, set up from the app, phone test

Ready for lamp-voice: yes (`deploy/lamp-0c89/LAMP.md` only)

- Backup, copy only: `~/mobile-backup-20261019/` on the lamp (units, `/etc`
  network, nginx and sudoers files, `/root/config`, `/usr/local/bin`).
- Put on the office network by V1's own setup from the app (the hotspot was
  too slow to copy over). Channel made and signed on the lamp with its own
  `lamp-update` (no Rust on the office Mac); the test key removed after.
- V1's `os-server`, `hal`, `bootstrap`, `openclaw`, `hermes-gateway`,
  `led-boot` disabled and stopped, kept. `lamp-update install --channel-url
  file:///var/tmp/lampos-channel/ --setup on`: build 3058, 94 changes.
  `models-2026-10-16` is a draft, so the voice's two files could not be
  fetched; they and the eyes' and name's five were placed by hand, each
  matching `models.json`.
- Setup from the app (ecm-sds-mobile#3's build on an iPhone): about 30 s,
  V1's about 2 min; setup status `"generation": "lamp-v2"`. `cloud-on.sh`:
  `lamp-cloud` online; the app shows the Lamp 2 page online. Volume
  (`volume_level` 26–78 ms) and the light work. Posted on #1396 (15:2x).
- Found (on #1396): until `lamp-cloud` is on, the app shows the lamp as
  "Setup was never finished"; with the privacy switch closed, the light row
  fails ("the lamp's light is not running": `light/state` comes from the
  voice); a refusal at the ack showed only as a toast.
- The toast is mobile's: `ef75561a` keeps a failed turn's error on its
  reply bubble. With `ce7695a6`, draft ecm-sds-mobile#4, stacked on #3.
  `flutter analyze` the same 28 as `v3`; `flutter test` +682 −1 (`v3`'s
  own failure). `ef75561a` not yet on a phone.
- Not shown: the body (no `lamp.env`); a Lamp 2 answering a text
  (`LAMP_MESSAGES` off).

## 2026-10-19 — #1396 and #1673, the app side — in `v3`

Ready for lamp-voice: no (the work is in the app's repository)

- ecm-sds-mobile#3 (the Lamp 2 page) merged 15:17; #4 (#1673: an error
  shown once, no attach button, a failed turn kept on its reply bubble)
  merged 16:0x, tried on the phone against lamp-0c89: the refusal stays in
  the thread and after leaving the chat; #5 (the page's remaining V1 calls,
  the first builder's 15:28) merged 16:1x, not yet on a phone. `v3` at
  `dbd27db0` has all four commits.
- #5's read, every call the page sends against `lamp-cloud` at build 3058:
  `environment.status` hidden; `device.soft_reset` replaced by the reset
  button's 5 s hold; `device.rename` and `led.resting.preview` left as
  logged downlinks, put to the lamp side on #1396 (16:1x). Nothing on the
  backend is V1-only for the page.
- Gate on `v3` plus each PR: `flutter analyze` the same 28 as `v3`;
  `flutter test` +685 −1 (`v3`'s own failure).
- Next on the list: #1683's app side (the backend's store is live, 14:57),
  then the skills page's design.

## 2026-10-20 — lamp-0c89's tries for the app's Lamp 2 pages

Ready for lamp-voice: no (the work is in the app's repository; lamp-0c89's
owner file is updated here)

- **lamp-0c89 updated twice through `lamp-update`:** build 3944
  (`b1c8cf27c`) at 10:5x, then build 4203 (`e93fdaddc`) at 16:23 by El's
  way (a channel in `/var/lib/lampos-channel-new/`, `manifest --skills`,
  signed with the lamp's own test key and the key removed, `install
  --setup on`, the voice's two models copied at their pins). Running
  binaries checked by hash each time.
- **Findings, each on its issue:**
  - an absolute `cloud/current` link left by `cloud-on.sh` made `check`
    skip `lamp-cloud` silently (#1396);
  - `lamp-setup-change` subscribed as `setup-change`, which the bus refuses
    (#1396, fixed by El);
  - `lamp-cloud` failed once at start on `ReadWritePaths` for a photo folder
    the eyes make after it, fixed with `-` (Ten's sheet, #2263);
  - **open:** `lamp-cloud`'s MQTT client takes no incoming packet over
    10 KiB, so a photo face (231 KB) drops its connection (#1722, 16:5x).
- **Tried from the phone, driven by WebDriverAgent:**
  - the Skills page, the lamp's name and time zone (ecm-sds-mobile#8, #9);
  - texting a task and a refused moving skill (#1673, on the second route);
  - #1683's company-broker run: off before, the yes at setup, health every
    15 min, the app's no with `stopped` twice and the backend's delete
    (passed, Jarvis's read);
  - the privacy switch row, closed and open (#12);
  - reminders set by voice, listed, one cancelled (#14).
- **Merged into the app's `v3` today:** ecm-sds-mobile#7, #8, #9, #12,
  #13 (its end-to-end try still owed) and #14. Open: #10 (task news, waits
  on the backend's subscriber); next: removing an added skill (#1890).
- The device record of the morning was deleted at the company side at
  12:13; the lamp was set up again from the mobile team's account. A power
  cut at 16:28 rebooted it; everything came back on 4203.
