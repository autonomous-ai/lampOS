# Add-ons review and rebase verification, 2026-10-22

For the subsequent rebase onto `74b90c317`, use the
[compact rebase verification](../addons-rebase-20261022/README.md) and its
refreshed mutation patches. This package remains historical evidence on
`729262749`, including its failed cache attempt and successful rerun.

The restored source at `2b53b580fd3511efec3a7aa315f37dadee7b804e`, rebased
onto `7292627491ad742ae50a259db4cd6a9cfe09a270`, passed all eight mutation
controls and all local gates. The successful run began at
**2026-10-21 19:02:49 UTC** (2026-10-22 locally). Its
[summary](passed/summary.json), [command records](passed/verification-results.json),
and [raw output](passed/commands-output.txt) are retained here.

This is refreshed correctness and build evidence for
[PR #2647](https://github.com/autonomous-ai/lampOS/pull/2647).
The [2026-10-21 timing series](../addons-judge-grace-20261021/README.md)
remains historical evidence on its documented base. No new before/after
fixture timing series was collected on this rebase. Gate wall times below
include compilation where the command performs it; they are not fixture
latency measurements.

## Approved option and the review limitation

The [first builder](https://github.com/autonomous-ai/lampOS/issues/1806#issuecomment-6315652017)
and [Blue](https://github.com/autonomous-ai/lampOS/issues/1806#issuecomment-6316727896)
approved a test `serve` option for the wait before a never-said `voice/mute`
counts as absent, retaining the product's 3-second default, identifying the
option as test-oriented in help, and leaving an actual mute's freshness
unchanged.

The reviewer-lead's P3 (comment 6323257736) identifies an explicit deployment
override and a process-coverage limit. The ordinary binary accepts
`--mute-grace-ms 0..3000`; no separate test mode enforces the intended use.
Explicitly passing a smaller value shortens the absent-mute wait both at
startup and after a new voice publisher boot. The shipped service passes no
override, so its default remains **3000 ms**. An actual mute retains its
original timestamp and independent **3-second freshness**.

The product-lamp process fixture passes zero grace. Unit tests using explicit
timestamps check the default boundary, startup/restart behavior, and actual
mute freshness. The process fixture does **not** verify the omitted-option
3-second window or that `serve` forwards the default into `Watcher`; it has
no injected clock. `--clock-id` supplies identity, not clock advancement.
A regression hard-coding zero in the runtime's watcher construction is
therefore outside that process coverage. The former 3.2-second pause only
checked eventual acceptance and did not establish rejection before 3 seconds.
This rebase preserves the approved option and states that limitation.

## Rebase integration

The [range diff](rebase-range-diff.txt) compares the original published change
at `0f961123bae4ef804acd04ac53db6e074d2dd161` against the rebased change.
Its text rendering removes generated trailing padding; the
[raw JSON record](rebase-range-diff-raw.json) preserves the exact output.
The rebase retains upstream #2530 part 2a: the `addon/ask` publisher,
`policy/decisions` subscription, pending say state, policy answer handling,
and the new `say_through` helper/test bodies. The mute-grace CLI test remains
a separate appended test.

Both `--say-quiet-ms` and `--mute-grace-ms` appear in help and retain their
parser branches. The custom `Config::default()` includes `say_quiet: None`
(the upstream 15-second say quiet period) and
`mute_grace_us: ASLEEP_FRESH_US` (3 seconds). `Watcher::default()` includes
upstream's `resting_since_us: None`. Runtime imports retain both `Drop` and
`ASLEEP_FRESH_US`. The light/mood fixture documentation is qualified so it
does not describe the upstream say fixture as publishing before startup.

## Refreshed controls

The [prepared control manifest](prepared-controls.json) retains the eight
exact commands, selected test names, patch hashes, and adaptation notes.
Four patches preserve the original replacement text with refreshed context.
Upstream moved decision publication into `write()`, so the two ordering
patches move with that code: the early-decision patch keeps its original
synthetic decision and 50 ms delay before `decide()` and removes the normal
later `write()` call; the duplicate patch adds the same extra publication
and 50 ms delay immediately before the normal publication inside `write()`.
The tested defects, delays, selected tests, and repeat counts are unchanged.

| Mutation | Required diagnostic | Final result |
|---|---|---|
| [Default grace zero](patches/mutant-default-grace-zero.patch) | Default 0 instead of 3,000,000 microseconds | 1/1 intended failure |
| [Runtime ignores override](patches/mutant-runtime-ignores-override.patch) | No light; decision says asleep or unknown | 1/1 intended failure |
| [Restart does not renew grace](patches/mutant-restart-does-not-renew-grace.patch) | Awake before the restarted voice's boundary | 1/1 intended failure |
| [Actual mute's age refreshed](patches/mutant-said-mute-age-refreshed.patch) | Reading timestamp 4,000,000 instead of 1,000,000 | 1/1 intended failure |
| [Decision before light, 50 ms](patches/mutant-decision-before-light-50ms.patch) | `a light was asked after its decision` | 2/2 intended failures |
| [Duplicate decision, 50 ms](patches/mutant-decision-duplicated-50ms.patch) | Two kept decisions with the same proposal ID | 2/2 intended failures |

Each run required exit 101, exactly one selected test failed, the named
test's panic, and the specific diagnostic. Compilation failure, zero tests,
a different failing test, or an unrelated assertion could not qualify.
All eight final runs satisfied those checks. The
[raw output](passed/commands-output.txt) includes every control and patch
application check, with its exact command, exit, status, and wall time.

Exact source bytes were restored after **each** attempt. All eight
[restoration records](passed/restorations.json) have equal before/after
hashes. The final [source hashes before](passed/source-before-sha256.json)
and [after](passed/source-after-sha256.json) are identical and match the
source packaged with this review. Source snapshots themselves are not
committed; their hashes were checked while packaging these records.

To replay one control, start from the restored source, apply only its patch
with `git apply` from the repository root, run its exact command from
`lampOS/` using the environment below, and restore the file before another
control or any gate. The two ordering controls each run twice.

## Local gates

Host: macOS 26.6.2, x86_64. The [environment record](passed/environment.json)
retains the execution environment. Cargo used
`CARGO_TARGET_DIR=/private/tmp/tests-codex-1569-target`, jobs 4, dev/test
debug information 0, incremental compilation 0, and offline mode. Commands
ran from `lampOS/` after all mutations were restored.

| Command | Result | Wall seconds |
|---|---|---:|
| `cargo fmt --all --check` | Exit 0 | 10.4175 |
| `cargo clippy -q -p lamp-addons --all-targets --message-format=short -- -Dwarnings` | Exit 0 | 5.3885 |
| `cargo test -q -p lamp-addons` | 35 library + 10 integration tests; exit 0 | 7.7682 |
| `cargo build -q --workspace` | Exit 0 | 53.3039 |
| `git diff --check` | Exit 0 | 0.0837 |

Offline Cargo metadata found [no workspace reverse dependents](passed/reverse-dependents.json)
of `lamp-addons`; no additional packages were required for this package's
Clippy/test gate. The exact metadata command and successful status are
retained in the command records. Bulk metadata stdout is explicitly omitted.

## Failed attempt and cache recovery

The first run began at **2026-10-21 18:58:33 UTC**. Its eight controls,
formatting, Clippy, and 35 + 10 tests passed, but the workspace build failed
with exit 101 after 1.5096 seconds. It reported unresolved
`lamp_msgs::AddonAsk` and a missing `Decision.proposal`, although both were
present in the checked-out source. The diagnosis was a stale shared-target
workspace-profile `lamp-msgs` artifact. That attempt is retained as
[failed](failed-cache-attempt/summary.json), with its
[complete command records](failed-cache-attempt/verification-results.json)
and [raw control/gate output](failed-cache-attempt/commands-output.txt).
The run stopped at the failed build; its final diff gate did not run.

Before the successful rerun, timestamps were refreshed for **1,314 tracked
workspace crate and root Cargo/toolchain files**, retaining registry
dependency artifacts. [source-freshness.json](source-freshness.json) records
the same whole-content SHA-256 before and after:
`efb20a92ed31bf8f8fa4920ace7122108a04e743f011fe1ca808083284f3ff88`.
That digest combines each tracked relative path, a NUL separator, and the
binary SHA-256 digest of its bytes, in `git ls-files` order. Only timestamps
changed. All eight controls and all gates were then rerun successfully;
the first attempt's successful stages were not substituted for the rerun.

## Artifact boundaries and remaining validation

Both runs retain their original JSON command records. Their `log` fields
name the original external log files; each run's
[output index](passed/output-index.json) maps those names to sections in
`commands-output.txt` and records the original byte count and SHA-256.
The [failed attempt has its own index](failed-cache-attempt/output-index.json).
Every control/gate log's raw bytes are retained unchanged. Only bulk Cargo
metadata stdout is omitted; its command/status and dependency closure remain.
[Artifact provenance](artifact-provenance.json) records the copied inputs.
No Python runner, raw source snapshot, compiled binary, or target artifact
is included in this package.

The preparation manifest's `unrun` field describes its state when prepared;
the two run summaries record actual execution. The retained range diff ends
at the tested source commit. The subsequent documentation qualification and
this evidence package do not change the tested Rust source.

Native Linux ARM64 CI on the updated published PR head remains pending.
No device execution or device latency measurement is claimed here.
