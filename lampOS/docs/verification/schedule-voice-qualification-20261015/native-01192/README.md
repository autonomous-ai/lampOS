# Native ARM64 pass for the revised runtime

[Run 38626209078](https://github.com/autonomous-ai/lampOS/actions/runs/38626209078)
completed successfully at 2026-10-14 20:12:04 UTC for
`01192ba0fbfda04afb19e1db63aa86423c21fd6d`. A GitHub Ubuntu 22.04 ARM64
runner with Rust 1.96.0 ran:

```sh
cargo build --locked --release --workspace --bins --examples
cargo test --locked --release --workspace -- --test-threads=1
```

Both passed. The raw output records **4,338 passed / 0 failed / 33 ignored**
across 279 completed harnesses. Binary checksum and artifact-upload steps
also passed. The [result](result.json), [unedited output](run.txt), and
[retention index](retention.json) preserve the run identity and all emitted
binary checksums.

The final head `acb628b494df0c38842ea467d1289eec8d35d2ca` differs only by
the explicit `systemctl daemon-reload` disable instruction in `docs/schedule.md`;
all 697 frozen Rust/manifest hashes match. Its exact-head run 38627179829
still has to finish independently before readiness.

The build and test logs each retain two inherited output-name warnings:
Controls and Expression both have an example named `replay_cost`, colliding
at `examples/replay_cost` and `replay_cost.dwp`. Both example sources are
unchanged from integration, and the preceding c0 log has the same warnings.
No warning-free workspace build is claimed. The production Actions, Schedule
and Voice binary names are distinct; an unqualified `replay_cost` artifact
name alone does not identify its source package.

## Not shown

This is CI execution, not a lamp measurement. Ignored probes/models and
privileged fixtures remain unrun by this command. No install, physical
privacy, sound, motion, provider compatibility or acoustic latency was shown.
The final documentation head's CI result remains separate.
