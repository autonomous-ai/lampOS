# Generic voice clock gate, 23 October 2026

PR #3260, head `7c3693e6640a0e144355a1cbdcdf4b0ddd88c9ad`, based on
`b5a199368`. The equal patch was first tested as `ad18934be` on `cf462adb0`.
Five existing product switches are enabled in the generic voice template:
`LAMP_TOOLS`, `LAMP_CLOCK`, `LAMP_TIME_PLACES`, `LAMP_KERNEL_CLOCK` and
`LAMP_SESSION_FACT`. No product unit or runtime algorithm changes.

## Tests first and full gate

Before the unit change, the new unit census failed on missing LAMP_TOOLS,
and the isolated generic function/context test failed its settings assertion.
After the unit change, on the Intel Mac with local socket access:

```text
cargo fmt --all --check
cargo test -q --offline --locked -p lamp-update --all-targets -- --test-threads=2
cargo clippy -q --offline --locked -p lamp-update --all-targets --message-format=short -- -D warnings
```

All exit zero. Tests: 120 library, 5 function, 82 install, 13 remaining
integration and 123 round cases passed (343 total); 2 library cases ignored,
and the two Linux-only custom harnesses each reported one ignored case.
Full tests took 1088.01 s; Clippy took 71.02 s, jobs 2.

## Latest-base carry

Range-diff: `ad18934be = 7c3693e66`. The isolated patched worktree on
`b5a199368` matches the complete tracked tree of the pushed head, and all
five source hashes stayed unchanged during its gate.

The first invocation mistakenly used the restricted sandbox. Four loopback
binds failed with EPERM and the bus test returned no events (115 passed,
5 failed, 2 ignored). The bus constructor can discard a subscription error
through the test's silent diagnostic closure, but that error was not captured:
the empty receive is retained as unattributed. Function tests and Clippy did
not run after that failure. This is an invalid environment invocation, not a
product pass or a waived regression.

One corrected invocation with socket access and unchanged source/fixtures:

```text
cargo test -q --offline --locked -p lamp-update --lib --test functions -- --test-threads=2
cargo clippy -q --offline --locked -p lamp-update --all-targets --message-format=short -- -D warnings
```

Both exit zero: 120 library plus 5 function tests passed, 2 ignored
(11.563 s), strict Clippy passed (60.767 s). Prior unchanged-tree formatting
passed in 11.416 s. No retries with altered timing or fixtures.

## Limits

The dependency read follows the existing system clock and zone files,
worker flag forwarding, local clock function and new-session context paths.
No additional product-only configuration or service is required. This does
not set the lamp's zone or prove a moved lamp's spoken time or acoustic
latency. No device action or ARM64 linking on the Mac was performed.
Native gates for the exact head subsequently passed:

- [ARM64 39938443228](https://github.com/autonomous-ai/lampOS/actions/runs/39938443228):
  10,999 nextest cases passed, zero failed, 68 skipped. The case
  `same_voice::tests::each_line_the_check_says_is_said_to_its_journal`
  was marked leaky; that marker is retained. Doc tests passed and the
  separate custom harness passed 12/0. The workflow completed successfully.
- [Linux Clippy 39938443224](https://github.com/autonomous-ai/lampOS/actions/runs/39938443224):
  every workspace crate and target passed with warnings denied.

The reviewer-lead posted READ-OK on the exact head. Linux formatting also
passed in [receipt 0fd7a46b9](https://github.com/autonomous-ai/lampOS/tree/0fd7a46b9/lampOS/docs/verification/generic-voice-unit-linux-fmt-20261023),
with Rust/Cargo 1.96.0 and rustfmt 1.9.0 on Linux x86_64. The receipt's
commit, parent, root tree and lampOS tree match the pushed source locally;
all 21 indexed receipt files and 20 embedded raw streams have verified
hashes. The raw formatter sentinel is `cargo_fmt_exit=0`, and before/after
source fingerprints match. The archive audit records 6,680 verified Git
blob/mode identities. An earlier archive preparation timeout is retained;
only the export budget changed, and the formatter ran once.

## Landed verification

First's train [#3261](https://github.com/autonomous-ai/lampOS/pull/3261)
merged this change at `8e3eaa6c26cbe1a5a0c924f5b3a7e972d9083850`.
All five authored file blobs match `7c3693e66` exactly, checked with
`git rev-parse <commit>:<path>` for both commits. The train's
[ARM64 run 39941472913](https://github.com/autonomous-ai/lampOS/actions/runs/39941472913)
and [Linux Clippy run 39941472888](https://github.com/autonomous-ai/lampOS/actions/runs/39941472888)
both completed successfully. Signing/publishing and failure-report jobs
were skipped. This verifies the merged source and CI gates; it does not
claim installation or spoken-time acceptance on a lamp.
