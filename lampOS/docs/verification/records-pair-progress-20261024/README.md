# Records published-pair progress, 24 October

Ready for lamp-voice: no (#3329 is draft; native artifact cases and exact-head gates pending).

The three actual Records/Voice release-pair cases are published in [#3329](https://github.com/autonomous-ai/lampOS/pull/3329), currently `38a8d5651`. No production guard or retention policy changes. The five Rust files and three shell/jq helpers are unchanged by the two workflow repairs. Native execution remains separate from ordinary tests.

[Summarized terminal receipts](results.json) bind the current local Mac/Linux formatting, strict Records all-target Clippy and Linux workspace build to tree `e49f75850`. Mac ordinary Records passed 182/0/1 ignored. Original Linux Records passed 187/0/5 on tree `e6c42840b`; exact parity of 104 Records/dependency/manifest files was verified on the carry. All 10,323 tracked files retained bytes, modes and mtimes through the final gates, and owned cleanup was confirmed. Shell/jq controls passed 52/0. No original result is relabelled as native pair execution.

Run [40057441482](https://github.com/autonomous-ai/lampOS/actions/runs/40057441482) failed workflow validation: zero jobs, zero artifact cases. Job-level `runner.temp` was moved into the first step through `RUNNER_TEMP`/`GITHUB_ENV`. Pinned actionlint v1.7.7 rejected both original expressions and accepted the correction; its ShellCheck/Pyflakes integrations were disabled. Separate Bash syntax checks passed for all nine run bodies.

Run [40058164064](https://github.com/autonomous-ai/lampOS/actions/runs/40058164064) started a native ARM64 job but failed host preflight before acquisition. Its retained artifact proves Linux AArch64 and ordinary UID 1001; the exact later failed prerequisite was not recorded. The next correction retains each PID1/namespace/unified-cgroup prerequisite and performs namespace reads with the already-required root privilege. It preserves all requirements, adds checked command statuses and does not claim the historical cause. Actionlint, nine Bash bodies and the privileged inner body pass. Run [40058378617](https://github.com/autonomous-ai/lampOS/actions/runs/40058378617) has passed host preflight and all 52 offline controls and is acquiring artifacts at this entry; its actual cases remain pending.

Common's old Mac gate is also terminal: Voice 4,063/15/15, Eyes 327/0/5, Update 386/0/6, Skills 23/0/0. Linux Voice remains 4,131/8/18, Eyes 329/0/5, Update 388/0/4, Skills 23/0/0. Both hosts passed formatting, strict workspace Clippy and build. The corrected log parser verifies 266 required source controls once per host; it does not waive the full Voice failures. These bind the old frozen Common tree only; the new enrollment carry is still being resolved.

Memory's current-base Mac/Linux checks continue, while its unchanged actual-pair driver retains the separately published 2/0 result. No lamp test, physical latency result, power-loss qualification or build-row increase is claimed.


## Published correction and Memory PR, 10:56 +07

Records is now `6f79d33d6` on #3329. Both reviewed P2 findings are folded. The corrected candidate tree `b839e645d` passed Mac Records 182/0/1 and Linux Records 193/0/5, including all six new descendant/malformed-proof controls; formatting and strict Records Clippy passed on both, and Linux workspace build passed. Source and cleanup guards verify. The preceding candidate's Linux items-after-test-module failure remains historical; the unchanged helper body was relocated before the test module. Fresh dedicated native/ARM64/Clippy runs are active, and readiness still waits on those runs and renewed reads.

The [raw historical native receipts](historical-native-40058378617/manifest.json) retain the actual three passing cases and six-unit external cleanup on old `38a8d5651`, plus source identity/maps, host diagnostics and Root's audit. The run API is cancelled, and its old Rust cleanup defect is not waived by a successful normal run. The four misleading host-failure lines came from expected inactive service exit codes and remain in the raw record. The new workflow handles those statuses explicitly.

Memory is published as [#3333](https://github.com/autonomous-ai/lampOS/pull/3333), `4f8e95111` on current `9ed44f88c`. Current tree `69bf9af35` passed fresh full Memory 314/0/8 on both hosts, workspace formatting, strict scoped Mac Memory Clippy and strict Linux workspace Clippy/build. All 10,315 tracked files and cleanup verified before a README-only result update. Its actual release-driver result remains 2/0 on the unchanged direct driver, with identical published Memory payloads and retained original failures. Current native ARM64/Clippy and independent reads remain required; no lamp or row increase.
