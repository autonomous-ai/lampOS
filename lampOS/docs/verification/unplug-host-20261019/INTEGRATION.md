# PR #1699 integration recheck, 19 October 2026

The original [host gate](README.md), its 3,995 passed tests and
[source manifest](source-sha256.txt) belong to feature commit `13e72c290`
on base `6275fcc8cb2c229f8b18a6db3b4485cb46a11676`. Those files are preserved;
they do not claim a full-suite rerun on the integrated source below.

This rebase integrates `origin/lamp-voice` at
`251c6700e79272f871eaa01082749aa6e45a9a33` into the same single feature commit
on `fifth/unplug-skill`. [The integration manifest](integration-source-sha256.txt)
pins the rebased feature implementation, the product catalog fixture, and the
Cargo lockfile checked here.

## Resolution

- `crates/update/src/install.rs`: retain upstream's complete product skill
  list and `skill_file!` macro; add Prepare-to-unplug to `PRODUCT_SKILLS` and
  install its two files. The product still does not set `LAMP_UNPLUG=on`.
- `crates/update/tests/functions.rs`: the product-default catalog fixture
  loads installed skills directly, so apply the same unplug exclusion that
  the runtime uses with the switch off. The product keeps 31 functions.
- `docs/install.md`: retain upstream's qualifications about the new product
  units and append the unplug installation/opt-in instructions.
- Voice coordinator and module declarations merged without conflict. The
  existing guarded acceptance, cancellation, audio admission and final
  speaker dispatch checks remain intact beside upstream changes.

## Commands and results

From `lampOS/`, each command used
`RUSTC_WRAPPER= CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0` and the existing
`lampOS/target` directory. Test commands ran outside the filesystem sandbox
because the fixtures use local IPC sockets and subprocesses.

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | Passed |
| `cargo clippy -p lamp-update -p lamp-voice -p lamp-skills --all-targets -- -D warnings` | Passed, no warnings |
| `cargo test -p lamp-update --test install --test functions -- --test-threads=1` | Install 48 passed; functions 2 passed |
| `cargo test -p lamp-voice --lib unplug -- --test-threads=1` | 10 passed |
| `cargo test -p lamp-voice --lib say:: -- --test-threads=1` | 36 passed |
| `cargo test -p lamp-skills unplug -- --test-threads=1` | Library 3 passed; integration 2 passed |

All listed test commands completed with zero failures and zero ignored tests.
The two Voice filters overlap; these counts are not added into a unique-test
total. No full workspace test rerun, native ARM64 result or device trial is
claimed here. The feature stays default-off pending the qualification described
in [unplug.md](../../unplug.md).
