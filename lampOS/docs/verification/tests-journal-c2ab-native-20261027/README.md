# Journal c2ab: fresh original native CI

Fresh original attempt-1 CI is green for [#3622](https://github.com/autonomous-ai/lampOS/pull/3622): [Linux ARM64](https://github.com/autonomous-ai/lampOS/actions/runs/40405108925) and [Linux Clippy](https://github.com/autonomous-ai/lampOS/actions/runs/40405108853). Both checkouts used preview `741cb66ff45bdd2ce5b2d1793d7116321c90d267`, parents `a3b9c46814` and `c2ab4426aa`; its full tree equals committed c2ab. The seven actual workflow/configuration/Reader/fixture sources also match that head.

The ARM64 run has **7,541 unique PASS, 0 FAIL/LEAK/TIMEOUT**, with **53 skipped tests**, **70 binaries reported by nextest** and **114 skipped binaries**. It is the printed twelve-package selection in one partition (1/1), not a whole-workspace runtime result. Each selected identity occurs once in the execution window; no repeated execution or retry record was found. [audit.json](audit.json) includes per-binary counts, required identities and receipt hashes.

The journal case passed once. All fourteen ordinary Linux Reader tests, including the changed outside-sentinel case, passed once. The ignored cross-identity DAC control remains unrun; ordinary mode checks do not substitute for that proof. Individual skipped identities are not emitted, so only numeric skip counts are claimed. This workflow uploads no independent source-test inventory; selected coverage is derived from original execution identities and Starting/Summary counts.

Separate successful counts are promotion **25/0/0**, workspace doctests **4/0/0**, process custom harness **13/0**, and relay-signals `ok`. Publication/packaging auxiliaries and whole-workspace all-targets Clippy/formatting steps are green. General CI is absent under the actual workflow's lampOS-only path exclusions and is not claimed green.

Only artifact metadata was collected: the c2ab-named artifact is unexpired and its archive metadata digest is recorded. No ARM64 binary or archive was downloaded or verified. Old 2011 green outcomes are preserved separately and were not reused. No CI rerun/write, device qualification, performance comparison or required-check carry is claimed.
