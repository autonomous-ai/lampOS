# Updater process-cut evidence, 22 October 2026

This is part of [#2810](https://github.com/autonomous-ai/lampOS/issues/2810),
not closure of that issue. Seven actual SIGKILL cuts passed in Linux ARM64
processes under Podman. Each positive execution is **one checker with seven
internal cases**, not seven libtest tests. The attended physical unplug has
**not** been performed.

The latest completed local gates bind base
`f012c47259ec302138d8ee4f91860889eef6619a` plus the recorded test-source hashes.
Earlier e223 results remain historical. The subsequent rebase to
`5e51ac016c914cbb8464609a0907072755eca8fd` preserves all 34 bound updater and
workspace files, but changes other dependencies. **These local gates do not
qualify the full 5e51 integration; PR ARM64 CI must do that.**
[sources.json](sources.json) preserves the source/base distinction and the
three updater files changed between e223 and f012.

The final publication rebase also includes `8e274b5e394b09ef5e5f18cb0d2382fbaddec95a` (Voice-only
upstream changes after the 5e51 snapshot). The 34 bound source files remain
identical; qualification of the combined dependencies awaits PR CI.

The checker stops its owned child immediately after a successful directory
`fsync`, verifies the exact pending/link/refusal prefix, sends SIGKILL and
requires signal 9 when reaping. A fresh process recovers from the same files
and completes the next round. Selected current, retained old/new and fixed
fallback executables are checked independently. The harness introduces no
runtime updater changes or production hooks.

| Internal case | State at cut | Selected | Recovery |
|---|---|---|---|
| `candidate_synced` | Candidate renamed/synced, before made record | Old | Resume switch and confirm new |
| `pending_synced` | Pending saved before link replacement | Old | Put back/refuse new |
| `current_synced` | New link synced; pending remains | New | Confirm new |
| `confirmation_synced` | Pending removal synced | New | Already confirmed |
| `put_back_link_synced` | Old link restored, before refusal | Old | Finish put-back/refusal |
| `refusal_synced` | New refused; pending remains | Old | Finish cleanup, no duplicate refusal |
| `put_back_cleanup_synced` | Old selected, refusal retained, pending gone | Old | Back has nothing left to do |

All seven completed positive rows report `fsync exit=0`, signal 9, recovery
passed and next round `UpToDate(2)`. Runnable-current assertions occur before
and after recovery.

## Completed results and retained failures

- f012 ARM64 checker: **1 passed, 0 failed, 0 ignored**, seven internal cuts,
  **16.074 s command elapsed**. Strict Linux Clippy passed in **9.660 s**.
- Restored f012 nextest after A1–A7: **1 passed, 0 skipped**, all seven rows;
  **3.366 s test time**, **12.050 s command elapsed**. Source hashes match the
  positive f012 gate. This repeats the same checker, not seven added tests.
- f012 Mac gate: fmt **5.259 s**, strict Clippy **5.124 s**, tests **318.373 s**;
  **306 passed / 0 failed / 3 ignored / 0 filtered**. The Linux checker is one
  of those ignores. Separate listing checks prove default and ignored
  discovery plus ignored execution; they are not process-cut passes.
- Historical e223 ARM64 checker: **1 passed**, seven cuts, **27.099 s**;
  strict Clippy **21.515 s**. Historical nextest: **1 passed / 0 skipped**,
  seven rows, **3.267 s test / 13.450 s command**.
- First ARM64 run: seven cuts passed in **67.496 s**. Strict Clippy then failed
  after **33.727 s** with `clippy::zombie_processes`: the helper used raw
  waitpid without an explicit Rust Child wait. The helper changed to
  `try_wait`/`wait`; a second seven-cut run passed in **11.618 s**, then Clippy
  passed in **8.000 s**. Both histories remain in [linux.json](linux.json).
- Historical initial Mac gate: fmt **5.506 s**, strict Clippy **19.647 s**,
  tests **340.298 s**, **305 / 0 / 3 / 0**. Historical e223 Mac: fmt
  **5.106 s**, strict Clippy **14.226 s**, tests **332.638 s**, **305 / 0 / 3 / 0**.
  All three source maps were unchanged through their respective runs.

## Negative controls

**Sixteen controls compiled and failed at intended checker assertions.**
All 17 exact patches are retained, including unqualified T1.

- **A1–A7** remove old, new and fixed fallback fixture executables after the
  verified SIGKILL cut. They explicitly assert all paths, including selected
  current, are absent, log `zero updater executables remain`, then fail the
  unchanged exact-prefix check. All seven are qualified; combined command
  elapsed is **74.316 s**. They demonstrate rejection of a fixture
  with no working updater. They are destructive fixture inputs, **not
  production-ordering mutants or a demonstrated product defect**.
- Historical **R1–R7** remove only the selected current executable after the
  cut. Other updater copies and the fixed fallback survive. **T2** rejects a
  TERM delivery stop as a SIGKILL reap; **B1** rejects a missed barrier. These
  nine qualified controls took **91.824 s** combined.
- **T1 is unqualified**: TERM left its tracee stopped and the five-second wait
  expired before the intended assertion (**14.218 s command elapsed**).
- R3 initially expected `NotFound`, but removing the selected new binary
  failed the earlier exact-prefix assertion. R3/R4 classification now matches
  that observation. Original log and intermediate receipt hashes remain;
  no product fault or manufactured rerun is implied.

[controls.json](controls.json) preserves each exit, assertion line, completed
prior case, patch/mutant/log hash, exact normalized command and restoration.
All 17 patches reconstruct their recorded mutants in memory. This evidence
export did not mutate or execute the source.

## Commands and source binding

Inside the recorded ARM64 container, from `/work/lampOS`:

```sh
cargo test --locked -p lamp-update --test self-process-cuts -- --nocapture
cargo clippy --locked -p lamp-update --all-targets -- -D warnings
cargo nextest run --locked -p lamp-update --test self-process-cuts --status-level fail --final-status-level fail --success-output immediate
```

[linux.json](linux.json) and [controls.json](controls.json) retain exact
normalized Podman argument arrays. `CHECKOUT` and `NEXTEST_TOOLS` replace
private host paths; container paths remain literal. Source was mounted
read-only, with separate target/registry volumes, two build jobs, no
incremental compilation and debug level zero. Tracing used `SYS_PTRACE` and
the recorded seccomp allowance. [environment.json](environment.json) records
image/runtime identity. Dependency/toolchain fetches appear in raw logs:
these executions were not network-isolated. The fixture uses no live channel,
systemd or lamp.

[source-deltas.json](source-deltas.json) preserves the helper wait correction
and Mac ignored-list correction without whole source copies.
[mac.json](mac.json) retains all three completed Mac gates and listing proof.
[raw-inputs.json](raw-inputs.json) binds original receipts/logs, historical
source snapshots and control drivers by full SHA-256. Public JSON is compact
and normalized, not byte-identical to raw receipts. Raw logs and full image
inspection remain private. [SHA256SUMS](SHA256SUMS) covers every public file.

This proves killed-library-process recovery with executable shell fixtures.
It does not exercise installed-daemon exec handover, real systemd/timer
recovery, physical loss of power/kernel caches, or flash/fsync durability.
The [attended-unplug plan](../updater-attended-unplug-plan-20261022.md)
remains separate. These scoped runs do not qualify the full Linux workspace,
a future PR head, or the later 5e51 integration.
