# Forget cleanup outcomes — 9 November 2026

This verifies the assigned follow-up from Queue #4710, comment 6555801724.
A successful primary memory deletion must not hide a failed room-line or
household-consent cleanup. The existing deletion scope, operation order,
retries and deadlines are unchanged. Only fixed step outcomes are added.

## Coverage

- Real Unix-socket replies: room cleanup loses its reply, reports a storage
  error, returns a malformed count, or confirms zero removed. An empty
  primary store plus failed room cleanup remains partial.
- An older memory's `unknown_op` response reports `kept`. With the room
  switch off, no room cleanup result is invented; refused primary deletion
  still does not call the room cleanup.
- A directory at each consent-file path independently forces removal to
  fail. The other consent is still removed; an absent consent is cleared.
  Later success never restores `everything: true`.
- Sanitizer tests retain only `cleared`, `kept` and `uncertain`, reject
  arbitrary strings/objects/null, preserve absent fields and partial flags,
  and do not copy cleanup fields from unrelated calls.
- An Engine test inspects the actual next model request after forgetting:
  all three fixed outcomes and the partial flag survive, while private
  payloads and earlier context do not.

## Local checks

Initial base: `a0d68e2a9`. Broad local verification base: `6167fe37a` (train 742).
The reporting patch rebased without conflicts. It was then carried to
review base `f84505707`; every authored added/removed line in the source
and existing docs was compared and is identical. The broad local results
below belong to `6167fe37a` plus this patch; the 19 focused checks, formatting, strict Clippy
and the workspace build passed again on the review base. Native CI must
verify the final PR composition.

Mac host, Rust 1.96.0, debug tests; `CARGO_BUILD_JOBS=2`,
`CARGO_TARGET_DIR` set to the shared local Cargo cache, `TMPDIR=/tmp`.
Commands run from `lampOS/` unless specified otherwise.

- `cargo fmt --all --check`: passed.
- `cargo test -q -p lamp-voice --lib forget_cleanup -- --test-threads=1`:
  2 passed.
- `cargo test -q -p lamp-voice --lib after_forget -- --test-threads=1 --nocapture`:
  16 passed.
- `cargo test -q -p lamp-voice --lib a_known_persons_forget_everything_also_forgets_what_they_said_in_the_room -- --test-threads=1`:
  1 passed.
- Strict Clippy passed with `--all-targets -- -D warnings` for
  `lamp-body`, `lamp-cloud`, `lamp-control`, `lamp-expression`, `lamp-keeper`,
  `lamp-lean`, `lamp-memory`, `lamp-name`, `lamp-peek`, `lamp-setup`,
  `lamp-skills`, `lamp-update`, `lamp-voice`, and `lamp-voice-eval`.
- Full serial tests for the same packages: 10,327 passed, 6 failed,
  56 ignored, across 102 completed targets. Exit 101 names exactly Keeper
  library, Setup integration, and Update image-journal integration; all
  six failures are detailed below. Voice library: 5,355 passed, zero failed,
  15 ignored; its remaining targets passed. Update round: 269 passed.
- `cargo build -q --workspace` on both broad local and review bases: passed (exit 0).
- From `.github/scripts/docs-links`: `npm ci --offline --ignore-scripts --no-audit --no-fund`,
  `npm test`, `npm run check`: 9 tests, 191 documents, 1,885 local links,
  zero broken, including the final check after this record was added.

The broad gate commands, with identical package selection for lint/tests:

```sh
cargo fmt --all --check
cargo clippy -q -p lamp-body -p lamp-cloud -p lamp-control \
  -p lamp-expression -p lamp-keeper -p lamp-lean -p lamp-memory \
  -p lamp-name -p lamp-peek -p lamp-setup -p lamp-skills -p lamp-update \
  -p lamp-voice -p lamp-voice-eval --all-targets -- -D warnings
cargo test -q --no-fail-fast -p lamp-body -p lamp-cloud -p lamp-control \
  -p lamp-expression -p lamp-keeper -p lamp-lean -p lamp-memory \
  -p lamp-name -p lamp-peek -p lamp-setup -p lamp-skills -p lamp-update \
  -p lamp-voice -p lamp-voice-eval -- --test-threads=1
cargo build -q --workspace
```

