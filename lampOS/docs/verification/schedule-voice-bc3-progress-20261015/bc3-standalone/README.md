# bc3 standalone host evidence

The clean baseline and Task31 source copy both use base `bc3fccb546a22baaef180fcd57b3291a44e0e4c1`. These are Third's host measurements on **Darwin 25.6.0, x86_64, rustc 1.96.0 (ac68faa20 2026-05-25)**. No lamp or provider was used. A later main-worktree rebase does not change this evidence's identity.

The permitted source-copy run passed **282 test executions, 0 failed, 2 ignored probes**, with all six Cargo stages completed. Its **72 default-off cases passed 144 exact stdout comparisons** against the independently archived clean bc3 baseline. This is a bounded standalone check; the complete affected-package gate has its own report and source identity.

| Attempt | Result | Completed and skipped work |
| --- | --- | --- |
| Clean bc3 baseline | PASS | Offline locked helper build, offline locked metadata, 72 cases / 144 stdout and 144 empty stderr readings. |
| First restricted source copy | FAILED: 16 passed, 13 failed | Workspace and helper builds passed; Actions stopped with 9 direct socket `PermissionDenied` failures and 4 readiness failures whose individual cause was not proven. Schedule, Voice reminder, Voice tools and default-off comparison stages were not reached. |
| Same source copy, local Unix sockets permitted | PASS: 282 passed, 0 failed, 2 ignored | All six stages completed. Actions 33/0/0; Schedule 160/0/0; Voice `reminder` 70/0/2; Voice `tools` 19/0/0. All 144 stdout comparisons passed; all 144 stderr readings were empty. |

The initial failure remains an independent failure record. The permission-only rerun used the same source and helper bytes, with no source correction, weakened assertion, or retry added inside a test. The four readiness failures are not relabeled as proven permission failures.

The [original baseline report](reports/baseline.json), [restricted report](reports/restricted.json), and [permitted report](reports/socket-permitted.json) retain their original paths and bytes. Their original raw output is under [logs](logs/); both offline Cargo metadata payloads are under [metadata](metadata/). The [retention index](retention-index.json) maps every retained original path to a relative file, length and SHA256. [Provenance](provenance.json) contains exact command arrays, working directories, timeouts, environment boundaries, machines, counts and omitted artifact hashes. The [final filesystem audit](reports/socket-permitted-filesystem-audit.json) verifies the completed source-copy report and raw results.

| Corpus | Source files | Source bytes, excluding helpers | Helpers |
| --- | ---: | ---: | ---: |
| Clean bc3 archive | 1,437 | 22,459,378 | Same 2 files / 1,755 combined bytes |
| Task31 source copy | 1,455 | 22,890,565 | Same 2 files / 1,755 combined bytes |

The helper figure is the sum of both helper files in each source copy, not the size of each helper. The original reports contain complete source hashes. The [baseline](source/baseline-file-map.json) and [implementation](source/implementation-file-map.json) maps also record verified byte lengths. The implementation uses 46 task overlays and the exact [676-path frozen Rust/manifest map](source/frozen-gate-map.json); 399 untracked nonruntime/evidence paths were excluded. Offline metadata resolves all 34 local crate manifests inside each respective copy. Build outputs, Git state and credentials were not copied into the standalone source tree. Tracked source fixtures/assets remained part of the original archive.

Both case sets contain 144 stdout readings totaling **562,392 bytes**, plus 144 empty stderr readings. All 288 stdout files and 288 empty stderr files are mapped in [comparison-readings.json](comparison-readings.json). Storage is deduplicated by exact bytes and SHA256 into three stdout payloads totaling **14,428 bytes** plus one zero-byte stderr payload; every reading retains its stage, profile, switch values, role, original path, retained path, byte length and hash. No output was normalized. The Actions fixture catalog's original path maps to the same retained Actions payload.

The matrix is 2 profiles (`ordinary`, `tasks`) × 6 values of `LAMP_SCHEDULE_TOOLS` × 6 values of `LAMP_SCHEDULE_SPEAK`: absent, `off`, `ON`, `true`, `1`, and ` on `. Exact lowercase `on` is deliberately outside the default-off matrix. Paired runs used the same absolute fixture paths. Invalid relative Schedule socket and ledger paths were not created. Ordinary mode declares 14 tools; Tasks mode declares 17 including upstream `check_agent_task`.

The unchanged [Actions helper](helpers/schedule_off_actions.rs.txt) captures the public catalog. The unchanged [Voice helper](helpers/schedule_off_voice.rs.txt) captures environment-derived arguments, declared tools, pure route choices for its fixed old-name list, and ordinary `Written` serialization. It does not directly route-check `check_agent_task`, compare new `End.sound` serialization, contact providers, or measure audio/motor/lamp behavior. These helper bytes are retained as text so the evidence adds no runtime Rust sources.

| Command stage | Clean/restricted first attempt, seconds | Permitted repeat, seconds |
| --- | ---: | ---: |
| Clean baseline helper build | 29.957836366 | — |
| Source-copy workspace build | 86.940103741 | 0.468332829 |
| Source-copy helper build | 28.383715155 | 0.250900679 |
| Actions tests | 55.706212 (failed) | 15.638498341 |
| Schedule tests | Not reached | 36.943134892 |
| Voice `reminder` tests | Not reached | 34.568786856 |
| Voice `tools` tests | Not reached | 2.362502126 |

These are host subprocess wall times; permitted builds reused the same isolated target cache. They are not clean build comparisons or lamp response times. The exact unrounded values and commands remain in the original reports. The two ignored reminder probes were not run, and filtered suites may overlap: 282 is a test-execution total. This standalone Voice command used the broader `tools` filter (19 passes). The separate focused main-worktree command used `tools::` (18 passes); they are distinct check scopes.

Historical 7f/dcc results and raw audit assertions remain in the parent verification folder. No older evidence is overwritten or promoted to the newer source identity. No compiled binary, source archive, target tree, Python script or file ending in `.rs` is retained in this folder.
