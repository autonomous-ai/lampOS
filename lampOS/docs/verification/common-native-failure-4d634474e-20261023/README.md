# Common #3064 — exact-head native CI failure

Head `4d634474e740bd093430ac5ab62801fa9629f014`, tree `137baf4f76e4caf0af5bb436c4e102943b4b0721`, parent `be16c3327524ccef8a1908a3b8ebf40c16efba8a`.

[ARM64 39978630852](https://github.com/autonomous-ai/lampOS/actions/runs/39978630852) **failed**. [Strict workspace/all-target Clippy 39978630861](https://github.com/autonomous-ai/lampOS/actions/runs/39978630861) **passed**. Both ran attempt 1 and checked out merge `efd5080bab8fd656e690d4af65d06ca9fc805c2b`, with an executed tree exactly equal to the author tree. No retry or dispatch was issued by this collection.

Nextest: **5,213 passed / 1 failed / 69 skipped**; **5,214 of 11,205 selected tests ran**, leaving **5,991 unrun** after fail-fast. There were zero leaky results. Profile `pr` currently has `default-filter = 'all()'`; 166 binaries were selected and two custom-harness binaries were excluded for separate execution.

The failing test is `lamp-skills::it package::a_peer_that_is_not_an_asker_is_refused_and_nothing_is_removed`. Its shared helper `ask_helper_as` panicked at `lampOS/crates/skills/tests/it/package.rs:1004` when `ours.write_all(line).unwrap()` received `BrokenPipe` (OS error 32). The test at line 1030 calls that helper with peer authorization disabled and never reaches the unsuccessful-child-status, refused-response and unchanged-file assertions. The entire file has identical blob `7f8c383e042a320b9b582c167aaa4a9dc897c682` at the current head, its base and prior Common `00c79072298c583c88ac92a0cad42d2f9155520c`. Common changes no Skills path. This establishes source identity and the failing operation; **the runtime cause is not yet proved**.

The unchanged production `asked` branch at `lampOS/crates/skills/src/main.rs:378–396` can refuse an unauthorized peer before reading a request, print the refusal and exit unsuccessfully. This source permits an early-refusal/write interleaving; the failed helper did not capture the child status or journal to establish that sequence on this run. A bounded follow-up, after owner assignment, is fixture-only early-refusal handling with a deterministic control, preserving the exact refusal JSON, unsuccessful-child-status and unchanged-file assertions and reliable child cleanup. Root is routing the failure through related Skills owner issue #1685; no fixture change or reproduction was performed here.

All **80 Common controls** and all **41 current-base Update controls** remained **unrun**. The shared scratch census is one overlap, so **120 distinct requested controls** are unrun; no old result is inherited. Doctests, relay signals and the turn-end-wire process harness were **not executed** because their CI step was skipped. Artifact export/upload were also skipped: there is **no artifact** to checksum or inspect.

Observed commands before failure, relative to the lampOS workspace:

```sh
cargo build --locked --release --workspace --bins --examples --message-format=json-render-diagnostics > arm64-build.jsonl
cargo build --locked --release -p lamp-schedule --example schedule_check --target-dir target/lampos-artifact-examples --message-format=json-render-diagnostics > arm64-schedule-example.jsonl
cargo build --locked --release -p lamp-expression --example replay_cost --target-dir target/lampos-artifact-examples --message-format=json-render-diagnostics > arm64-expression-example.jsonl
filter='not (binary_id(lamp-ipc::relay_signals) | binary_id(lamp-turn-end-wire::process))'
cargo nextest run --locked --release --workspace --profile pr --no-tests=pass -j num-cpus --partition hash:1/1 -E "$filter"
cargo clippy --locked --workspace --all-targets -- -D warnings
```

Separate host evidence published in commit `0a47224dd2` reports Mac Update 372/0/4, Linux Update 374/0/2, and nine privacy tests passed on each platform. Those local results do not turn this native failure into a pass. No readiness, activation or device claim is made.

The accompanying [JSON](receipt.json) contains the exact checkout, counts, unrun control/source identities and sanitized failure boundary. No absolute personal paths or raw logs are included.