The initial default-parallel broad run stopped in the unchanged Body
supervisor fixtures: 534 passed and 10 failed with `motor startup timed out`
at `crates/body/src/runtime/supervisor/tests.rs:157`. Later packages and the
workspace build did not run in that command. The fixture starts the current
test executable as a child and has a two-second readiness bound. The final
run on train 742 uses `--no-fail-fast -- --test-threads=1`; no timeout or
runtime implementation was weakened. On the rebased serial run Body's
library passed all 551 tests, including the failed fixtures; its binary and
integration targets also passed. Both results remain recorded.

The serial run also exposed Keeper's unchanged
`package::tests::the_bus_root_is_root_s_with_the_parts_group_and_set_group_id`:
this Mac retained mode `0770` rather than `02770`. A standalone temporary
directory reproduced it: its inherited group was not in the process's group
list, and chmod discarded set-group-ID. The exact library test passed (1/1,
0.06 s) with `TMPDIR` pointing to a fresh directory whose group was changed
to the process's primary group. No project source was changed. Executable:
`target/debug/deps/lamp_keeper-a34ac815c59d59b7`, arguments `--exact
package::tests::the_bus_root_is_root_s_with_the_parts_group_and_set_group_id
--nocapture`. An initial binary-target selection matched zero tests and is
not counted as a verification pass.

Setup's unchanged integration test
`reset::a_reset_marks_this_boot_so_its_shutdown_keeps_no_journal` failed at
`crates/setup/tests/it/reset.rs:326` because it unconditionally reads Linux's
`/proc/sys/kernel/random/boot_id`, absent on this Mac. The platform guard is
owned by Second's separate #5780; this patch does not duplicate it. Native
Linux verification is required for that test.

Update's unchanged image-journal integration target passed 13 and failed 4:
`image_ramlog::a_boot_the_owner_forgot_is_not_kept_at_its_shutdown`,
`image_ramlog::a_failed_move_keeps_the_kept_boots_and_says_so`,
`image_ramlog::the_kept_boots_are_kept_72_hours_at_most`, and
`image_ramlog::the_last_two_boots_are_kept_at_shutdown_only`.
The script uses GNU `stat -c %Y`; this Mac's `stat` rejects `-c` and the
script reports an arithmetic error. The missing generated retention
folders and zero-versus-two retained-link assertion are consistent with
that host incompatibility. GNU `stat` is not installed here; no shim,
package install or unrelated script edit was made. Native Linux CI must
verify these cases.

The first consent failure fixture used an unknown caller and hit the
existing privacy refusal, before cleanup. Correcting the fixture to use a
known person made both tests pass; the runtime guard was not weakened.

## Timing and limits

Target: less than 1 ms of added status bookkeeping. The measured boundary
is only sanitizer entry to its returned outcome, with all three cleanup
fields, in the host debug build. In the original grouped test run the
fixture's first timed sample was 62,368 ns; across 1,000 repeated calls,
p50 7,126 ns, p95 7,420 ns, maximum 10,846 ns. Other tests had already used
the sanitizer in that process, so that sample is not cold first use.

A second run selected only `second_provider::after_forget::tests::cleanup_outcome_status_timing`
with `--exact ... --nocapture` in a fresh test process, using the rebased
`lamp_voice-47ed6d9ed9bbd06f` library executable. It passed 1/1. First
sanitizer call: 162,282 ns; 1,000 repeated calls: p50 10,581 ns,
p95 12,215 ns, maximum 61,787 ns. The broader serial gate was running on
the same host; this is not an idle-host or before/after speedup claim.
Both sets meet the target for this boundary, not all added bookkeeping.
The timing fixture reports observations without a flaky wall-clock assertion.

No file-deletion, IPC, process-startup, model-response, acoustic or lamp
latency is claimed. No device was accessed. Native ARM64 and Linux Clippy
CI, and the required two privacy reads, remain separate release gates.

## Self-review

The patch changes result reporting, not identity selection or deletion
permissions. Room switch-off and primary refusal retain their existing
behavior. Final scope/deadline checks still replace outcomes with generic
uncertainty when a result cannot be released. No new switch, bus topic,
unit setting, message type, persistence field, retry or background task is
introduced. The new tool-result keys contain no names, paths, memory text
or arbitrary error strings. Both producer and sanitizer prevent a partial
result from becoming complete after another step succeeds.
