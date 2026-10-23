# Actual helper from the current integration base

Additional provisional evidence for #2809 and #3269, 23 October 2026.
The [6408 run](../previous-helper-6408-20261023/README.md) remains unchanged.
Release 6439 became available while the PR was in review. Its source
`810e05109bced3beca88a5b602930cfed5fba4e5` is the direct parent of current
head `46f5f384ad94f9c2dcc5f7050991ef4325621384`, making it the closer
provisional old side. First still selects the supported release pair.

The [published release](https://github.com/autonomous-ai/lampOS/releases/tag/lampos-lamp-voice-6439)
identifies successful push/publisher run
[39954691687](https://github.com/autonomous-ai/lampOS/actions/runs/39954691687).
The helper is 4,690,480 bytes, asset 669155906, SHA-256
`46cc53ce09ef4e93272daf4849f4aab41034efad61cb5bc10f7aefde55067d17`.
Its bytes match both GitHub's asset digest and the decoded manifest entry;
its ELF header identifies Linux ARM64. The signed envelope still names key
fingerprint `54213f8d3aa2f241`; independent signature verification remains
pending Migrate's trusted public key. Metadata agreement is not that check.

## Executed result

All six unchanged manual tests pass against this different executable:

- Standalone format/restore/refusal cases: **4 passed / 0 failed**, 2.20 s.
- Current-coordinator mismatch and logical-cut recovery: **2 passed / 0 failed**,
  7.56 s, 155 ordinary round tests filtered out.
- Formatting and strict scoped Clippy: exit 0, no diagnostics.
- Final fixture-owned container listing: exit 0, empty.

[standalone.txt](standalone.txt), [round.txt](round.txt), [cleanup.txt](cleanup.txt)
and [receipt.json](receipt.json) retain output, source and artifact bindings.
Terminal blank lines were removed; original output hashes are recorded.
The typed cut panic in round output is deliberate and caught by the test.
The current writer/coordinator is Mac Rust 1.96.0 with fake systemd. Only
old Apply/Back executes in fresh network-disabled ARM64 containers.

The old 6408 and 6439 helper runtime sources are identical: their only Update
file changes are test code in `src/unit.rs` and `tests/functions.rs`. Records,
CLI, manifest, Cargo manifests/lock and runtime dependencies are unchanged.
The actual executable digests differ, so this is new exact-artifact coverage,
not a relabelled earlier run. From 6439 to the PR, production changes are
`src/apply.rs`, new `src/head_touch.rs`, `src/lib.rs` and `src/round.rs`;
request/back/done/journal formats, CLI and manifest remain unchanged.

## Reproduction and limits

Start with the source and two fixture files described in the
[6408 reproduction instructions](../previous-helper-6408-20261023/README.md#isolation-and-reproduction).
Apply [artifact-pin.patch](artifact-pin.patch) afterward with
`git apply --unidiff-zero artifact-pin.patch` from the repository root, and point
`LAMP_COMPAT_OLD_HELPER` at release 6439's `lamp-update`. The patch changes
only the pinned SHA-256 constant. Every assertion, test name, container
boundary and command is unchanged. The receipt records both final fixture
hashes; no binary or key is committed.

This does not execute an ARM64 coordinator, real systemd, a real process cut,
a physical unplug or a real-key verification. It does not select the supported
pair or qualify an installed lamp. Successful new pins projection still needs
the fixed helper refreshed by install; this old helper's incompatible signed
pins are restored before reader restarts. The PR's official ARM64 gate and
its review dispositions remain separate. First owns pair selection and merge;
Migrate owns the public-key handoff; the holder/person own physical acceptance.
