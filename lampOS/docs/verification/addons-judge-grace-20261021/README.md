# Add-ons judge startup-grace verification, 2026-10-21

For the latest rebase onto `74b90c317`, use the
[compact rebase verification](../addons-rebase-20261022/README.md) and its
refreshed controls. The timings below remain historical measurements on
the base recorded here. The [earlier review verification](../addons-review-20261022/README.md)
retains its separate failed shared-cache attempt and successful rerun;
neither historical package measures the latest candidate.

## Scope and approvals

This extends Tests [PR #2647](https://github.com/autonomous-ai/lampOS/pull/2647).
Its previous head was `4e16dbed9316417b5f620352ad8ebc3688be26bf`.
Rebasing onto fetched `lamp-voice` at
`8029dbd30e171e609a706bbc622f27a781075510` produced
`9b577ce0f0ad361d2b68a83adba9642e7ff0b713`; `git range-diff` reported `=`.
The **before** measurements below use that rebased, unmodified PR head.
It includes #2577's restarted-voice reset and the newer kept-mood and
resting-person's-light tests. The **after** measurements and controls use
the working-tree change on that same base; exact source hashes are in
[source-sha256.json](source-sha256.json).

The [first builder](https://github.com/autonomous-ai/lampOS/issues/1806#issuecomment-6315652017)
and [Blue](https://github.com/autonomous-ai/lampOS/issues/1806#issuecomment-6316727896)
approved a test `serve` option for the never-said-mute wait, with the
product's default remaining `ASLEEP_FRESH_US` (3 seconds), help identifying
the option as test-only, and a said mute's freshness unchanged.

## What changed

- `serve --mute-grace-ms N` accepts 0..3000 milliseconds. The default is
  3000; the product service's command is unchanged and passes no override.
- `Config::mute_grace_us` reaches `Watcher::new(began_us, mute_grace_us)`.
  The wait starts again on a new voice publisher boot. It does not change
  `rules.rs` or the 3-second freshness of a mute that was actually said.
- The product-lamp test alone passes a zero grace. It no longer sleeps
  3.2 seconds before publishing its proposal.
- Tests publish the lamp's retained state before spawning the judge.
  Publisher teardown keeps Latest values; the judge reads them before
  processing its first proposal. This removes the 300 ms settling sleeps
  from the shared fixture and the kept-mood and resting-light tests.
  Existing retained-value expiry handling remains unchanged.
- The ring snapshot is taken immediately after the first decision, before
  observing 100 ms of quiet for duplicate decisions. A later ring request
  fails the helper. This checks the intended completion ordering while
  continuing to detect a duplicate decision delayed by 50 ms.

The previous helper drained ring requests after its 100 ms observation.
A light arriving 50 ms after its decision could therefore enter that
snapshot. The refreshed ordering controls below test the corrected
boundary; the earlier PR's timing-only claim is not used as proof of this
ordering. These are bounded observations, not proof about arbitrary
delays or the relative scheduling of messages that have both arrived
before the first decision is received.

## Measurement boundary

Host: macOS 26.6.2, x86_64. Rust's unoptimized test profile; one named test
per invocation of the compiled libtest executable. Each recorded wall
time covers process startup, test execution, teardown and captured output
bookkeeping. Compilation is excluded. Every original judge test is
measured three times before and after the change, sequentially, with the
same environment and commands. The new CLI test has no baseline counterpart.

Target: the complete product-lamp test finishes in less than one second
on this Mac. All three after runs meet that target. This measures fixture
duration and does not establish device response latency.

Commands run from `lampOS/` with:

```sh
export PATH=/Users/autonomous/.cargo/bin:$PATH
export CARGO_TARGET_DIR=/private/tmp/tests-codex-1569-target
export CARGO_BUILD_JOBS=4
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export CARGO_INCREMENTAL=0
cargo test -p lamp-addons --test judge --no-run --message-format=json
/private/tmp/tests-codex-1569-target/debug/deps/judge-118df33f9a03ac3c --list
/private/tmp/tests-codex-1569-target/debug/deps/judge-118df33f9a03ac3c TEST_NAME --exact --nocapture
```

All times below are seconds, in run order. Full test names and raw output
are retained in [before.txt](before.txt), [after.txt](after.txt),
[baseline-host-timings.json](baseline-host-timings.json) and
[after-host-timings.json](after-host-timings.json). Compilation and test
discovery output is in [compilation.txt](compilation.txt); the recorded
environment is in [environment.json](environment.json).

| Test | Before: 3 runs | After: 3 runs | Median before → after |
|---|---|---|---|
| Calm lamp's idle light | 1.0577 / 0.4966 / 0.4971 | 0.7504 / 0.1406 / 0.1395 | 0.4971 → 0.1406 |
| Kept-light decision | 0.4918 / 0.4989 / 0.4935 | 0.1443 / 0.1389 / 0.1428 | 0.4935 → 0.1428 |
| Kept mood | 0.3930 / 0.3937 / 0.3872 | 0.0414 / 0.0366 / 0.0378 | 0.3930 → 0.0378 |
| Product lamp, no pause/mute | 3.6924 / 3.6991 / 3.7015 | 0.1378 / 0.1373 / 0.1348 | 3.6991 → 0.1373 |
| Foreign proposal | 0.4936 / 0.4925 / 0.4968 | 0.1407 / 0.1434 / 0.1370 | 0.4936 → 0.1407 |
| Resting person's light | 0.3984 / 0.3955 / 0.3942 | 0.0420 / 0.0480 / 0.0422 | 0.3955 → 0.0422 |
| Person's own light wins | 0.4911 / 0.4972 / 0.5005 | 0.1394 / 0.1433 / 0.1440 | 0.4972 → 0.1433 |
| Switch off | 0.3475 / 0.3436 / 0.3409 | 0.0353 / 0.0331 / 0.0371 | 0.3436 → 0.0353 |
| New CLI bounds/help test | — | 0.0401 / 0.0421 / 0.0420 | — → 0.0420 |

All eight original tests passed 3/3 before; all nine passed 3/3 after.
The sum of the eight comparable, separate-process wall times was
**7.3655 / 6.8171 / 6.8117 s before** and
**1.4313 / 0.8212 / 0.8152 s after**. Including the new CLI test gives
1.4714 / 0.8633 / 0.8572 s after. These sums are not the elapsed time of a
concurrent whole-binary run. The first calm-lamp invocation was slower in
both series (1.0577 s before and 0.7504 s after); those first-use effects
are retained rather than discarded as warm-up.

## Controls

Deterministic boundary tests cover the default 3-second wait; zero and
200 ms startup/restart waits; repeat messages from the same voice boot;
and a said mute being fresh exactly 3 seconds later and stale one
microsecond after that. The CLI checks both help forms, both allowed
range boundaries, missing/invalid/out-of-range values and duplicate options.

All six mutants failed at their intended test assertions: **8/8 expected
failures**, with the two ordering mutants each run twice. Every command
returned 101 because its selected test failed, with no compile failure.
Each mutated file was restored byte for byte in `finally` before the next
case and before the final gates.

| Mutant | Intended failure | Result |
|---|---|---|
| Product default grace changed to zero | Default is 0 rather than 3,000,000 µs | 1/1 failed |
| Runtime ignores the CLI override | Product-lamp proposal is dropped instead of producing a light | 1/1 failed |
| Voice restart fails to renew grace | New voice boot becomes known awake before its new boundary | 1/1 failed |
| A said mute's age is refreshed | Its original reading timestamp is lost | 1/1 failed |
| Decision published 50 ms before its light | `a light was asked after its decision` | 2/2 failed |
| Decision duplicated after 50 ms | Two decisions observed for one proposal | 2/2 failed |

[controls.txt](controls.txt) contains the exact commands, raw failures and
run labels. The six `mutant-*.patch` files record the Rust mutations, each
relative to the restored candidate. To replay a control, apply just its
patch from the repository root with `git apply --unidiff-zero`, run its exact selected
test command from `lampOS/` with the environment above, then reverse that
patch with `git apply -R --unidiff-zero` before applying the next one. The two decision
controls each ran twice. The local measuring script stayed outside
`lampOS/`; its recorded run restored the changed source bytes in `finally`
and checked their SHA-256 hashes after each mutation.
[verification-results.json](verification-results.json) records every exit
code and wall time. Its original log filenames are the section labels
in the combined raw output files.

## Gates

The restored candidate passed these commands sequentially, with exit 0:

| Command | Result | Wall seconds |
|---|---|---:|
| `cargo fmt --all --check` | Passed | 10.3334 |
| `cargo clippy -q -p lamp-addons --all-targets --message-format=short -- -Dwarnings` | Passed | 8.0623 |
| `cargo test -q -p lamp-addons` | 29 library tests + 9 judge integration tests passed | 3.2970 |
| `cargo build -q --workspace` | Passed | 62.6647 |
| `git diff --check` | Passed | 0.0999 |

The crate gate's concurrent judge run reported 0.49 s. The raw output is
in [gates.txt](gates.txt). The required gates ran after all controls; only
this evidence documentation was added afterward.

## Environmental failures retained

`cargo nextest` was unavailable on this host. Measurements therefore use
the compiled libtest binary and its exact test names. An initial sandboxed
baseline invocation failed before behavior ran because binding its
temporary Unix socket returned `Operation not permitted`. Both failures
are retained in [environmental-failures.txt](environmental-failures.txt).
They are excluded from the successful behavioral measurement series.

## ARM64 queue context

The successful ARM64 [run #5191](https://github.com/autonomous-ai/lampOS/actions/runs/39633785316)
at head `0af798d348ba06f7163f7aac90b79a381263888c` ran **before PR #2647**.
Its eight judge test durations sum to 19.064 s, including 5.401 s for the
product-lamp case and 4.160 s with add-ons off. The extracted rows are in
[arm64-before-5191.json](arm64-before-5191.json). This explains the queue
priority; it is a separate machine and code baseline from the rebased
PR's Mac measurements above. It does not measure this change.

## Not shown

No native Linux ARM64 run of this candidate or device execution is included
here. New CI on the published PR head is still required. No product service unit,
deployed binary, mute freshness rule, or default output behavior was changed.
The ordinary binary does accept the explicit test-oriented grace override;
passing a smaller value shortens its absent-mute startup and restart wait.
The shipped service omits it. Unit tests cover the default boundary with
explicit timestamps, but the process fixture uses zero grace and does not
verify the default window or its forwarding by `serve`. The former 3.2 s
pause checked eventual acceptance, not rejection before the default boundary.
