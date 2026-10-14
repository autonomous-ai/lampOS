# Schedule voice standalone qualification on integration base 1337

This isolated implementation run passed **435 test executions, 0 failures and 7 ignored**, a workspace build, and **72 default-off cases / 144 exact cross-copy stdout comparisons**. All captured helper stderr streams are empty. These results are from Third's local host. The complete affected-package gate, format/Clippy, native CI and lamp qualification are separate parent-owned records; this subset does not replace them.

The clean integration baseline is `1337e7e1d8b88e102ec9fd12f544841569856f75`. The implementation was copied at task HEAD `d794cf2aca8fd5a7891af6363f2155e9fd3091fe` and its **700 Rust/manifest/toolchain paths** match frozen-map SHA256 `89adaafcd44a51bbd84233abc575dfdc55987499fcc3f1d427825d4829b885fd`. The map recorded the earlier task HEAD `f959aaa5e8b43b9ddea8d0031d0034272dcaa7af` plus the final Days regression before amendment. HEAD changes alone do not identify runtime changes; the exact frozen paths and copied final documentation were checked before and after execution.

The baseline has 1,661 source files / 27,159,406 bytes. The implementation has 1,679 / 27,611,889 bytes, with 46 overlays relative to the integration base. Its entire task verification folder was excluded, including 671 files observed at copy time. Neither copy imports the parent build tree. Metadata resolved 34 baseline and 34 implementation local package manifests, all contained within their respective source copies.

The full Memory package test command includes its default targets and doc tests because Memory develops against Voice. The singular Voice `reminder` filter includes `remember::tests::kept_out_reminder_is_not_journalled_by_days_but_next_exchange_is`; the original log and filesystem audit explicitly verify its passing result. That regression exercises the real Days client FIFO and a local Unix socket: the kept-out reminder is not journalled, while the next ordinary exchange is. The child test execution is covered by the parent test result rather than added again to these reported harness totals. The broader standalone Voice `tools` filter differs from the parent's focused `tools::` filter; counts are from each exact command.

## Commands and measurement boundaries

Host: `Darwin 25.6.0`, architecture `x86_64`, processor string `i386`; `rustc 1.96.0 (ac68faa20 2026-05-25)`. Offline locked Cargo used distinct copy-owned target directories. Local Unix sockets were explicitly permitted. No live provider, account or device was accessed. Elapsed command times use monotonic time immediately before process launch through exit and log closure; they exclude source/evidence audits and are not lamp latency measurements.

Baseline helper build: 30.191950346 seconds. Baseline metadata: 0.357460894 seconds. Implementation metadata: 0.471940153 seconds.

| Implementation stage | Exit | Seconds | Passed / failed / ignored |
| --- | --- | --- | --- |
| workspace-build-first | 0 | 83.879675077 | build |
| helper-build-first | 0 | 26.817668312 | build |
| actions-tests-first | 0 | 30.344349327 | 33 / 0 / 0 |
| schedule-tests-first | 0 | 34.691446607 | 160 / 0 / 0 |
| memory-tests-first | 0 | 167.240647263 | 147 / 0 / 5 |
| voice-reminder-tests-first | 0 | 35.288772879 | 75 / 0 / 2 |
| voice-tools-tests-first | 0 | 3.009404499 | 20 / 0 / 0 |

Exact Cargo commands, in baseline then implementation order:

```sh
cargo build --offline --locked -p lamp-actions -p lamp-voice --example schedule_off_actions --example schedule_off_voice
cargo metadata --offline --locked --format-version 1
cargo build --offline --locked --workspace
cargo metadata --offline --locked --format-version 1
cargo build --offline --locked -p lamp-actions -p lamp-voice --example schedule_off_actions --example schedule_off_voice
cargo test --offline --locked -p lamp-actions --all-targets -- --test-threads=1
cargo test --offline --locked -p lamp-schedule --all-targets -- --test-threads=1
cargo test --offline --locked -p lamp-memory -- --test-threads=1
cargo test --offline --locked -p lamp-voice --lib reminder -- --test-threads=1
cargo test --offline --locked -p lamp-voice --lib tools -- --test-threads=1
```

The metadata command runs after the implementation workspace build. Exact helper commands and environments are in [comparison-readings.json](comparison-readings.json). Original invocation records preserve runner command names and first output. No command was retried, no assertion was weakened, and no planned stage was skipped. Python source is outside this text-only export, with identities recorded separately.

## Exact default-off byte comparison

Each `ordinary` and `tasks` profile independently sets `LAMP_SCHEDULE_TOOLS` and `LAMP_SCHEDULE_SPEAK` to absent, `off`, `ON`, `true`, `1`, or ` on `: 2 × 6 × 6 = 72 cases. Actions and Voice helpers are invoked in both copies for each case. The same fresh baseline-owned catalog fixture path is used by both copies. Raw stdout bytes, including paths, are compared without normalization.

Each copy has 144 stdout readings / 562,392 bytes and 144 empty stderr readings. There are 3 distinct stdout payloads totaling 14,428 bytes. All 288 stdout and 288 stderr original paths are individually mapped with copy, case, profile, switch values, role, original and retained path, size, SHA256, command, timing and exit status. Identical originals are retained once after SHA256 and byte equality checks. Empty stderr is retained as one zero-byte file. Both metadata stdout/stderr sets and every current-run raw command log are retained.

The helper pair is unchanged: **179-byte Actions + 1,576-byte Voice = 1,755 combined bytes per source copy**. Actions SHA256: `8fc55e05703d1aa5ca58348132e3f4b0d7eed202e443f608f0d5af562c8f35e1`. Voice SHA256: `12d36f2d201b1117df21a55574855c8b44bcd2ec06f7b6fa980603e809b41695`.

The helper boundary is the public Actions catalog, Voice environment arguments and declared tools, a fixed existing-route list and ordinary Written serialization. Helpers do not start the Tasks service, directly route `check_agent_task`, serialize End.sound, or exercise live providers. The integration base includes the larger Tasks response limit; the helper does not execute Tasks exchanges. Missing Memory/Tasks fixture paths and invalid relative Schedule paths remain absent. Exact byte parity is confined to this captured corpus.

## Retained originals and limits

- [Completed clean baseline report](records/baseline-completed.json)
- [Completed implementation report](records/implementation-completed.json)
- [Execution filesystem audit](records/execution-audit-first.json)
- [Preparation audit](records/preparation-audit.json)
- [Original-path retention index](retention-index.json)
- [Helper, archive and executable identities](artifact-identities.json)
- [Referenced paths intentionally excluded](excluded-references.json)

The audit rechecked 3,649 historical and baseline originals. Mutable top-level prose and active parent qualification records were excluded from that immutable-history claim. Earlier intermediate successes and full-gate or restricted failures retain their original identities. The baseline-only audit's statement that implementation comparison was pending describes its earlier capture stage; this completed pair supplies the later result without rewriting that original.

Only UTF-8 text is exported: no Python, archives, executable binaries, source trees, fixture directories or credentials. Helper/source/archive/executable bytes stay outside the bundle and are identified by hashes. `SHA256SUMS.txt` covers every export file except itself, including the retention index. Each retained raw file was checked against the original byte-for-byte. The capture proves no behavior or timing on the lamp and makes no overall readiness claim.
