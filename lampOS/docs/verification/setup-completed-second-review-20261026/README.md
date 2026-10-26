# PR #3570 focused host review

Exact executed head: `9531e2289fab2226fe932503ba5f924019a853d3`; parent: `690dff041e331b5c89160901b4f38f3a9915e60b`.

Independent isolated host gate: formatting, strict lamp-update all-target Clippy, then matching compile/runtime `cargo +1.96.0 test --locked --offline -p lamp-update setup_lifecycle` selection. Runtime adds `--no-fail-fast -- --test-threads=1`; compilation adds `--no-run --message-format=json`. All receipts exited zero with unchanged source. The 18 unique passing cases are two parser controls and sixteen lifecycle controls; other targets were filtered, not fully tested.

Read all 18 tests and changed implementation boundaries. No P1/P2 found in this scope. Positive controls assert preserved PID, no restart and changed link; negative controls retain restart/watch failure for uncertain overrides, custom units, manager aliases and reset. Recovery verifies truthful prior activity and restored links. This does not prove atomic reset coordination, bounded filesystem latency, real systemd behavior, device grace expiry or ARM64 execution.

Separate reviewer report: a broader round-suite invocation failed `a_build_for_a_newer_updater_is_refused_and_taken_once_the_updater_can` with `another lamp-update is at work`; its isolated replay passed. Cause is unproven. That failure is not erased or qualified by this focused 18-case run. No retries were performed here. Results belong to the frozen executed head, not a later carry.

Raw logs, source inventory and preserved binaries remain private scratch evidence; this packet contains their hashes, exact normalized commands, literal summaries and artifact bindings, with no binaries.

After runtime closure, the current public branch was fetched. `carry.json` records the exact published head and identical whole lampOS tree; this is carry evidence, not a new execution.
