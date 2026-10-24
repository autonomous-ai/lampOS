# Actions cap-state fixture host and workflow checks

Source head: `65e7b57fa785acc37196d4b5a110f576d3741926`, based on
`9ed44f88c573837c50451c94f4d88f010e8a6721`. This evidence covers host
compilation/tests, source-only Linux formatting and synthetic workflow guard
checks. It contains **no native released-binary execution result**.

## Mac host gate

Rust 1.96.0 ran these commands sequentially in the isolated task checkout:

| Command | Observed result | Wall time |
|---|---|---:|
| `cargo +1.96.0 fmt --all --check` | exit 0 | 5.64 s |
| `cargo +1.96.0 clippy --locked --offline -p lamp-actions --all-targets -- -D warnings` | exit 0 | 5.98 s |
| `cargo +1.96.0 test --locked --offline -p lamp-actions` | 120 passed / 0 failed / 2 ignored / 0 filtered; exit 0 | 15.09 s |

The test total is 65 + 3 + 29 + 23 passing tests. Both ignored cases are the
new explicit native release-pair directions. Host compilation of their driver
does not run either published ARM64 executable. Complete logs and individual
exit receipts are under [`mac/`](mac/).

The fixture and dependency files were frozen throughout these gates; their
recorded SHA-256 values match the committed blobs, as checked in
[`source-binding.json`](source-binding.json). The source commit followed the
host gate, after the verification README recorded its counts. No complete
contemporaneous Mac worktree before/after hash manifest was retained. The
source-binding record is a later publication audit, not a replacement for
such a manifest or a new gate execution.

## Exact committed source on Linux

The exact committed source passed `cargo fmt --all --check` using cached image
`sha256:7dc3bb4b3b88bb00e16e37eeb5c98a74f9d926f956584eb8c009c9ca86023cd2`
on Linux ARM64. The container had network disabled, read-only root, no host
mounts, private temporary filesystems, all capabilities dropped and UID/GID
65534. A source archive was streamed over stdin, and source write permissions
were removed before formatting.

The [original receipt](linux-format/linux-fmt-current.json) binds these values:

| Item | Recorded value |
|---|---|
| Repository tree | `ff1c931850b7c51974633e388c8785c258996c93` |
| lampOS tree | `5986e4acb99f496833878edf5a1755bda4727646` |
| Source archive SHA-256 | `7dab2f23e087ff037497e4f9f40c5958945f993244d77718ad6b4d69e730c32b` |
| Source archive bytes | 212,879,360 |
| Regular files before / after | 6,971 / 6,971 |
| Both regular-file manifest SHA-256 values | `68fb4dd0be925c452979a8cd4c56b89061e1312b92c21dbb80cccfa714a4857e` |
| Formatting / source comparison / container exit | 0 / 0 / 0 |

The retained archive's size and digest were checked again while preparing
this publication. The archive is excluded from this directory. The
[captured output](linux-format/linux-fmt-current.txt) records both manifest
digests and the actual comparison exit. Individual manifest files were
temporary inside the container and were not retained after removal.
The [cleanup receipt](linux-format/linux-fmt-current-cleanup.json) and command
outputs record verified ownership, successful removal and no remaining owned
container. This was formatting only: no Linux build, Clippy, test, device or
release-pair run occurred in this container.

## Workflow checks are synthetic

The committed workflow digest matches
[`workflow/workflow.sha256`](workflow/workflow.sha256):
`8606184cf2ca3ec84d74db801562d1bfff779f040d62b4831781f482e0a9696c`.
The retained [structure report](workflow/structure.txt) says its runner,
environment, provenance and always-run checks passed.

The [execution-guard report](workflow/execution-guard-controls.json) contains
six synthetic controls: one accepted complete two-case result, and five
refused inputs covering zero cases, a partial result, duplicate direction
names, missing reap and an extra result. Their input summaries and JSON
receipts are retained under `workflow/guard-*`. These are deliberately
fabricated guard inputs, not released-process receipts; their `passed`
strings and 0.01-second summaries must not be counted as native tests.

`workflow/actionlint.txt` contains zero diagnostic bytes. No separate
actionlint exit receipt or shell syntax-check result was retained in the
provided source directory, so this bundle does not independently establish
those exit codes. Extracted shell programs are excluded. No workflow, Cargo
command or container was rerun to prepare these files.

## Copy integrity and limits

[`evidence-map.json`](evidence-map.json) lists every original and published
SHA-256, byte count, destination and redaction. There are 44 copied text
receipts: 43 are byte-identical and one Mac test log replaces five private
home-directory prefixes with `<private-home>`. `.log` names become `.txt`;
that rename itself changes no bytes. No test name, result, count or error was
removed. Original logs remain retained privately.

[`SHA256SUMS`](SHA256SUMS) covers every published file except itself. This
directory excludes binaries, archives, standalone scripts, credentials and
API identity objects. Container command/configuration receipts remain plain
text evidence, not executable scripts.

The source fixture uses two byte-identical published Actions artifacts and
private state/socket/bus paths. Host gates and workflow controls neither
prove native release compatibility nor confirm hardware output. Native
released-process results, ordinary ARM64/Clippy CI, later rebases, release
trust, updater installation and physical durability need their own receipts.
