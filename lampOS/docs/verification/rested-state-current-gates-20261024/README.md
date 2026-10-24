# Rested-night fixture: current carried local gates

Second, 24 October 2026, 10:00 +07. PR #3327 head
`b76fe12ae98c1ac2bd284dae347ab42bb3816776`, based on `9ed44f88c` and
parent #3323 `6c8c52e37`.

Mac Rust 1.96.0 commands from `lampOS/`:

```sh
cargo fmt --all --check
cargo clippy -q -p lamp-skills --all-targets -- -D warnings
cargo test -q -p lamp-skills
```

All exited 0: **514 passed / 0 failed / 7 ignored**. Six ignored cases are
published-reader cases, including the two new rested-night directions; one
is pre-existing. This is compilation and ordinary-suite evidence, not native
execution of those ignored cases. Cargo ran sequentially in the existing
cache after the parent gate, without overlapping builds.

Exact-head source-only Linux formatting passed with **6,974 regular files
unchanged**. The source worktree remained clean; pinned offline/read-only
container, no host mounts or devices, UID/GID 65534 and private tmpfs.
Owner-label, removal and container absence were verified. This is not a local
ARM64 build/test gate.

Native jobs are separate: pair 40056359883, ordinary ARM64 40056359863,
Linux Clippy 40056359896. They were pending when this record was written.
The fixture uses private synthetic Expression/Body peers, permitted offset
mapping and actual released Skills readers. It does not qualify physical
output, an elapsed night, timezone migration, updater installation, power loss
or independent signature trust.
