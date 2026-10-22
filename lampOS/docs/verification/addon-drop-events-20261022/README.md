# Add-on drop completion, 22 October 2026

Part of #1806. Test-only source overlay on `464438fc549e528f1d812e25c14c459e4d609379`.
Only `a_fifth_say` in `crates/skills/tests/it/addon.rs` and its explanation in
`docs/skills.md` changed. Every byte outside that helper, including all three
original assertions, is unchanged. No product behavior or duration changed.

The full [ARM64 run #6055](https://github.com/autonomous-ai/lampOS/actions/runs/39804008782)
at `16b1a2a15` selected the work: `lamp-skills::it` summed to 97.352 s and
`says_the_judge_dropped_do_not_spend_the_hour` took 5.632 s. Those sums use
unique nextest result records before each shard summary, excluding compilation
and repeated diagnostic output. They are selection data, not an ARM64 after measurement.

The fixture now subscribes before launch, observes the first four Done events,
and serializes resting probes until all four show the real 300 s drop refusal.
The fifth request waits for that evidence. Its completion and the final four
Done events precede the journal read; the program writes each line before its
event. An eight-second host budget starts before the first proposals and is
checked before new probes and after collected completions. The ten-second
per-add-on interval, four-per-hour cap, 300-second drop hold, and exact
`proposals.recv(Duration::from_secs(3))` absence observation are unchanged.

## Fixed control plan

Native x86_64 macOS, on a shared host with other work using separate targets.
The small target was exclusive to this work. All 6,422 tracked lampOS input
mtimes were refreshed before switching from the previous worktree; content
hashes before and after that refresh were equal. Each compiler/run records its
source hashes. `source.json` records the final frozen aggregate and scoped hashes.

One prewritten sequence ran once: clean before ×3, paused before ×3, paused
after ×3, clean after ×3, then the four listed mutation runs. No sample was
discarded or retried. Whole selected-test process times, including fixture
startup and teardown, were:

| Sample | Before, s | After, s |
|---|---:|---:|
| 1 | 9.2561 | 5.8134 |
| 2 | 6.7993 | 3.7541 |
| 3 | 6.3591 | 3.8017 |

Both clean sets passed 3/3. These shared-host observations are not device
latency or a claim about the ARM64 run's after time.

The scratch pause runs **after the first valid drop message is received and
before time sampling, acquisition of the Proposals mutex, or decision
consumption**. Only that decision worker waits; the trigger loop stays live.
The helper gives its own child a unique gate path and prints its PID/path.
The external controller waits for that child's positive ready marker, checks
the PID, waits exactly 2.0 s on its monotonic clock, then writes the release
file. Actual recorded holds were 2.008–2.036 s. No process searches or pause
signals are used. The controller's emergency cleanup targets only its own
test process and the recorded fixture child; no cleanup was needed.

- Original helper plus pause: **3/3 fail at the original fifth-proposal assertion**.
- Candidate plus identical pause: **3/3 pass**.
- Refund disabled, retaining the hour's says while still recording the drop:
  the candidate fails the original fifth-proposal assertion.
- Drop hold disabled: the candidate fails its real bounded positive barrier;
  the same mutation with the original helper fails the original hold-journal
  assertion. The earlier candidate failure is stated separately, not relabeled.
- Hour cap bypassed: the candidate fails the original undecided-hour assertion.

Every negative exited 101. `raw-output.json` preserves every original UTF-8
byte of all 16 selected-test outputs, including the complete 1.66 MB barrier
failure, with byte counts and SHA-256. JSON escaping preserves trailing
whitespace and newlines. `observations.json` records times, PIDs, release
boundaries, source/artifact hashes and exits. The compact excerpts below are
derived from those streams; they do not replace them.

## Replaying the controls

Use a disposable worktree at the full base above. `fixture.patch.json` applies the
candidate and documentation; each semantic mutation patch applies on top of
that, except the original-helper hold control which applies `hold-disabled.patch.json`
directly to the base. Each pause patch applies directly to the base and contains
both its exact fixture variant and the scratch decision-worker hook. All hooks
and mutations are excluded from the final source. Patch JSON stores the exact
UTF-8 patch bytes, count and SHA-256, avoiding whitespace changes to blank
context lines. Extract without an extra newline, for example:

```sh
jq -j '.utf8' before-pause.patch.json | git apply
```

Compile each variant with:

```sh
cargo test --locked --offline -p lamp-skills --test it --no-run --message-format=json
```

Run the emitted `it` executable with
`addon::says_the_judge_dropped_do_not_spend_the_hour --exact --nocapture --test-threads=1`.
For a pause variant, read `ADDON_CONTROL child=PID gate=PATH` from test output;
wait for `PATH.ready` to contain that exact PID; after 2.0 monotonic seconds
write `PATH.release`. The test prints `ADDON_CONTROL_RESUMED` with PID and
actual held seconds. Preserve the complete output and exit status. The gate's
own safety bound is 12 s; the controller's whole-test watchdog is 45 s.

The exact argv are in `commands.json`. Native environment: two compiler jobs,
dev/test debug information disabled, incremental disabled, color disabled,
backtraces disabled, no RUST_TEST_THREADS override, isolated Cargo target.
The external Python orchestration is not committed; `plan.json` records its
digest and fixed sequence. No physical recording, binary, or full Cargo JSON
stream is committed.

## Gates on restored source

Both product files were restored byte-for-byte before these gates. All tracked
lampOS source contents were unchanged throughout the gate sequence.

- `cargo fmt --all --check`: exit 0; 11.758 s.
- `cargo clippy --locked --offline -p lamp-skills --all-targets -- -D warnings`: exit 0; 61.820 s.
- `cargo test --locked --offline -p lamp-skills --test it addon:: -- --test-threads=1`: exit 0; 42.249 s. test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 282 filtered out; finished in 17.83s
- `cargo test --locked --offline -p lamp-skills -- --test-threads=1`: exit 0; 329.359 s. test result: ok. 155 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 19.91s; test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 10.85s; test result: ok. 290 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 270.90s; test result: ok. 31 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.76s; test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
- `cargo build --locked --offline --workspace`: exit 0; 112.144 s.
- `git diff --check`: exit 0; 1.971 s.

The initial `cargo fmt` attempt could not find Cargo on PATH (exit 127; no
Cargo invocation). The first explicit-path fmt check found one Vec-collection
layout issue, which was corrected; the candidate and final fmt checks passed.
Those original diagnostics are retained. No test failure was discarded.

Linux ARM64 and x86 Linux Clippy qualification belong to the eventual PR head;
they were not run by this native receipt. No ARM64 build on the Mac, SSH,
device operation, source push, or merge is claimed here.
