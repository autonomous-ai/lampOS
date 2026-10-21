# Add-ons rebase verification, 2026-10-22

[PR #2647](https://github.com/autonomous-ai/lampOS/pull/2647), source
`adfcad8f63f23e2dc337c4cdcb5166637d6388b2` on
`74b90c317b39b0320fcf342e40a232240578a626`, passed all eight expected-failure
controls and the sequential local gates below. The run lasted from
2026-10-22 00:06:54 to 00:08:40 UTC on macOS 26.6.2, x86_64,
Rust 1.96.0 (`ac68faa20c58cbccd01ee7208bf3b6e93a7d7f96`). Only evidence
documentation was added after verification.

## Rebase boundaries

The previous change was `8ee2219b5a97bf84d57641fcfc786498524b3de0` on
`7292627491ad742ae50a259db4cd6a9cfe09a270`. This was a resolved rebase,
not an unchanged range diff: the range-diff header is `1: 8ee2219b5 !
1: adfcad8f6`. The exact command and full external output's digest are
recorded in [results.json](results.json).

The resolution keeps upstream move/say handling, body socket, quiet options,
decision journal, and all-input subscription readiness. All **11 upstream
judge tests** remain, alongside the mute-grace CLI test. `listening()`,
`moved()`, `say_through()`, the two move test bodies, and the policy-answer
test body remain unchanged from the new base; their existing quiet waits
remain. The new default still includes `move_quiet: None` and
`body_socket: None` as well as upstream say configuration.

The light/mood fixtures publish retained calm state before starting the
judge, then wait for its final `policy/decisions` subscription. It is bound
after all lamp-input subscriptions; retained inputs are folded before the
first proposal is judged. The privacy guard now publishes its first value
synchronously on the same publisher before starting its 20 ms renewal
thread. Keeping that guard alive prevents both the initial publication
race and a stale privacy value during a loaded runner's startup. Existing
retained-value expiry handling is unchanged.

The first decision triggers an immediate ring snapshot. The original
100 ms duplicate-decision observation follows, with an assertion that no
late ring request arrived. The two 50 ms ordering controls below exercise
that boundary. This is a bounded observation: it does not prove ordering
for arbitrary delays or when both messages arrive before the test receives
the first decision. The disabled fixture waits for the child's exit and
checks the absence of its proposal subscription.

## Grace and coverage limit

The [first builder](https://github.com/autonomous-ai/lampOS/issues/1806#issuecomment-6315652017)
and [Blue](https://github.com/autonomous-ai/lampOS/issues/1806#issuecomment-6316727896)
approved the test startup-grace option. Help labels `--mute-grace-ms` as
test-oriented; the ordinary binary accepts 0..3000 ms without a separate
test mode. The shipped service omits it and the default remains **3 s**.
An explicit smaller value shortens the never-said-mute wait at startup
and after a new voice boot. An actual mute keeps its original timestamp
and independent **3 s freshness**.

Unit tests check the default, startup/restart boundaries, and actual mute
freshness with explicit timestamps. The product-lamp process test passes
zero grace. As recorded in the [P3 review](https://github.com/autonomous-ai/lampOS/pull/2647#issuecomment-6323257736),
it does **not** verify the omitted-option 3 s window or `serve` forwarding
the default into `Watcher`. Hard-coding zero in that construction is
outside this process coverage. The former 3.2 s pause checked eventual
acceptance, not rejection before the default boundary.

## Controls and restoration

All eight controls compiled and returned **101 at the intended assertion**,
with exactly one selected test failed, zero passed and zero ignored. The
default-zero mutant's unused-import warning is preserved in the raw log;
it did not prevent compilation. Exact commands, exits, elapsed times and
log hashes are in [results.json](results.json); all eight complete failed
outputs are in [controls.txt](controls.txt).

| Mutation | Checked failure | Runs |
|---|---|---:|
| [Default zero](patches/mutant-default-grace-zero.patch) | 0 instead of 3,000,000 microseconds | 1 |
| [Override ignored](patches/mutant-runtime-ignores-override.patch) | No light; decision says asleep/unknown | 1 |
| [Restart grace not renewed](patches/mutant-restart-does-not-renew-grace.patch) | Awake before the new voice's boundary | 1 |
| [Actual mute age refreshed](patches/mutant-said-mute-age-refreshed.patch) | Timestamp 4,000,000 instead of 1,000,000 | 1 |
| [Decision 50 ms before light](patches/mutant-decision-before-light-50ms.patch) | `a light was asked after its decision` | 2 |
| [Duplicate decision after 50 ms](patches/mutant-decision-duplicated-50ms.patch) | Two kept decisions for one proposal | 2 |

Each patch applies independently to the verified source. From the repo
root, run `git apply --check PATCH` then `git apply PATCH`; run that
control's exact command from `lampOS/`; restore with `git apply -R PATCH`
and check its source hash before another control or a gate. Do not stack
the patches. The recorded runner restored the exact original bytes in
`finally` after each attempt. All eight restoration hashes match, and all
13 final source/configuration/document hashes equal the prepared and
pre-control hashes: [source-hashes.json](source-hashes.json).

## Local gates

Commands ran from `lampOS/` (the diff check from the repository root), with:

```sh
export PATH=/Users/autonomous/.cargo/bin:$PATH
export CARGO_TARGET_DIR=/private/tmp/tests-codex-1569-target
export CARGO_BUILD_JOBS=4 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_INCREMENTAL=0 CARGO_NET_OFFLINE=true CARGO_TERM_COLOR=never RUST_BACKTRACE=0
```

No cross target or test-thread override was set. Before the first Cargo
command, all **1,322 tracked crate and root Cargo/toolchain files** had
their mtimes refreshed to avoid stale shared-target reuse; the aggregate
SHA-256 was unchanged:
`7d07c23ce2c5e2aa1760bdda6fc77562553e84f9ea5ef7b434df00920c089421`.
It hashes each sorted relative path, NUL, content length as a big-endian
64-bit integer, and exact content bytes. Only timestamps changed.

`cargo metadata --format-version 1 --no-deps --offline --locked` confirmed
that `lamp-addons` has no workspace reverse dependents. After all controls
were restored, these commands passed sequentially with exit 0:

| Command | Result | Wall seconds |
|---|---|---:|
| `cargo fmt --all -- --check` | Passed | 10.5649 |
| `cargo clippy -q -p lamp-addons --all-targets --message-format=short -- -Dwarnings` | Passed | 5.8962 |
| `cargo test -q -p lamp-addons` | 38 library + 12 judge; 50 passed, none failed/ignored | 8.8936 |
| `cargo build -q --workspace` | Passed | 55.1358 |
| `git diff --check` | Passed | 0.4872 |

The judge suite reported 5.94 s. Command wall times include compilation
where applicable; this run is correctness/build verification, with **no
new before/after latency series**. The [original timings](../addons-judge-grace-20261021/README.md)
and [earlier rebase/cache recovery](../addons-review-20261022/README.md)
remain historical evidence on their own bases. They do not measure this
new candidate. No native Linux ARM64 run or device execution of this
candidate is included here; CI must identify its exact published head.

## Artifact scope

This package retains the compact command/result record, all failed-control
bytes, six replayable Rust patches and relevant hashes. Every original
command log has its byte count and SHA-256 in `results.json`; nonempty
success output, including full Cargo metadata, remains outside the repo
at `/private/tmp/tests-codex-addons-rebase-20261022/run-20261022T000654.384280Z/`.
The same record identifies the complete range diff, all per-file freshness
records, source snapshots' restoration hashes and the external runner
hashes. These external files are local scratch artifacts, not repository
links or guaranteed permanent storage. No Python source, bulk metadata,
1,322-file hash listing, build output or binary is committed here.
