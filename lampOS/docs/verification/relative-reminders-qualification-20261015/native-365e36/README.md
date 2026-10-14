# Relative reminder follow-up: native ARM64

Run [38644608947](https://github.com/autonomous-ai/lampOS/actions/runs/38644608947)
completed successfully on exact task head
`365e36dbcd78ff247adf30b2d18afd4d9d6f1065`:
**4,410 passed / 0 failed / 35 inherited ignored**, across 286 harnesses.
The new published-catalog regression passed. This is GitHub's Ubuntu 22.04
ARM64 runner with Rust 1.96.0, not a lamp run.

The workflow ran, from `lampOS/`:

```sh
cargo build --locked --release --workspace --bins --examples
cargo test --locked --release --workspace -- --test-threads=1
```

The job ran from 2026-10-14 22:01:07 to 22:32:38 UTC. GitHub's step boundaries
record the build from 22:01:52 to 22:07:41, and tests from 22:07:41 to 22:32:22.
Those are CI process boundaries, not interaction or device latency. All job
steps, including checksum generation and artifact upload, succeeded.
Four inherited Controls/Expression `replay_cost` output-filename collision
warnings appear in the original build/test log; no warning-free whole-workspace
claim is made. The task's strict affected-package host Clippy passed separately.

Artifact: `lampos-arm64-365e36dbcd78ff247adf30b2d18afd4d9d6f1065`.
Install only its `lamp-actions`, SHA-256:

```text
2c4aad6713b74ccccf55e5c2b6138f88ec8af9b5dc8d898b34138c320e6e4588
```

The CI-produced Voice and Schedule checksums match their previously qualified
and installed Task 31 binaries. No replacement of those binaries is required.
Preserve current lamp-owned units, character, Schedule store and Voice ledger.
First follows the [off-minute and relative-time replay](https://github.com/autonomous-ai/lampOS/blob/365e36dbcd78ff247adf30b2d18afd4d9d6f1065/lampOS/docs/schedule.md#relative-time-follow-up-on-the-lamp),
then restarts Actions before Voice with enabled settings so the next provider
session reads the new catalog. Third did not access a lamp or live provider.

- [Terminal job and step metadata](terminal.json)
- [Original complete CI output](terminal-log.txt)
- [Retained counts, warnings and checksums](retention.json)
- [Independent parent verification](parent-verification.json)
- [All CI-produced binary checksums](SHA256SUMS.txt)
- [Original watch output](watch.txt)

The parent verification re-parses the totals, requires successful terminal
status on the exact head, checks the new regression result, and verifies all
retained original bytes. It preserves warnings and ignored tests. It does not
qualify model selection, elapsed reminder delivery or lamp hardware behavior.
