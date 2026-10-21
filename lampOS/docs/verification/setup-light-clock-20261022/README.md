# Setup-light stop on its fixture clock, 22 October 2026

Part of #1806. Kind: test harness only. Base:
`4efcf4eb0cda0ef730d30658572f49da7592a0b8` on `lamp-voice`.

`light::stopping_ends_with_black_and_a_closed_ring` now constructs its light
with `Light::spawn_with_clock`, the unchanged `RETRY`, and this module's
existing `Fast` clock, as adjacent fade tests already do. Every original
observation and assertion remains: wait until white is shown, require a
successful stop, then require the exact newest-first tail
`[Closed, Frame(BLACK)]`. The painter ends when it has closed the ring.

Only the fixture's clock selection changes. `Fast` advances by the frame
duration and pauses for one fiftieth of it. The clock implementation,
production fade constants, painter and real two-second stop deadline are
unchanged. The production source and changed fixture are hashed in
[source-sha256.json](source-sha256.json); product bytes and the assertion
suffix were checked against the base before measurement.

## Same-base measurements

One exact test per compiled libtest process, three serial runs each:

| Before (s) | After (s) |
|---|---|
| 2.1536 / 1.4018 / 1.4128 | 0.7302 / 0.0482 / 0.0487 |

All six runs passed. Wall time includes process launch, execution, cleanup
and captured output; compilation is excluded. The planned target was
under 0.5 s: repeated invocations meet it, while the first invocation is
retained as a miss. These are fixture durations, not lamp latency.
[Before timings](before-host-timings.json), [after timings](after-host-timings.json),
[before output](before.txt), [after output](after.txt).

Full-main [ARM64 #5220](https://github.com/autonomous-ai/lampOS/actions/runs/39639887119)
at `b87b3e91ad04b5811fe441efa118773d33416630` recorded 1.358 s.
[Extracted row](arm64-before-5220.json) is before-only context from another
base and machine; the relevant source files are unchanged between that
base and this branch's base. Native results are recorded on the PR before
it is marked ready.

## Negative control

The temporary [stop-without-black mutation](mutant-stop-without-black.patch)
marked the painter ended and returned immediately when stopped, skipping
its fade and shutdown. It compiled, then the exact test failed once at its
original final-tail assertion: the closed ring's last frame was
`Rgb { red: 8, green: 7, blue: 5 }`, not black. [Raw output](mutation-controls.txt).
Both source files were restored byte for byte before the final gate and
checked again afterward. No product mutation is shipped.

## Validation and replay

Passed from `lampOS/`, in recorded command order:

```text
cargo fmt --all --check
cargo clippy -q -p lamp-setup --all-targets --message-format=short -- -Dwarnings
cargo test -q -p lamp-setup
cargo build -q --workspace
git diff --check
cargo clippy -q -p lamp-cloud --all-targets --message-format=short -- -Dwarnings
cargo test -q -p lamp-cloud
```

Setup: **157 passed, zero failed**, one existing ignored manual
`serve::write_a_reports_answer_fixture`. Cloud, which uses setup as a
development dependency: **165 passed, zero failed**, one existing ignored
manual `reports::tests::write_a_reports_fixture`. No ignore was added.
[Gate output](local-gate.txt).

[Environment](environment.json) records the host, Rust and build settings.
[Commands](commands.json) records each argv, exit code and duration;
stdout/stderr names identify sections in the grouped transcripts. All
test/gate output and compiler stderr are retained. Compiler stdout keeps
the tested executable's artifact and build-finished records, omitting
unrelated dependency metadata.

Measuring scripts remain outside the repository. Reproduce by compiling
each version with `cargo test -p lamp-setup --test it --no-run
--message-format=json`, finding the `it` executable, and running the full
test name above with `--exact --nocapture` three times serially. Measure
each subprocess with a monotonic host clock. Apply the mutation separately
over the fixture change, recompile and verify its expected failure, then
restore product bytes before the final gates. No ARM64 build ran on the
Mac; no device was contacted.
