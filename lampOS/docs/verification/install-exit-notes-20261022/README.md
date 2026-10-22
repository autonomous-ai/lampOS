# Install the product exit-note hooks

Recorded 2026-10-22 07:35 +07, branch `second/install-exit-notes`, based on public
`lamp-voice` `e2b2f596d71dcc8112cdb1ee508667a4ee4b0bbe`.
Assigned after failed-unit reporting in [Queue #2602](https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6324418282).

Install now writes one reserved `exit-note.conf` per fixed Cloud service,
33 names including five templates, in the existing systemd debt/write batch.
The recorder keeps the fixed template name; instance identifiers do not
reach note contents, filenames or reports. Other drop-ins and existing
switch/consent behavior remain. Bounded local notes may exist with sharing
off; queueing and sending require current consent. Existing earlier
stop-post failures can skip the appended recorder, as documented in cloud.md.

Final Mac gate, Rust 1.96.0:

- `cargo +1.96.0 fmt --all --check`: exit 0, 5.132 s.
- `cargo +1.96.0 clippy --locked --offline -p lamp-update -p lamp-cloud --all-targets -- -D warnings`: exit 0, 6.053 s.
- `cargo +1.96.0 test --locked --offline --no-fail-fast -p lamp-update -p lamp-cloud -- --test-threads=1`: **471 passed / 0 failed / 3 ignored / 0 filtered out**, exit 0, 318.604 s.

[gate.json](gate.json) retains the exact command-output summaries, timings,
raw-log hashes and all seven changed-file hashes. The ignores are existing
manual/network fixtures: Cloud `reports::tests::write_a_reports_fixture`,
Update `fetch::tests::a_release_file_from_github` and
`records::tests::write_records_fixtures`.

Compiled controls, restored byte-for-byte before validation:

- Omit the install hook chain: `install_places_every_exit_note_hook_and_keeps_other_drop_ins`
  fails at the missing hook, **0 passed / 1 failed / 0 ignored / 75 filtered out**,
  exit 101, 27.201 s. [Receipt](hook-control.json).
- Store the raw template instance: `reports::tests::template_exit_notes_keep_only_the_fixed_name_in_files_and_reports`
  fails at the fixed-name assertion, **0 passed / 1 failed / 0 ignored / 151 filtered out**,
  exit 101, 20.440 s. [Receipt](template-control.json).

The controls' six source hashes all match the final gate. The seventh file
is the round fixture correction described below; it changes no runtime.
Fixtures cover all hooks, exact lists, modes, preservation, reinstall and
repair, partial-write reload debt, all templates, note bounds, privacy and
consent. The installed drop-in's two active lines and the actual-instance
systemd query remain unchanged. Changed names reach Update and Cloud only;
both crates are gated. Read review found no P1/P2.

The first full gate was **448 passed / 23 failed / 3 ignored**. Twenty-two
failures expose one round fixture reading new drop-in directories as files;
`running_binaries` now skips directories. The remaining unchanged Cloud
Wi-Fi timing test observed 591.783458 ms against its 300 ms assertion. It
passed once on unmodified public `515e43d3a`, whose `online.rs` is byte-identical,
and passed in the final candidate gate. Its initial timing cause is not
established and its code was not changed. [All initial failures](initial-failures.json),
[initial gate](initial-gate.json), and [baseline comparison](baseline-timing.json)
are retained.

A second attempt stopped before tests with six missing-constant E0425
errors despite the constants being present and candidate hashes unchanged,
after switching the shared target through the baseline worktree.
[Its receipt](compile-attempt-2.json) is retained. Cleaning only Update and
Cloud's task-owned cached artifacts (381 files, 618.1 MiB), without further
source changes, preceded the final passing gate. No whole-cache cleanup or
full local workspace gate was used.

No recorder or service ran on a device. Host fixtures do not demonstrate
live systemd hook execution. ARM64 qualification is the PR's native run;
no local ARM64 build was run. Native status is recorded after publication.

## Rebase integration check

The patch rebased cleanly from `e2b2f596d` onto
`fc800db6f839cf006098f73ee84942572acbccfa`; [range-diff](range-diff.txt)
is equal. Upstream added one shipped reset-plan hash in Update and
shortened one existing Cloud test deadline. The implementation remains the
same patch. [Rebase gate](rebase-gate.json) records all seven final source
hashes before and after: unchanged during verification.

- `cargo +1.96.0 fmt --all --check`: exit 0.
- `cargo +1.96.0 clippy --locked --offline -p lamp-update -p lamp-cloud --all-targets -- -D warnings`: exit 0.
- `cargo +1.96.0 test --locked --offline -p lamp-update --test install -- --test-threads=1`: **76 passed / 0 failed / 0 ignored / 0 filtered out**, exit 0.
- `cargo +1.96.0 test --locked --offline -p lamp-cloud -- --test-threads=1`: **177 passed / 0 failed / 1 ignored / 0 filtered out**, exit 0.

These focused integration results supplement the full changed-crate
**471/0/3/0** gate above; they are overlapping tests, not additional coverage
to be summed. Changed-name searches found only Update and Cloud callers.
The pushed PR head still needs its own ARM64 and Linux Clippy runs.
