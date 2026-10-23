# Relay worker PID readiness — native control and validation

The fixture now waits for `worker.ready`, published only after the complete
worker PID write succeeds. One deterministic before control fails at the
original empty-PID parse; one after control passes all six original assertions.
The unhooked full VoiceEval suite passes **623 tests, 0 failed, 0 ignored and
0 filtered**, including all seven relay-hardening cases.

These results qualify frozen base `62b1120fc1af5462529f0c76089a322500e30929`
plus the two file hashes in [source-and-restoration.json](source-and-restoration.json). The candidate had no
commit at execution. The only code change is inside
`crates/voice-eval/tests/it/relay_hardening.rs`; the other file is one sentence
in `docs/voice-eval.md`. All Rust outside the named case and the complete tail
containing its six assertions are byte-identical to the base.

## Controlled observation

| Arm | Actual test exit | Observation | Test-process wall time |
| --- | ---: | --- | ---: |
| Before | 101 | Original `ParseIntError { kind: Empty }`; opened empty PID, no release | 1.401921 s |
| After | 0 | `WAIT worker.ready`, one release, complete 6-byte PID, all assertions pass | 0.649556 s |

The scratch-only shell opens/truncates the PID file, records its runtime and
worker PIDs, then blocks on a private FIFO. The closer sees a positive opened
marker and an empty PID. It can release the writer only while waiting for the
selected readiness file. In the before arm the PID already exists, so closure
happens without releasing the write. In the after arm `worker.ready` does not
exist, so the FIFO releases the write and the ready marker follows its success.
No guessed delay or product change creates this distinction. This reproduces
the fixture mechanism, not the exact scheduling history of an earlier flake.

Both normal control exits independently proved the recorded runtime and worker
PIDs absent using a bounded 3-second/20-ms event poll. Both were absent on its
first observation. No signals were sent and neither 20-second external
watchdog fired. A timeout would have invalidated the control; runtime cleanup
would first require one targeted inspection proving that runtime remained a
direct child of the live owned test process and leader of its process group.
The exact command, PIDs, markers, complete raw UTF-8 stdout/stderr and original
byte counts/SHA-256 digests are in [controls/before.json](controls/before.json) and [controls/after.json](controls/after.json).
The driver exit was 0 in both arms because it checked these opposite expected
outcomes; it does not relabel the before test exit 101 as a test pass.

## Unhooked native gates

| Command | Exit | Wall time |
| --- | ---: | ---: |
| `cargo test -p lamp-voice-eval --offline --locked --no-fail-fast --message-format=json` | 0 | 150.563307 s |
| `cargo fmt --all --check` | 0 | 12.853335 s |
| `cargo clippy -p lamp-voice-eval --all-targets --offline --locked --message-format=json -- -D warnings` | 0 | 63.275152 s |
| `cargo build --workspace --offline --locked --message-format=json` | 0 | 191.715816 s |
| `git diff --check` | 0 | 0.217519 s |

Full-package results are 378 library, 7 binary and 238 integration passes;
doctests contain zero cases. The reported target times are 1.49 s, 19.21 s and
55.94 s respectively. The command wall time includes compilation. All compiled
case names match actual terminal rows; none is ignored, filtered or unrun.
The single full package invocation includes the named case and entire relay
module; no extra normal case/module run and no failed-test retry occurred.
[census-and-outcomes.json](census-and-outcomes.json) retains each compiled name and actual result with
its executable digest. Both control artifacts independently listed the same
238-case integration census; their exact selected test was run once each.

The environment uses Rust 1.96.0, `x86_64-apple-darwin`, jobs 4, dev/test debug
0, incremental 0, offline/locked and `RUST_TEST_THREADS` unset. `file` identifies
the compiled test executables as Mach-O x86_64. The captured OS reports Darwin
x86_64; physical hardware architecture was not independently measured. No ARM
build, device or network pull was used. Only the existing assigned target
`/private/tmp/tests-codex-1569-target` was used, exclusively during this run.

## Source, replay and limits

The control compile command in each isolated owned worktree was:

```text
cargo test -p lamp-voice-eval --test it --offline --locked --no-run --message-format=json
```

The selected invocation was the saved artifact followed by
`relay_hardening::a_closed_runner_transport_stops_the_runtime_and_its_workers
--exact --nocapture`, with a distinct private `LAMP_RELAY_PID_CONTROL_DIR` and
an external 20-second watchdog. The complete exact commands, cwd, bounded
watchdogs and timings are in [command-results.json](command-results.json) and each control JSON.
The frozen driver's digest is recorded; its Python implementation and saved
binaries remain external. The readiness hooks themselves are fully included
as exact patch wrappers. Decode a patch without changing its bytes using
`jq -j .utf8 patches/before-control.patch.json > before-control.patch`, then
apply it only to the stated before source. The after patch targets the source
after [candidate.patch.json](patches/candidate.patch.json); each wrapper supplies its original SHA-256.

Before each worktree switch, all 10,002 tracked repository inputs (6,656 under
lampOS), including docs, had their mtimes refreshed and their byte aggregate
rechecked. Both control worktrees restored the complete clean base aggregate
in `finally`; the candidate aggregate stayed equal to the frozen source
through the final gate. No scratch hook was applied to the candidate. The
full raw input maps remain external with recorded digests. The immutable
[preexecution-source-proof.json](preexecution-source-proof.json) intentionally retains its earlier
`prepared_only`/zero-execution fields; actual outcomes are separate here.

All original assertions and deadlines remain: the 4-second elapsed assertion,
5-second closer watchdog/5-ms poll, 5-second session and allowance, and
3-second worker-absence check/20-ms poll. `read_pid` is unchanged. The observed
control times are not a speedup claim. This fixture-only fix does not change
product behavior, switches, topics, schemas, privacy or ownership rules;
no new device or product privacy qualification is claimed.

[native-streams.json](native-streams.json) includes exact complete stderr streams. Cargo stdout
contains a declared projection removing only Cargo build JSON records; exact
retained byte ranges, bytes, original full-stream hashes and projection hashes
are recorded. All test output is retained. Other stdout, including the raw
control failure, is lossless UTF-8 JSON. The full originals remain external;
local paths are not a promise of remote availability. No binary, whole-source
export, Python script or full Cargo JSON dump is included in this bundle.

This receipt remains bound to the original base and frozen two-file snapshot.
Later commits or rebases require their own current-base qualification. The
[manifest](manifest.json) records every payload file; [validation](validation.json)
records the source, stream, case-census, link and whitespace checks.
