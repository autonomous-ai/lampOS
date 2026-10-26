# Reset command bounds: next-piece recommendation

Audit only, 26 October 2026. No task claimed, source changed, compilation, tests or device actions. Current source inspected: `2aae46323701bf5b66c7627c88c9abaa6378a612`; supporting runner: `258b2a9f105d0ccdb3b4b3f0dc386e24fe01fcc0`.

Recommend a narrow follow-up to #3579: bound subprocess output and ownership while preserving the delivered reset policy. Do not impose a shared 60-second stop deadline. A safe universal stop deadline cannot be established from repository configuration alone.

## Ownership snapshot

Private open/all-state PR inventories and the summarized `ownership-scan.json` record the ownership check across 592 comments on Queue #3504 through 2026-10-26 15:39:51 UTC (22:39:51 +07). No competing open reset command-bound follow-up was found. This is a snapshot, not a task claim. #3583 is closed and its supporting runner was handed to #3579; #3579 retained command bounds as a P3 follow-up.

References: [supporting branch handoff](https://github.com/autonomous-ai/lampOS/issues/3504#issuecomment-6415499372), [retained P3](https://github.com/autonomous-ai/lampOS/pull/3579#issuecomment-6416056880), [source closure](https://github.com/autonomous-ai/lampOS/pull/3579#issuecomment-6416122554).

## Complete product-unit scan

`product-unit-budgets.json` enumerates all 46 product service/socket/timer/path/target files, exact directive lines and the 35 installed exit-note recipients. A missing service timeout is an unknown effective manager/default/drop-in budget; sockets, timers, paths and targets do not acquire a service TimeoutStopSec merely by appearing in this inventory.

| Source under lampOS/ | Declared stop work or budget |
| --- | --- |
| deploy/product/lamp-body.service:46-49 | Mixed kill mode, SIGTERM, 25s; preserve standing-lamp fold time. |
| deploy/product/lamp-voice.service:41 | 5s. |
| deploy/product/lamp-records.service:16 | 5s. |
| deploy/product/lamp-setup-wifi@.service:29-33 | ExecStopPost wifi-restore, TimeoutStopSec=2min. |
| crates/setup/src/wifi_change.rs:157,463,477 | Restore allowance 90s; unit comments allow a 55s command and query/poll slack. |
| deploy/product/lamp-setup.service:26 | ExecStopPost systemctl stop nginx; no explicit service stop timeout; vendor nginx unit is outside this product inventory. |
| deploy/product/lamp-setup-wifi-restore.service | Boot restore oneshot; no explicit stop timeout. |
| deploy/product/exit-note.conf:10 | Installed ExecStopPost exit-note hook, including failure paths. |
| crates/update/src/install.rs:260-306 | EXIT_NOTE_UNITS applies that hook to 35 units. |
| crates/cloud/src/main.rs:48-87; src/reports.rs:173,587 | Exit-note metadata command has a 2s bound; this does not bound all note-file I/O. |

These are the only four explicit TimeoutStopSec declarations in the product unit set. No literal ExecStop directive was found. Update apply/back TimeoutStartSec=60 and selftest TimeoutStartSec=420 are start budgets, not stop allowances. No repository manager DefaultTimeoutStopSec override was found in the scanned configuration. RuntimeMaxSec is likewise not a cleanup allowance.

A 60s shared limit can interrupt legitimate Wi-Fi cleanup before its declared 90s restore or 120s unit allowance. Body alone fits 60s, but discovery, ordered stop phases and appended hooks consume additional time. The reset marker can shortcut some restore paths; it does not prove that already-running cleanup or custom-journal paths always finish within 60s. [systemd documents](https://www.freedesktop.org/software/systemd/man/252/systemd.service.html) stop command/service timeouts and effective manager defaults; that documentation is not proof of a particular installed unit configuration or version.

## Narrow reuse boundary

Current `crates/setup/src/reset.rs:290-299` uses unbounded Command::status/output. Keep its current `stop_and_wait` ordering at line 447, starter/writer lists at 534/554, stop-failure journal/reboot handling at 655, and recovery semantics. In particular, resume executes inside Setup and must not adopt a stop list that stops Setup itself.

The preserved `crates/setup/src/reset/systemd.rs:16` already sets STOP_LIMIT=180s, explicitly allowing Wi-Fi cleanup. Its reusable portion is the runner, not its whole alternate membership/stop policy:

- Lines 342-360 own the child process group and kill/reap it on error. Killing the CLI does not cancel a systemd job already submitted.
- Lines 375-430 use null stdin, piped nonblocking stdout, a 64KiB cap, one monotonic deadline, EOF plus child exit, and cleanup of descendants retaining stdout. The leader remains unreaped until EOF to preserve group identity.
- Lines 919 onward exercise deadline/output overflow and a descendant retaining the pipe. Unit-count policy (256) and broader discovery rules are separate from this runner reuse.

Start with bounded reads and the owned runner, then attach stop-command deadlines only with an explicit budget contract. The preserved 180s design is a candidate for the declared product paths, not a universal guarantee: serial phases, default timeouts, vendor nginx and effective drop-ins remain relevant. Preserve an operation deadline across repeated discovery/verification rather than resetting it per read; distinguish query and stop allowances. Refusal/timeout must retain the journal and avoid wipes. Do not silently shorten legitimate cleanup to meet an arbitrary limit or replace the delivered reset policy.

## Evidence needed for that follow-up

Retain existing helper, update-back and Wi-Fi recovery controls. Add deterministic runner controls for capped output, timeout, nonzero/spawn failure, pipe-holding descendants and owned cleanup. Demonstrate legitimate simulated cleanup exceeding 60s but inside the chosen stop budget, and cumulative phases consuming their original deadline. Verify timeout leaves journal/data intact and follows current failure handling. Inspect effective unit/default/drop-in budgets before claiming an installed stop bound. Native ARM64 and installed acceptance remain separate; this audit provides neither. Kernel-blocked spawn/reap or filesystem work is not proven hard-wall bounded by a polling deadline.
