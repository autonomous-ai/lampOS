# Core unplug identity correction, 19 October 2026

Review of PR #1699 at `dbbfe5cb8` found that an alternate-named tool-only skill
could declare `body.prepare_unplug`, start folding while enabled, and then have
its completion speech rejected by Voice's canonical skill-name check.

The loader now accepts the operation only for `prepare-unplug` with tool
`prepare_to_unplug`. Runtime tool admission rechecks both names. The request
writer checks them again before any bus access, including reuse of a kept latch.
Tests reject aliases of either name, verify that neither alias creates a request,
and verify that an ordinary skill cannot borrow or change an existing latch.
The canonical single-step fixture still tests its prior trigger/additional-step
restrictions under the correct skill name.

These are routing restrictions within a trusted local runtime. Installed skill
definitions and local tool-socket callers remain trusted; the change does not
authenticate spoken consent or exclude another process with bus write access.

## Verification

The original [3,995-test host snapshot](README.md) and its source manifest are
unchanged. The prior [integration recheck](INTEGRATION.md) and its manifest also
remain unchanged. [identity-source-sha256.txt](identity-source-sha256.txt) pins
only the four Skills source/test files validated for this correction.

Commands from `lampOS/` used `RUSTC_WRAPPER= CARGO_PROFILE_DEV_DEBUG=0
CARGO_INCREMENTAL=0` with the existing target directory:

- `cargo clippy -p lamp-skills --all-targets -- -D warnings`: passed.
- `cargo test -p lamp-skills unplug -- --test-threads=1`: 4 library and
  3 integration tests passed.
- `cargo test -p lamp-skills -- --test-threads=1`: 15 library and
  80 integration tests passed; zero failures or ignored tests.
- Rust 1.96.0 `rustfmt --edition 2024 --check` on `crates/skills/src/lib.rs`,
  `main.rs`, `unplug.rs` and `crates/skills/tests/skills.rs`: passed.

The focused tests are included in the full Skills count, not additional unique
tests. This is Mac host evidence; no device or native ARM64 result is claimed.
