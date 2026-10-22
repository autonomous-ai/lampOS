# Failed-unit report: native CI after base 3095487

PR [#2764](https://github.com/autonomous-ai/lampOS/pull/2764) head `27ab70a2c6dbf1e4769de767082fc66291a87c8d` is qualified by the completed
[ARM64 run 39720328972](https://github.com/autonomous-ai/lampOS/actions/runs/39720328972) and
[strict x86 Linux Clippy run 39720329001](https://github.com/autonomous-ai/lampOS/actions/runs/39720329001).
Both conclusions are **success**. This receipt covers this exact head, not a
later rebase or device acceptance. The reviewer decides whether to merge.

Both checkout logs identify merge
[`583477b12f4b18df17b6b7b2f8a0a6e6f570af14`](https://github.com/autonomous-ai/lampOS/commit/583477b12f4b18df17b6b7b2f8a0a6e6f570af14). Its recorded parents are base
`3095487ad6330c89566e7d6db6208dcc882f47e9` and head `27ab70a2c6dbf1e4769de767082fc66291a87c8d`, in that order.
Checkout proof is at ARM64 log lines 289–292 and Clippy lines 326–329.
The API pull-request association agrees, but it is mutable; checkout logs and
merge parent objects independently establish what was tested.

## Observed results

- Nextest: **10,190 passed, 0 failed, 63 skipped**, in **174.456 seconds**.
  The passed total includes **one leaky pass**. Exactly 10,189 primary `PASS`
  rows and one `LEAK` row reconcile to the summary at line 10867; repeated
  output after that summary is not counted again.
- All three added Cloud tests passed: allowlisted/unambiguous failed units
  (line 1911), consent-gated shared query/cache (1912), and canonical schema-one
  retransmission (1989). Exact names and timings are in [ci.json](ci.json).
- Doc tests: **4 passed, 0 failed, 0 ignored**, across **46 targets**.
  Separate `lamp-turn-end-wire` process harness: **12 passed, 0 failed**.
  Separate IPC relay harness: **one successful invocation**, `relay_signals: ok`.
- Publication simulator, artifact provenance, and real CLI rehearsal:
  **35, 12, and 7 passed**, respectively; zero failures.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` passed on
  x86 Linux with **zero Rust/Cargo warnings**. Infrastructure notices remain
  recorded below and in the JSON.

This is the PR profile, partition `hash:1/1`: the selector chose the whole
workspace, with 162 nextest binaries. The realtime group is excluded by this
profile. The start line reports 63 tests and two binaries skipped; individual
skip names/reasons are not logged, so none are invented here. The two explicit
nextest harness exclusions ran separately as documented above.

The leaky pass is
`lamp-voice same_voice::tests::each_line_the_check_says_is_said_to_its_journal`
(line 8956, 0.228 seconds). It is retained as a limitation, not described as a
clean subprocess result.

## Warnings and skipped workflow work

ARM64 logged eight Cargo example-output collision warnings: `replay_cost`
and `progress_cost`, plus their `.dwp` outputs, each during release build and
nextest preparation. It also logged one certificate-rehash notice, one
existing-rustup-settings notice, four Node deprecation warnings, and one
Node-20-to-24 runner annotation. Clippy logged the same certificate/rustup
notices, two Node deprecations, and one runner annotation; no Rust/Cargo warning.
Every warning event is retained with its line and normalized text in ci.json.

The ARM64 publication job and full-run-failure reporting job were skipped.
The cold-publication source-time step, example-target cleanup, and cache-save
step were skipped; Clippy cache save was skipped. A nested nonapplicable
PowerShell installer action was also skipped. All API job/step conclusions
and the nested log skip are preserved. No real publication, key use, release,
or environment activation was exercised.

## Artifact and evidence limits

CI reports packaging **47 binaries + 2 examples** with `SHA256SUMS`, and
uploading **50 files**. The
[artifact metadata](https://github.com/autonomous-ai/lampOS/actions/runs/39720328972/artifacts/12400313733) records ID **12400313733**, name
`lampos-arm64-27ab70a2c6dbf1e4769de767082fc66291a87c8d`, and **113,681,468 bytes**.
Its reported ZIP SHA-256 is
`9068912c0d7b2ed0b7b9566495d489f3d7b83e36fc703d20fbbaa68561f79496`.
ID, name, size, run/head identity, and digest agree between API metadata and
upload log. **The payload was not downloaded:** member inventory and content
checksums were not independently verified for this receipt.

[ci.json](ci.json) is a compact normalized record, not a byte-for-byte copy of
raw input. It binds all nine retained raw log/API/merge/run files by byte count
and SHA-256. Raw logs and full API objects are not published. [SHA256SUMS](SHA256SUMS)
binds the two public files. No tests were rerun to prepare this receipt.

The [same-head Mac proof](../report-failed-units-rebase-3095487-20261022/README.md)
is unchanged. The
[earlier failed native run](https://github.com/autonomous-ai/lampOS/blob/f90b1d028ba6cd2a76f4188fc10b18265f94b799/lampOS/docs/verification/report-failed-units-rebase-fab69-native-failure-20261022/README.md)
remains historical evidence; this successful run does not erase it. No physical
lamp, live broker, target latency, or acoustic acceptance is claimed.
