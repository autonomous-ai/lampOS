# Records release-pair fixture: native artifact execution pending

Part of #2809. Fixed pair: rollback floor **6408**, current **6449**. This change adds only native Linux/root/systemd integration tests and CI acquisition/execution support. Records retention, process visibility, store checks and Voice runtime are unchanged. No native execution, successful cleanup, fixture producer layout, rotation, physical/device acceptance or signature-trust pass is claimed by this document. The first actual qualified run must record exact source/artifact/test identities and outcomes in the PR; failure is not a skip or a reason to weaken the guard.

## Three explicit actual-artifact controls

`crates/records/tests/release_pair_systemd.rs` contains three ignored tests:

- `records_6408_prunes_6449_private_sessions_and_preserves_live_invocation`.
- `records_6449_prunes_6408_private_sessions_and_preserves_live_invocation`.
- `records_6408_refuses_6449_shared_layout_and_6449_can_resume`.

Generic nonroot/permission-sensitive workspace tests do not run these. The dedicated `.github/workflows/lampos-records-pair.yml` workflow explicitly verifies/selects this binary's three ignored names, then invokes only it under root, serially. Missing environment, nonnative architecture, nonroot, missing real PID1/systemd/proc visibility, absent `openat2`, unexpected actual report or producer startup failure is a failed explicit test, never a successful early return. No fake proc, PATH-substituted manager, injected live set or runtime bypass exists.

Required absolute artifact variables: `LAMP_PAIR_6408_RECORDS`, `LAMP_PAIR_CURRENT_RECORDS`, `LAMP_PAIR_6408_VOICE`, `LAMP_PAIR_CURRENT_VOICE`; CURRENT is pinned to **6449**, not the newest release. The fixture rechecks these exact SHA-256/size/ELF64 little-endian AArch64 payloads and verifies them again after a successful case:

| Part | Release | Bytes | SHA-256 |
| --- | --- | ---: | --- |
| Records | 6408 | 1,393,272 | a1839349ca730d0051033bf37a43238dfc0deb2a32184e600893424724722031 |
| Records | 6449 | 1,406,976 | b18095bc74064e9949d9ed16c8f3c9a4c8c46e35892d3922800cf4aea8bb668f |
| Voice | 6408 | 28,375,944 | af7cd0d1e384ebddcca2b58eea94176c80aba573a5f47f4cd95cc8ce1a7ad084 |
| Voice | 6449 | 28,372,464 | 21783ba5fd6654d96164f7426dc27f2c72f06a074ce91fe06b256feec697c6da |

6408 source is `ee53765c8d014eb00e41c1a5a7054d2546c18bdb`; 6449 source is `be16c3327524ccef8a1908a3b8ebf40c16efba8a`. Acquisition separately verifies release API asset/digest and decoded manifest source/part bindings. These facts do not establish trusted-key signature verification or authorize installing an update.

## Real producer and ownership boundary

Both selected Voice artifacts support a continuous service which starts metadata while durably software-muted. Actual `serve` receives a private dummy provider path, a valid version-1 muted record and private controls bus. No unmute publisher, credentials, provider/audio/privacy/camera worker, product feature, host device or lamp is supplied. The existing software gate is checked before starting workers. A real transient **Type=exec** service uses the exact Voice executable, literal `ROOT/${INVOCATION_ID}` loaded argv and real systemd-expanded process argv/cgroup. A real metadata commit is the startup milestone: an initial service_segment header and service_sequence=1, with private run_start mode=serve or the shared service_event projection, plus unfinished/nonfailed writer health. The persisted written-event count may still be zero because both releases write first-event health before incrementing it; the actual event bytes establish the commit. This is not listening readiness.

Voice first requires genuine inactive/absent `hal.service`, `os-server.service`, `led-boot.service` and `led-shutdown.service`. The fixture checks those fixed real `systemctl is-active --quiet` results (3/4), and the unchanged release binary repeats the check. It never stops any of these units. The retained rootless 6408 probe failed before metadata with “cannot verify legacy service ownership”; no 6449 probe followed. That is failed producer evidence, not a cleanup/rotation pass, and no rootless retry/bypass is proposed.

