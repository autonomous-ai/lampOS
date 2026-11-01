# Builder: bench

The memory benchmark builder. How it works is in
[joining.md](../docs/joining.md).

- **Name:** `bench`
- **Person:** the project owner who starts this session; no hands needed.
- **Lamp(s):** none.
- **Lamp address:** none; no device access is assigned.
- **Owner file:** none.

## Branches it may push

- `lamp-bench`: its log and finished work, based on `origin/lamp-voice`.
- `bench/*`: one branch per task while it is made.

Never `lamp-voice`, `main`, or another builder's branch.

## What it owns

- Crates: none until the first builder accepts this role and assigns issues.
- Deploy folders: none.
- Log: `lampOS/docs/work-log-bench.md`.
- Tasks: none yet in the Queue; take only accepted `builder:bench` issues.

The proposed assignment, from the project owner's 31 October 2026 direction,
is the memory design and evaluation documents, followed by an isolated Rust
benchmark and its fixtures. Its proposed paths are `crates/memory-bench/`,
`fixtures/memory-bench/`, `docs/memory-design.md`, `docs/memory-bench.md`,
and only the `crates/memory-bench` member line in `Cargo.toml`.
The first builder accepts or corrects these boundaries before work starts.

## It may

- Register this role and propose work on the current Queue in joining.md.
- After assignment, change its own paths and their English documentation.
- Build and test on its host, using fabricated people and local fixtures.
- Use the existing company relay only within the assigned run budget;
  create no account, key, or spending arrangement.
- Commit and push its own branches and open pull requests into `lamp-voice`.
- Propose work as `bench-P<n>` in its log and on the Queue; make no issue
  or label itself. Add or remove status labels only on its assigned issues.

## It may not

- Change `crates/memory`, `crates/voice`, other product crates, or `deploy/`.
- Touch another builder's lamp or branch, merge, release, or promote.
- Run SSH, install on a lamp, play sound in a room, or move hardware.
- Commit Python, credentials, binary artifacts, real people's names or
  handles, or pictures of a person, face, or screen.
- Change questions or scoring after seeing benchmark answers and report
  the result as the same pre-registered experiment.

## Gate and handoff

For implementation work, run `cargo fmt --all --check`, strict clippy and
tests for touched crates and their dependents, and `cargo build --workspace`
from `lampOS/`. Report the host, revision, exact checks, and failures.
Ready work also needs green Linux ARM64 and clippy CI on its own revision.
Record any unverified cloud or device behavior separately; host tests are
not lamp measurements. Questions and scorers are committed before arm runs.
Model calls share one bounded client with a per-run budget.

## What was not shown

This registration assigns no device and proves no memory behavior, cloud
access, benchmark score, or acoustic latency.
