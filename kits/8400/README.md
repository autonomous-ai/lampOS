# #8400 row kit, legs 1 and 2, on the climb rig (for El)

PR #8400 (for #7018, the first builder's go at 16:59 on 23 Nov): an up-to-date round adds the product units that
the lamp's installed build carries but that the older updater which switched to it did not. It does this once per
build (`adds_tried` in `state.json`). The row's legs:

1. A lamp on a build without a unit takes a build that adds one, through an **older updater's** switching round
   and then a self-switch. The next up-to-date round adds, enables and starts the unit, with #7018's journal lines.
2. Owner-deleted and owner-edited units aren't touched.

Leg 3 (the gate's two real rounds) is not in this kit.

**Not run yet.** Nothing here has touched a lamp, the rig, podman or cargo. Every script passes `sh -n`, and
`mac/judge.py` and the systemctl stand-in were tried on synthetic files only.

## The bench: the climb rig, and how this maps to it

Eighth hasn't posted their own climb steps on #8353. The rig as written there is the first builder's 10:59 recipe
(#8353, from #3689's rig), and this kit follows it:

| The recipe (#8353, 10:59) | This kit |
|---|---|
| The shared Mac's podman VM `lampos-arm64`, amd64 with qemu, `--platform linux/arm64 docker.io/library/debian:bookworm` | `run.sh up`: `$PODMAN run -d --name $RIG --label el-8400=kit --platform linux/arm64 debian:bookworm sleep infinity` |
| "Use your own container name and volumes" | The container is `RIG` (default `el-8400`). It has no volumes; files go in by `tar` through `podman exec -i`. **The "lamp alias" is `RIG`, with `PODMAN`** (`PODMAN="podman --connection lampos-arm64"` when the VM isn't the default connection). |
| systemd as PID 1 segfaults: a logged `systemctl` that answers like an idle lamp; `lamp-update-back.timer` enabled+active+waiting with `Unit=lamp-update-back.service`; `-back`/`-apply`/`-writer` static, real FragmentPath; starts of `-apply`/`-writer*` run their ExecStart; never start a part; stub `journalctl` and `systemd-tmpfiles` | `lamp/stub/systemctl` (Python) and the three small stubs. **This is my reconstruction of that description, not Eighth's file**: theirs isn't in the repo or on #8353. If Eighth has one, compare the two before the run. Beyond the recipe: `lampos.target` reports active (needed for `start_added`), and a part that was started reports `active running` with a fake PID. |
| `libasound2`, `python3` | `lamp/rig-init.sh` (and `ca-certificates`, `procps`) |
| The stable public key in `/etc/lampOS/update/key.pub` (`54213f8d3aa2f241`) | `install-a.sh` writes the production key from `LAMPOS_UPDATE_PUBLIC_KEY` and checks its fingerprint before and after. **The test key is never written there** (#8369); every command gets it as `--key-file /var/tmp/el-8400/test.pub`. |
| The old build installed at `/` by its own `lamp-update install` | Build A's own `lamp-update install --channel-url file:///var/tmp/el-8400/chanA/ --channel lamp-voice --key-file …/test.pub --setup on`, as the imager's Phase 4 runs it at A's commit |
| The round: the old build's own unit line, `LAMP_UPDATE_SELF=on … once --apply on` | `round.sh`: `env <every Environment= of the installed lamp-update.service> /opt/lampOS/update/current/lamp-update once --channel-url file:///var/tmp/el-8400/chanB/ --channel lamp-voice --key-file /var/tmp/el-8400/test.pub --apply on`. These are the runbook's step 7 plus #8369's `--key-file`, run with the unit's environment (which includes `LAMP_UPDATE_SELF=on`). |
| Rounds a minute apart (GitHub refuses repeated big downloads) | Not needed: both channels are `file://` folders. Only A's install fetches its pinned models (~152 MB) from GitHub releases, once. |
| Remove the container and downloads afterwards | `run.sh clean` (only a container that has this kit's label), then remove `stage/` by hand |

## The build pair, and why

- **A = build 14976** (`969ff46eb2`, 20 Nov 01:13 UTC, stable for some lamps: lamp-9adf ran it on 23 Nov).
  - Its updater is level 13 (`manifest.rs:45` at that commit), the same level #8400's build asks for. So A's own
    updater makes the switch, and no climb happens. That's #7018's gap. A higher-asking B would make A climb, and
    B's updater would then add the unit in a normal switch.
  - Its `install::PRODUCT` lacks `lamp-npu.service` (b4976996, landed 20 Nov 14:45) and the apps units (355d6943,
    20 Nov 17:58). Both commits came after UPDATER 13 (65b98de7, 19 Nov 20:01).
  - Its CI artifact has expired. The kit takes its published release `lampos-lamp-voice-14976`: the same retained
    binaries, units and manifest that CI published, with the production signature checked on the Mac.
  - The real-world case: lamp-9adf went from 14976 to 15550 through 14976's updater on 23 Nov. #7018 says
    lamp-npu would only have come with its next switch.
- **B = #8400's head** (`85befe7280`), its ARM64 artifact `lampos-arm64-<sha>` (run 45154119749, run number
  **15704**, used as the sequence, as the runbook does). The non-development binaries are kept as
  `lampos-publish.py` keeps them. Its manifest is made by B's own `lamp-update manifest` with `deploy/`, `skills/`
  and `deploy/product/models.json` at the head, as CI does (models.json is identical at A and B, so nothing new is
  fetched). `MODELS=0` leaves models out, as the runbook does.
- **The unit judged: `lamp-npu.service`.** It's enabled (`install::enabled()`), `WantedBy=lampos.target`, not a
  one-shot, and runs as `lampos`, so `start_added` starts it. B also adds `lamp-apps-folders.service`,
  `lamp-apps-start.service`, `lamp-apps-manage.socket` and three templates. They're listed in the verdict, not
  required.

## Files

| File | Where | Does |
|---|---|---|
| `stage.sh <head sha>` | Mac | Refuses unless the sha is #8400's head now and the ARM64 release job is green. Checks the pair from the code (UPDATER levels, PRODUCT lists, #7018's line in B). `gh run download` of B's artifact, checked against its `SHA256SUMS`. `gh release download` of A, with its envelope checked by openssl and the production key, and every asset by size and SHA-256. Then A's `files_url` points at the rig's folder. `git archive` of B's `deploy/` and `skills/`. Makes a fresh Ed25519 test key (private half only in `stage/key/`). Writes `stage/STAGE.env`. |
| `run.sh <step>` | Mac | `up push make sign install snap leg1 leg2 judge putback clean`, or `go`. Results go in `results/<RIG>-MMDD-HHMM/`. |
| `lamp/rig-init.sh` | rig | Refuses unless aarch64, no systemd, no `/opt/lampOS`. apt deps; stand-ins into `/usr/local/bin`. |
| `lamp/stub/*` | rig | The logged `systemctl`, `journalctl`, `systemd-tmpfiles`, `systemd-run`. Every call goes into `/var/tmp/el-8400/calls.log`. |
| `lamp/mkchan.sh make\|check` | rig | B's manifest, by B's lamp-update. **Stops** unless B's `min_updater` ≤ A's 13, A < B, and `lamp-npu.service` is a product unit B has and A lacks. `check`: both envelopes carry the test key, the manifest bytes, the rig's `files_url`. |
| `lamp/install-a.sh` | rig | Production `key.pub`, A's install, then checks: installed 14976, `update/current` = A's lamp-update, its unit starts `update/current` with `LAMP_UPDATE_SELF=on`, the back timer enabled, no npu/apps units, `key.pub` unchanged. |
| `lamp/snap.sh save\|check\|restore` | rig | S0, the lamp as found. Keeps a tar of `/etc/systemd/system`, `/etc/lampOS`, `/etc/tmpfiles.d`, `/usr/local/lib/lampos`, `/var/lib/lampOS`, `/var/lib/lampos*` and the stand-in's state; a tar of `/opt/lampOS` outside the content-addressed `<sha256>/` folders, with every path listed; and a digest. **Rig only** (`rig_only` refuses anywhere without the stand-in): restore replaces whole folders. |
| `lamp/round.sh <name>` | rig | One round (above). It keeps `out/<name>/`: `journal.txt` (the process's `lamp-update:` lines and outcome, in order, then `EXIT n`), `state.json` before/after, new `log.jsonl` lines, `self.json`, `calls.txt` and `files.txt` (the links, the unit hashes, the wants links, the stand-in states). |
| `lamp/leg2-edit.sh` | rig | D1: `systemctl disable --now lamp-npu.service`, then the file and its link are removed and `daemon-reload` runs. E1: one comment line appended to `lamp-apps-start.service` (added by leg 1). E0: the same for `lamp-eyes.service` (which the lamp had on A). The edited bytes are kept. |
| `lamp/putback.sh` | rig | Refuses while a lamp-update runs. Restores S0, then reads back twice: the digest = S0's, `key.pub` = `54213f8d3aa2f241`, installed = 14976, no lamp-npu unit. `PUTBACK-OK`, or `PUTBACK-DOUBT` with what to do. |
| `mac/judge.py` | Mac | Prints every check below, then `LEG1 PASS/FAIL` and `LEG2 PASS/FAIL`. |

## How to run it (on the shared Mac with the podman VM)

1. Post the slot as usual. The rig needs no lamp hold, but say you're on the shared VM.
2. `SRC=<a lampOS clone with both commits> sh stage.sh $(gh pr view 8400 -R autonomous-ai/lampOS --json headRefOid --jq .headRefOid)`.
   **The head moves:** stage again whenever it does. The artifact expires with CI's retention.
3. `PODMAN="podman --connection lampos-arm64" sh run.sh go` (or the steps one by one). `go` runs putback itself if
   leg 1, leg 2 or the judge stops.
4. Post `results/<run>/verdict.txt` and the putback lines (`READBACK 1/2`, `PUTBACK-OK`). Then `sh run.sh clean`,
   and remove `stage/` (~0.5 GB).

**Duration:** about 25–40 min under qemu.

| Step | Time |
|---|---|
| up + apt | ~3 min |
| push (~470 MB) | ~2 min |
| make + sign (hashing under qemu) | ~3 min |
| install A (~152 MB of models from GitHub) | ~5–10 min |
| snap | ~2 min |
| r1 (fetch and place B's changed parts from `file://`, self-switch, confirm, the adds pass) | ~5–10 min |
| r2 + r3 | ~1–2 min |
| putback with two read-backs | ~3 min |

Rounds are bounded at 30 min each by `timeout 1800`.

## What is judged (lines matched exactly, `lamp-update: ` prefix)

**Leg 1** (normally all in `r1`; if r1 lacks the #7018 line, `run.sh` runs `r1b`, the next up-to-date round, and the
judge reads that one):

| Line / fact | Code |
|---|---|
| `build 15704 runs; lamp-update switches from its build <A's lamp-update[:12]> to <B's[:12]>, which must confirm itself` (A's round: a switch by the older updater, then its self-switch). After it, A's `once` `exec`s B's binary in its place. | 14976 `round.rs:316-321`; `main.rs:1457-1464` at 14976 |
| No `build 15704 needs updater …` (that would be a climb, not #7018's path) | 14976 `round.rs:322-331` |
| `confirmed: lamp-update runs its new build` | #8400 `main.rs:1046` |
| `build 15704, which the lamp runs, adds <…, lamp-npu.service, …>: the updater that switched to it did not carry them (#7018)`, after the confirm | #8400 `round.rs:1352-1355` (from `installed_adds`, called at `look()` `:1144`) |
| `started lamp-npu.service, added by build 15704`, after it | #8400 `round.rs:3513` with `:3480` |
| The round's outcome `up to date: build 15704`, `EXIT 0` | #8400 `round.rs:301` |
| None of `… was taken away …`, `… are not added …`, `putting build`, `could not be started`, `refused, nothing switched` | #8400 `round.rs:3623`, `:1329`, `:1367` |
| `lamp-npu.service` = B's manifest unit byte for byte; `lampos.target.wants/lamp-npu.service` there; `/opt/lampOS/npu/current` → B's `lamp-npu` sha; `update/current` = B's lamp-update; no `self.json` | `files.txt`, `updater.after` |
| `state.json`: installed 15704, previous 14976, `adds_tried` 15704 | #8400 `records.rs:147` |
| The stand-in was asked `systemctl start --no-block -- lamp-npu.service` | `world.rs:328` |

**Leg 2** (after D1/E1/E0, rounds `r2` and `r3`, each):

- `up to date: build 15704` and `EXIT 0`.
- No `which the lamp runs, adds`, `added by build`, `are not added` or `was taken away` line.
- `lamp-npu.service` and its wants link are still gone.
- E1 (`lamp-apps-start.service`) and E0 (`lamp-eyes.service`) are SHA-256 identical to the owner's edited bytes.
- `state.json` keeps installed 15704 and `adds_tried` 15704.
- No `systemctl start|restart|enable` of those units in `calls.txt`.

## Risks and limits (say them with the row)

- **It's the rig, not a lamp.** No part runs. A started unit is only marked active, so "starts" means
  `start_added` asked systemd (the stand-in). `ProtectSystem=` and the unit sandbox don't apply. The journal is the
  process's stderr, the same `lamp-update: …` lines a lamp's `journalctl -u lamp-update` shows. A real-lamp leg 1
  (52e6's done-when on #7018) is still owed.
- **The stand-in is reconstructed** from the recipe's text. If lamp-update asks it something it answers wrongly,
  r1 shows a refusal or a failed self-recovery check (`self recovery: …`), and the judge fails with the line. Read
  `calls.txt`, fix the stand-in, and `clean` + `go` again. Don't judge #8400 from a stand-in fault.
- **A's install may fail in a container** (it is 14976's install, run by hand, not the imager). It stops at
  `install` with the log in `out/install/install.log`. Nothing was kept yet, so `clean`.
- `r1` might not show #7018's lines because the exec'd B's first round wasn't up to date (for example a hold).
  Then `r1b` is the next up-to-date round, and the judge says which round carried them.
- A VM booted under 2 min before a round makes it hold (`the lamp booted N s ago`, `reset_hold.rs`).
  Wait and run the step again.
- **Disk:** `stage/` is ~0.5 GB on the Mac. The container is ~1.2 GB in the VM (channels, A's install, B's parts,
  S0). `clean` removes the container. The shared disk fills fast, so don't leave it.
- **The test key's private half** stays in `stage/key/test.key` (0600) on the Mac. It is piped to `lamp-update
  sign --key-file /dev/stdin` and never written in the rig. `key.pub` in the rig is the production key and is
  checked at install and at both read-backs.
- Not covered here (the PR's tests cover them): a failed watch or a cut-short of an adds pass, and lamp-region's
  own unit beside `OWN_UNIT`.