Each case creates a closed actual producer session A, then keeps another actual muted service B alive on the same root. Actual opposite-release Records is configured with B's real unit. Its unchanged double host `/proc` inventory and systemctl snapshots must establish active protection. A size limit of one byte makes A eligible without editing producer timestamps or contents. Dry run must leave the inventory unchanged; apply must physically remove only A with exact allocated-byte accounting, retain B's object identities/modes and preserve explicit unknown-layout/symlink/outside controls. Snapshots use O_NOATIME; live event/health contents may advance but retention may not replace those objects.

Both releases truthfully return **exit 1** for the successful warning case because deliberate invalid-layout siblings remain and the protected live session can stay above the tiny limit. The harness checks complete report errors, active/planned/removed IDs and exact bytes, not CLI-zero alone. A root-level failed guard/report is not successful deletion evidence. Shared layout uses actual 6449 writer grant on a prepared 0710 parent/0750 sessions root and existing root/GID0 group; no host account or ancestor mutation. Grant fallback to private fails the shared fixture. Actual 6408 must return **exit 2** with zero deletion and unchanged shared inventory; actual 6449 must then resume cleanup with B protected. This demonstrates safe old refusal, **not** successful shared retention during rollback. That policy limitation stays explicit.

## Bounds and cleanup

The explicit native invocation is bounded externally at 600 seconds plus a 15-second kill ceiling. Each case has an original 150-second work ceiling; Records commands receive at most 60 seconds within it. Manager queries have a 3-second ceiling; start plus metadata polling shares one original 10-second deadline. Owned service normal stop has systemd TimeoutStopSec=8s, a bounded command and final MainPID/ControlPID-zero, no-job and cgroup/state verification; failed stop escalates only that named unit. RuntimeMaxSec=180s is a backup. All command output/drains are bounded, partial failures are retained, and each command uses its own process group with bounded kill/reap on timeout. An exited command leader stays unreaped until both pipes close, preserving its PID/PGID for timeout signals. After reaping, cleanup only probes group absence; uncertainty fails cleanup and cannot be erased by diagnostic compaction.

Required `LAMP_PAIR_OUTPUT` is an absolute canonical root-owned 0700 evidence directory. Before **each** unique reserved unit starts, the test atomically persists 0600 `owned-units.json`: `{"version":1,"units":["lamp-records-pair-<pid>-<case>-<n>.service", ...]}`. It retains at most 24 exact names and never sweeps a prefix. Rust cleanup handles ordinary failures/panics; CI always-cleanup uses only that ledger if external cancellation prevents destructors running. It requires unified cgroup v2 and either absence of the exact owned group or both empty `cgroup.procs` and `populated 0` in its mandatory `cgroup.events`, with stable manager snapshots before and after. A v1 host or missing descendant-population proof fails qualification. Cleanup failure fails qualification and retains the fixture. There is no silent target/service release on unverified cleanup.

Each test writes a terminal JSON named for its exact test and prints `release_pair_case` with name/result/cleanup. Evidence contains bounded real command prefixes with full byte counts/digests, parsed actual reports, synthetic session inventories/bytes, artifact binding and cleanup outcome. JSON files are capped at 1 MiB; command diagnostic logs retain at most 128 entries with 2 KiB per stream prefixes and explicitly mark possible compaction. No session files, executables, credentials or private words are exported as CI evidence. Actual systemd/process responses contain only disposable fixture/runner data.

## Evidence not supplied by these tests

