# #8671 reset row on lamp-75c9: hand-off for the lead

This is El's kit for the factory-reset row of PR #8671, at head `cf7dacac6dd32a1e791919300e19912470ce8c68`. It's run from your laptop. It holds only scripts: you fetch the PR's own CI ARM64 binaries with `stage`, which checks each sha256 against the artifact's `SHA256SUMS`.

## What you need on the laptop

- `zsh`, `python3`, `sshpass` (the kit calls `sshpass -p "$LAMP_PASS" ssh …`; with key login any value works), `gh` logged in, `base64`, `tar`.
- A lampOS checkout at any branch (`GITREPO`). The kit reads files at the PR head with `git show`/`git grep`, and fetches the head if missing.

## Settings

```sh
cd rows/8671
export GITREPO=/path/to/your/autonomous-os   # the repo root, which holds lampOS/
export LAMP_HOST=orangepi@lamp-75c9.local
export LAMP_PASS='<the lamp login password>'
```

## Steps, in order: post the printed output of each step

1. `./run.sh check`: laptop only. Expect `CHECK: OK`.
2. `./run.sh stage`: laptop only. It downloads `lampos-arm64-cf7dacac6d…`, then checks the sha256s, the binary's strings and the plan. Expect `lamp-setup: OK`, `lamp-voice: OK`, `reset-plan.json: OK` and the plan line `64 paths; … True`. El's staged sha256s are:
   - `lamp-setup` `b9013ba1d78c47964894b63d1f9fd4cbebcb440fbe50c97b9ccb0780e5c10c69`
   - `lamp-voice` `c9e7fcd14101b18b4b3e97b4b1f03d059ddcd4ba9d579a9d19397743786d8d78`
   - `reset-plan.json` `07bc294005f1f8a3868eabbafc6c2fef1a4c5119d172124316f7d6eb3ed64360`
3. `./run.sh preflight`: read-only on the lamp. Expect `PREFLIGHT-OK`. It refuses unless the hostname is `lamp-75c9`. It also prints which interface SSH uses.
4. `CONFIRM_RESET=lamp-75c9 CLEAR_OK=1 ./run.sh go`: this is the real run.
   - **Install:** the PR's `lamp-setup`, `lamp-voice` and plan go into a `/run` tmpfs under `/root/el-8671`, with a 40-minute dead-man first. Nothing is written under `/etc` or `/opt`, and lamp-update is never touched.
   - **Leg 1, dry runs:** `lamp-setup check --plan` (the PR's plan, then the lamp's own), and `forget-agents`, `forget-engine-homes` and `forget-logs` with `--dry-run`. Each must exit 0 and change nothing. The two new commands must refuse without `--dry-run`. A miss stops the run here, with no seed and no reset.
   - **Leg 2, the seed:** harmless kit marker files go into each engine home, `voice/marked`, and the old and new clip folders. The lamp-voice line counts in `/var/log.hdd` are taken, counts only.
   - **Leg 2b, the old clips folder:** 3 made-up clips go into `/var/lib/lampOS/voice/ring-clips`, and 2 into the new folder. The PR's lamp-voice is started once through a `/run` drop-in, with `LAMP_OFFLINE_RINGS=off` for this leg only. **PASS** is one `lamp-voice: the old offline ring clips folder was removed (3 files)` line, the old folder gone, and the new folder's names and sizes unchanged. The stock voice is then put back. `CLEAR_OK=1` allows this leg to delete clips already there; the reset deletes them anyway.
   - **The journal window and the kit's files** are copied to the laptop under `results/run-*` **before** the reset.
   - **Leg 3, the reset:** the product's own command, the same one the button's watcher runs. The lamp says "Factory reset starting." It starts setup in place, and **a reboot counts as FAIL**. The reset wipes Wi-Fi, so the lamp leaves your network by design. A recorder on the lamp keeps the after checks.
5. When the lamp is back on a network you can reach (after re-pairing), run `LAMP_HOST_AFTER=orangepi@<its address> ./run.sh after results/run-<the folder go printed>`.
   - **PASS** means every seeded marker is gone. The engine homes are empty with their owner and mode kept. The kept paths (day-tally, tts-cache, models, and so on) are still there. The text logs keep their inode. `/var/log.hdd`, both the text logs and its journal, holds **0** lamp-voice lines from before the reset.
   - The voice's own lines appearing again is judged once the voice runs. Until re-pairing, that leg reads **OWED** and the row reads PENDING.
6. `./run.sh judge results/run-<folder>` writes `row.md`. Post its first line and its table, along with the fenced `old offline ring clips folder was removed` line from the journal window.

## Captures

Journal lines are counted, and only the kit's own lines are quoted. **No camera.** Don't post the journal window itself, since lamp-voice's lines can hold a person's words. Keep it on your laptop. El writes the ROW from `row.md`, the step outputs and that one quoted line.

## If something goes wrong

- `./run.sh putback results/run-<folder>` copies the journal window off first, then removes the kit's `/run` files and drop-ins.
- The dead-man removes them by itself after 40 minutes.
- The lamp stays reset: that's the point of the row.
