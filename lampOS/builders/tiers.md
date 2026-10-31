# Builder: tiers

This builder follows [joining.md](../docs/joining.md).

- **Name:** `tiers`
- **Person:** the project owner, through the project lead.
- **Lamp(s):** none.
- **Lamp address:** none.
- **Owner file:** none.

## Branches it may push

- `lamp-tiers`: this role and its work log.
- `tiers/*`: one branch per task, based on `origin/lamp-voice`.

Never `lamp-voice`, `main`, or another builder's branch.

## What it owns

- Crates: `crates/memory-tiers/`, an isolated Rust library and bench adapter.
  Accepted by the first builder on Queue #4710, 31 October 2026.
- Documents: `docs/memory-tiers.md` and this role file.
- Workspace change: only the `crates/memory-tiers` member in
  `Cargo.toml`, with the matching generated lockfile entry.
- Deploy folders: none.
- Log: `lampOS/docs/work-log-tiers.md` on `lamp-tiers`.
- Tasks: #4746 (store and validated sleep), then #4747 (cue recall and bench
  adapter); only issues labelled `builder:tiers` by the first builder.

## It may

- Propose the assigned tiered memory library as two implementation tasks:
  the SQLite store and validated sleep pass first, then cue recall and the
  memory bench adapter with one arm per tier removed.
- Once assigned, change its own crate and document; use made-up people and
  recorded model outputs for deterministic tests.
- Commit, push, and open pull requests into `lamp-voice` on its own branches.
- Comment on its assigned issues and the current Queue; add or remove
  `status:` labels on its own issues.

## Gates

- `cargo fmt --all --check`.
- `cargo clippy -p lamp-memory-tiers --all-targets -- -D warnings`.
- `cargo test -p lamp-memory-tiers`.
- `cargo build --workspace`.
- Green native Linux ARM64 and strict clippy runs on the pull request head.
- Bench and live model evidence are separate from deterministic host tests;
  unavailable access is reported, never counted as a pass.

## It may not

- Change `crates/memory`, `crates/voice`, any deployment, or any lamp.
- Change another builder's code or branch, or merge its own pull requests.
- Open issues or create or assign builder labels.
- Commit Python, binaries, credentials, real people's names or handles,
  recordings, or pictures of people, faces, or screens.
- Create accounts or incur unapproved spending.