The real writer pre-creates events-0..3 and health descriptors, but a short muted invocation generally fills only slot 0. Actual sizes/generation/rotations are recorded; four file names do not prove four populated streams, wraparound or sustained throughput. Existing writer rotation/records-store tests remain distinct. These tests do not measure acoustic/device latency, qualify power-cut/flash durability, exercise Memory/updater/task continuity, prove dedicated-reader DAC, deliver cross-version bus reports or recover an actual interrupted cleanup. They do not replace the attended whole-lamp acceptance row. No clock/proc/systemctl substitutions or weakened old-reader permissions are permitted to make them pass.

## Host checks and dedicated CI

On source tree `e6c42840b8ff32a15235d1cea00dc4fbec692a3b`, based on `59c4ac795`, Linux formatting and strict Records all-target Clippy passed. Ordinary Records tests passed **187 / 0 / 5 ignored**; the three explicit systemd cases are ignored in that run, so these counts are not release-pair execution. The same source also passed the Linux workspace build. After carrying onto `9ed44f88c`, candidate tree `e49f75850de132b12e9f2867450d0d85c6e2afeb` passed formatting and strict Records all-target Clippy on both Mac and Linux, the Linux workspace build, and ordinary Mac Records tests **182 / 0 / 1 ignored**. All 10,323 tracked files retained their bytes, modes and mtimes through each final gate; owned process/container cleanup was confirmed. The original Linux **187 / 0 / 5** result remains bound to its original tree, with exact parity of 104 Records/dependency/manifest files, including all five Rust fixture files, verified on the carry. The three Linux-only artifact tests are not compiled into the Mac target. This final README-only result update does not change any executable source. Exact-head native ARM64, Clippy and the dedicated three-case artifact job are still required before readiness.

The dedicated CI uses a native Ubuntu ARM64 VM, not the rootless local container that failed legacy-owner verification. The ordinary runner compiles exactly the current integration-test binary; a clean root environment executes its exact three ignored names. The shell/jq helpers bind six public assets to the pinned releases, check the selected executable and all three terminal receipts, retain partial failure evidence, and clean only journalled owned units. The native job is additional to the usual ARM64 and Clippy gates.

The helper's **52 offline controls passed**, including missing/wrong/duplicate case receipts, artifact/source mismatches, failed cleanup, missing/active control PIDs and manager jobs, live cgroup descendants and incorrect Cargo artifact selection. They do not execute release binaries, Cargo, systemd or a lamp. Two shell files and nine workflow run bodies passed syntax checks. There is no committed Python helper.

