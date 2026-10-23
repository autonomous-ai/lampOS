# Combined Voice failure source audit — 23 October 2026

This source-only audit does not establish a fold-specific cause or waive either failure. No tests, source changes, target reads or live reader-log reads were performed. Proposed controls below remain unexecuted and require a separate execution decision.

The completed Voice stage returned exit 101: **3,931 passed / 3 failed / 15 ignored**, with all six targets bound. The library had 3,651 passed / 2 failed / 15 ignored. The third failure was the separately tracked light-call latency assertion. This package does not claim completion or success of the overall combined gate.

## Source identity and reachability

The gate used boot carry `4e8ea747cd7dc745742283d6d08cb100297a8bfa` on base `09f8e7412f92870e16ccca14e1b9cdc1b14add48`, plus the reviewed integration peer fixture and combined documentation overlays. It was not an execution of a final combined commit. The full captured source aggregate was `f346596d5fa5ad67f19179739baf22ba50eb101a79e95e91f9e679d73501509d`. Prior reviewed heads were boot `f121d6391e53bf709ffbf07807e8657f7e86d3ce` and peer `55f671b12ef08c93f11ae235980f69cf7059b7a8`.

[The equality map](source-comparison.json) compares 24 selected files against all four Git refs and binds each to the captured gate source. Both asserting files and the selected coordinator, process, cleanup, transport, bus, controls privacy and IPC paths are identical. The peer overlay is integration-test code, outside the failing unit-test binary. This is source comparison, not binary equivalence or causal exclusion.

`scenario_clock.rs:41–50,118–147` keeps scopes in thread-local state and prevents moving them between threads. Turn-taking uses a thread-local Rc guard; StopWords' optional clock is per instance. The privacy fixture uses raw IPC/host clocks. Search's PID-scoped `lv-hand-loop` root is protected by its existing `one_session` lock; privacy uses a distinct `vpl` PID/counter root. No direct shared-clock or root overwrite path was identified. Shared-process scheduling/resource effects remain unclassified.

[The exact shared-helper delta](scope-enter-delta.json) remains relevant: default `Scope::enter` now samples raw monotonic time before taking `Instant::now`; the base did the reverse. Watchdog duration, stepping and cleanup code are unchanged. Preemption can change the cross-clock sampling skew; no such delta was measured in this run.

## Search boundary

`coordinator/tests/second_route.rs:384` failed `Run::assert_clean`, before its search assertions and before the unpushed comparison. The trace includes `look_dropped`, `search_pushed`, `hand_control_cancel` and `turn_finished`. It ends with `scenario host watchdog; verified cleanup failed: session Fixture: fixture child 3755 exit observation timed out (5 occurrences)`.

Child readiness precedes the 24-second host watchdog (`second_route.rs:38,1606–1608`); the scripted mute waits for capture frame 1,100 (`:2498`). `Peer::close_input` (`:457–465`) chooses that same scope deadline for exit observation. After the primary watchdog expires, this cleanup bound is already expired. `scenario_clock.rs:273–289` nevertheless probes once before checking it, so a child not yet observed exited can immediately produce the cleanup suffix. This does not prove a separate hung child or quantify its exit latency.

`coordinator/cleanup.rs:42–59,96–104` aggregates by stage and owner and retains the first error. **“5 occurrences” does not mean five waits on PID 3755.** The output lacks capture frame, logical elapsed time, per-role exit state and host budget remaining at the failure. It cannot distinguish slow progress, a stalled operation or another source of elapsed host time.

## Privacy boundary

`privacy_bus.rs:1313` exhausted ten attempts with `privacy publisher unavailable`. `notices` at `:477–484` maps either `Late` or `SourceGone` to that text. The raw block does not identify which notice, which read phase, or whether the before/after-first-second arm was active. It records no admission ages; the wording “host stalled” is the fixture's assertion message, not an independently measured cause.

A concrete timing path exists: this test intentionally receives only kept values, which do not update the bus's arrival time (`bus/subscribe.rs:423–433,528–563,638–639`). With the declared 10 ms period (`msgs/privacy.rs:43–46`), the bus can emit `Late` after 20 ms from subscriber construction even when a newly published kept lease is within its 50 ms freshness. `LeaseReader::poll` checks notices after draining (`privacy_bus.rs:534–552`). This explains a possible unavailable boundary; it does not prove which path occurred here.

## Proposed bounded diagnostics

- Privacy: record fixture phase, real elapsed time, current sequence and subscriber counters around the existing reads. A fixed 30 ms pause after reader construction but before fresh lease 2 publication can test the Late path on both base and candidate. Record actual lease age; preserve all assertions, retries, the 50 ms freshness and 500 ms read bound. Do not consume notices ahead of the production read. Unexpected expiry or setup failure remains a distinct outcome.
- Search: record capture frame, logical elapsed and host budget at push/mute/cleanup, and each role/PID at input closure. A one-shot pause through the original host deadline after the push can validate watchdog-to-expired-cleanup propagation on base and candidate. Such a control would demonstrate the boundary, not recreate the historical cause. Preserve the original 24-second watchdog, assertions and supervisor cleanup; no timeout widening or cleanup exemption is proposed.

## Evidence and decoding

[Failure blocks](failure-blocks.json) contain both complete original libtest failure blocks as UTF-8 JSON strings. Decode each `utf8` field and encode it as UTF-8 to recover its exact bytes, including trailing newlines. Check its byte length/SHA256 and the recorded zero-based half-open offsets against the original stdout. The same rule applies to `diff_utf8` in the scope delta. No diagnostic text was normalized.

[Provenance](provenance.json) binds the original command/status, six captured executable/target results and external source/census inputs. Full original streams and the complete source map are not duplicated here; their paths, lengths and SHA256 hashes are recorded. External paths alone are not portable availability. [The manifest](manifest.json) hashes every other file in this package and records validation. No Python, binary or Cargo machine-output stream is included.
