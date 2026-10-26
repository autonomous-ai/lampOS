# Original native CI evidence for three PRs

These are the original attempt-1 runs for [#3548](https://github.com/autonomous-ai/lampOS/pull/3548), [#3543](https://github.com/autonomous-ai/lampOS/pull/3543), and [#3466](https://github.com/autonomous-ai/lampOS/pull/3466). [audit.json](audit.json) records preview trees/parents, per-binary summaries, required test identities and original receipt hashes.

| PR | Authored head | Actual preview base | Native PASS / FAIL / skipped | Runtime scope |
| --- | --- | --- | --- | --- |
| Ready #3548 | `f8f1e7a5f` | `008e7a974` | 7,534 / 0 / 53 | Selected twelve packages |
| Sentinel #3543 | `7cafe341a6` | `978cb0e983` | 7,535 / 0 / 53 | Selected twelve packages |
| Carrier #3466 | `09b75f05a` | `e5889ebb53` | 11,417 / 0 / 106 | Workspace ordinary nextest selection |

Every executed identity occurs once, with no FAIL, LEAK, TIMEOUT or retry record. Each run uses one partition (1/1). Linux whole-workspace/all-targets Clippy, formatting and general CI are green. Compilation covers the workspace even where runtime selection is narrowed.

The Ready test `runtime::unplug_tests::ready_presents_sleep_even_while_voice_speaks_then_acknowledges` passes once. Sentinel's changed `session_reader::tests::linux::replaced_record_never_changes_an_outside_sentinel` and all fourteen ordinary Linux Reader tests pass once. The privileged `session_reader::tests::linux::dedicated_reader_can_only_read_new_events_under_dropped_identity` remains ignored/unrun; ordinary mode checks do not prove cross-identity DAC behavior. The non-Linux fallback test is also outside these Linux runs.

Carrier passes all twelve product-carrier tests, thirty-four round head controls plus the separate install head control, sixteen setup-lifecycle tests and nine additional install head-preflight controls. The three earlier Mac-parallel self-cut failures each pass once in this native run. Their identities remain in the audit.

Each run separately passes promotion tests (25), workspace doctests (4), the relay-signals custom harness (`ok`) and the process custom harness (13). Those counts are not added to nextest totals; the two custom harnesses are excluded from nextest selection.

Offline Git tree comparisons measure `tree(c7666c044) = tree(f8f1e7a5f)`, `tree(dc47ee2f9) = tree(8edc8aa02)`, and `tree(f71bf8826) = tree(d645d5698)`. The original preview tree OIDs are bound by collected commit metadata; the equal tree objects and comparison commits are available locally. These equalities establish tracked source only. They assert neither binary equivalence nor required-check carry.

Coverage is derived from original job-log identities and selected Starting/Summary counts. This workflow uploads no independent source-test inventory, and individual skipped identities are absent from its logs. Numeric skipped counts are reported without invented identities. No ARM64 binary artifact or CI rerun was needed. The initial collector network denial is retained separately; later read-only collection succeeded. That access failure is not a CI failure.

The prior Mac parallel result (439 passed, 3 failed, 6 ignored) remains failed and retained. This proof does not waive it, establish product causality, or qualify subsequent `bdc`-base heads/current latest-base gates.