The first dedicated workflow attempt, [40057441482](https://github.com/autonomous-ai/lampOS/actions/runs/40057441482), failed during workflow validation and started no jobs or artifact cases. The job-level environment incorrectly used `runner.temp`, which is unavailable in that context. Temporary paths are now set from `RUNNER_TEMP` inside the first runner step and exported through `GITHUB_ENV`. The failed attempt remains historical; YAML and shell syntax checks alone had not validated GitHub expression-context availability. The context restriction is documented in [GitHub's context availability table](https://docs.github.com/en/actions/reference/workflows-and-actions/contexts#context-availability).

Pinned actionlint v1.7.7 rejects both original job-level expressions and accepts the corrected workflow; all nine embedded Bash bodies pass `bash -n`. ShellCheck and Pyflakes integrations were disabled for this actionlint check; the separate shell/jq controls above remain unchanged. No Rust fixture or helper source changed in this correction.

The corrected workflow started a native job in [40058164064](https://github.com/autonomous-ai/lampOS/actions/runs/40058164064), then failed host preflight before acquisition, compilation or any artifact case. Its retained files prove Linux AArch64 and an ordinary UID 1001 runner, but do not identify which later prerequisite failed. PID1 namespace inspection now runs through the already-required noninteractive root boundary, with checked command statuses and retained PID1/namespace/cgroup results. The same systemd, matching namespace and unified-cgroup requirements remain; this does not claim the original cause or an artifact pass.

The independent Rust-cleanup P2 on the original fixture is being corrected: empty parent `cgroup.procs` alone did not prove descendant exit before removing the fixture. Rust cleanup now requires unified-v2 proof and either the absent exact owned group or empty direct members plus exactly one `populated 0` event, rejecting missing, invalid, oversized or linked evidence and unexpected group paths. Six ordinary Linux controls cover live descendants, genuine-empty/absent positives, malformed/missing proof, non-v2 absence, links/path mismatches and bounded reads. These controls use disposable files, not fake systemd execution or changed production guards. They are excluded by `--ignored` during the separately fixed three-case artifact run; the summary accepts the resulting filtered count while still requiring exactly three passing artifact cases and all exact terminal receipts. Fresh gates for this Rust change are pending; earlier ordinary counts remain historical.

Run [40058378617](https://github.com/autonomous-ai/lampOS/actions/runs/40058378617) completed all three native cases before the requested cancellation landed: command exit 0, **3 passed / 0 failed**, 45.44 seconds inside the Rust runner; external cleanup confirmed all six owned units and the before/after source check passed. GitHub overall status is cancelled, and the Rust cleanup P2 still prevents readiness on that source. Its host diagnostics prove real systemd PID1, matching namespaces and unified v2; the ERR trap also recorded four expected inactive legacy-service statuses as misleading failures. Those expected outcomes now use an explicit conditional, preserving the same accepted statuses 3 or 4. The corrected Rust fixture still needs fresh native execution.

The first correction candidate `ec1b3fbf2` passed Mac formatting, strict Records Clippy and ordinary tests **182/0/1 ignored**, with unchanged source and confirmed cleanup. Linux formatting passed, but strict Clippy rejected `valid_id` appearing after the new test module; its full Records tests and workspace build did not run. That failed attempt is retained. The function is now moved before the test module without changing its body or any assertion; fresh corrected-source gates follow.

The corrected source tree `b839e645d480a40fac2d9c9e53b76864c943c779` passed fresh formatting and strict Records all-target Clippy on Mac and Linux, Mac ordinary Records **182/0/1 ignored**, Linux ordinary Records **193/0/5 ignored**, and the Linux workspace build. All six new cgroup controls passed exactly once; the three actual artifact cases remained ignored in the ordinary Linux run. Every one of the 10,323 tracked files retained bytes, modes and mtimes through both gates, and all owned process/container cleanup was confirmed. This result-only README update leaves executable sources unchanged. Fresh exact-head ARM64, Clippy and the dedicated native three-case job remain required; the cancelled historical native pass does not qualify this correction.

## Cap-specific ordinary control follow-up

The existing `oversized_evidence_refuses_cleanup` control now also calls the
bounded text reader directly: exactly 4,096 and 65,536 bytes must be returned,
and each limit plus one byte must refuse. These assertions isolate the byte
limit from cgroup event parsing, so malformed event content cannot satisfy the
cap check. All six existing controls and every runtime cleanup guard remain.
This is a test-only strengthening. Frozen tree
`198e6784abb22eb0377606388ccd76cdcc4e8d07` on `2e2eee360` passes fresh
Mac and offline Linux AMD64 formatting, strict Records all-target Clippy,
full Records tests and workspace build: Mac **182 / 0 / 1 ignored**, Linux
**193 / 0 / 5 ignored**. All six Linux cleanup controls pass exactly once,
including the strengthened bound control; all three actual-release cases
remain ignored in the ordinary Linux suite and are absent on Mac.
Each host invalidates exactly the 46 validated local workspace packages,
then proves all 46 freshly compiled across Clippy/build, with unchanged source
and confirmed process/container cleanup and target release. The two carried
feature blobs remain byte-exact to the original `5328b0dce` proposal after
rebasing. This final README-only result update changes no executable source.
Raw checks are in
`/private/tmp/fourth-records-cap-gates-20261024/revision3/{mac,linux}/attempt1/`.
Current PR ARM64/Clippy and its dedicated native three-case artifact run
remain required. No new artifact execution, systemd, physical-lamp or
power-loss result is claimed by these ordinary local counts.
