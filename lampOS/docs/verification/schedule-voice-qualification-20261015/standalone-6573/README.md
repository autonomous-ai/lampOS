# Schedule voice final-policy standalone qualification

The isolated final-policy run passed with **287 test executions, 0 failures and 2 ignored**, plus a complete workspace build and **72 default-off cases / 144 exact cross-copy stdout comparisons**. Every captured helper stderr stream is empty. These are measurements on Third's local host, not the lamp. This subset does not replace the complete affected-package gate or native/device qualification, which the parent records separately.

The integration baseline is `6573ce6c1b594bf9c8cf7836bbd0e162c09e4cd1`. The implementation copy was observed at task HEAD `acb628b494df0c38842ea467d1289eec8d35d2ca`. Its 697 Rust/manifest paths exactly match frozen-map SHA256 `e0c5985563535bf46d6af76d3bae71634ab3f1f03d8c713d55f202f95972ca7f`. The frozen map originally recorded HEAD `168856fbd349a7931a962458f17509953b3a254d`; later checkpoint amendments did not change those runtime hashes.

The baseline contains 1,603 source files / 25,406,926 bytes. The amended implementation contains 1,621 / 25,853,029 bytes and 45 overlays relative to the integration base. Its entire task verification folder was excluded. Metadata resolved **34 local package manifests inside each respective copy**; neither build depends on the parent checkout. There are 35 Cargo.toml files in each source tree including the workspace root.

The original prepared implementation and audit are retained unchanged. Only `docs/schedule.md` changed for the amended copy: the disable runbook now explicitly runs `systemctl daemon-reload` and restarts Actions before Voice. No other non-evidence source file differs between the two prepared implementation copies. The [separate amendment record](records/documentation-amendment.json) records both identities.

## Commands and measured boundaries

Host: `Darwin 25.6.0`, architecture `x86_64`, processor string `i386`; `rustc 1.96.0 (ac68faa20 2026-05-25)`. Commands used offline locked Cargo and isolated target directories. Local Unix sockets were explicitly permitted; no device, provider or account operation occurred. Elapsed command times use monotonic time immediately before process launch through process exit and log closure, excluding source/evidence audits. They are not acoustic or end-to-end lamp latency.

Baseline helper build: 29.880328562 seconds. Baseline metadata: 0.407895886 seconds. Implementation metadata: 0.375174287 seconds.

| Implementation stage | Exit | Seconds | Passed / failed / ignored |
| --- | --- | --- | --- |
| workspace-build-first | 0 | 81.455834035 | build |
| helper-build-first | 0 | 26.076194209 | build |
| actions-tests-first | 0 | 30.060316488 | 33 / 0 / 0 |
| schedule-tests-first | 0 | 35.038902309 | 160 / 0 / 0 |
| voice-reminder-tests-first | 0 | 34.465421107 | 74 / 0 / 2 |
| voice-tools-tests-first | 0 | 2.993207257 | 20 / 0 / 0 |

The actual Cargo commands were:

```sh
cargo build --offline --locked -p lamp-actions -p lamp-voice --example schedule_off_actions --example schedule_off_voice
cargo metadata --offline --locked --format-version 1
cargo build --offline --locked --workspace
cargo metadata --offline --locked --format-version 1
cargo build --offline --locked -p lamp-actions -p lamp-voice --example schedule_off_actions --example schedule_off_voice
cargo test --offline --locked -p lamp-actions --all-targets -- --test-threads=1
cargo test --offline --locked -p lamp-schedule --all-targets -- --test-threads=1
cargo test --offline --locked -p lamp-voice --lib reminder -- --test-threads=1
cargo test --offline --locked -p lamp-voice --lib tools -- --test-threads=1
```

The implementation also ran the same metadata command after its workspace build. Exact helper commands and per-case environment values are preserved in [comparison-readings.json](comparison-readings.json). Original invocation records preserve the Python runner command names and their first output. Runner source is intentionally excluded from this text-only export. No execution was retried; no stages were skipped.

## Exact default-off comparison

For each `ordinary` and `tasks` profile, both `LAMP_SCHEDULE_TOOLS` and `LAMP_SCHEDULE_SPEAK` independently take absent, `off`, `ON`, `true`, `1`, and ` on `: 2 × 6 × 6 = 72 cases. Each case invokes the Actions helper and Voice helper in both copies. The baseline and implementation share the same fresh baseline-owned catalog fixture path so **raw bytes, including paths, are compared without normalization**.

The corpus has 144 stdout readings / 576,648 bytes per copy, plus 144 empty stderr readings per copy. It contains 3 distinct stdout payloads totaling 14,824 bytes. Byte-identical originals are retained once and mapped individually: 288 stdout and 288 stderr originals preserve each copy, profile, switch value, role, original path, retained path, size, SHA256 and command. Empty stderr has one exact-byte retained file. Metadata stdout/stderr and all current-run raw command logs are also retained.

The helpers are **179-byte Actions + 1,576-byte Voice = 1,755 combined bytes per copy**, unchanged from the prior proof. Actions SHA256: `8fc55e05703d1aa5ca58348132e3f4b0d7eed202e443f608f0d5af562c8f35e1`. Voice SHA256: `12d36f2d201b1117df21a55574855c8b44bcd2ec06f7b6fa980603e809b41695`.

These helpers capture public catalog bytes, environment arguments, declared tools, their fixed existing route list and ordinary Written serialization. They do not start a Tasks service, directly route `check_agent_task`, or serialize End.sound. The Tasks watch default is present identically in both source copies; this capture does not test its runtime behavior. The standalone Voice `tools` filter is broader than the parent's focused `tools::` filter; counts here come from these exact commands.

## Original records and retention

- [Completed baseline report](records/baseline-completed.json)
- [Completed amended implementation report](records/implementation-completed.json)
- [Filesystem execution audit](records/execution-audit-first.json)
- [Final preparation audit](records/final-preparation-audit.json)
- [Original pre-correction preparation audit](records/pre-correction-preparation-audit.json)
- [Original-path retention index](retention-index.json)
- [Helper, archive and executable identities](artifact-identities.json)
- [Referenced paths intentionally excluded](excluded-references.json)

The audit rechecked 2,690 historical/preparation originals. Mutable top-level prose and current parent qualification records were excluded from that immutable-history claim. Earlier failed full-gate and restricted runs remain identified in their original reports; this successful subset does not relabel them. The first baseline preparation command's stdout was not retained as a separate scratch log; its immutable prepared report, archive command/hash and source map are retained. The amended implementation preparation's first stdout is retained.

Only UTF-8 text is exported. There are no Python files, archives, executable binaries, credentials, fixture directories or source trees. `SHA256SUMS.txt` covers every export file except itself, including the retention index. Every raw artifact's exact bytes and SHA256 were checked against its original at export.
