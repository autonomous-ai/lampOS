# Fourth's successful native ARM64 runs, 2026-10-14

Complete, unmodified `gh run view <run> --repo autonomous-ai/lampOS --log`
output is retained beside this file. These are GitHub-hosted Ubuntu 22.04
ARM64 ordinary-user runners, Rust 1.96.0, release mode; no lamp was used.
The workflow builds all workspace binaries/examples, runs workspace tests
serially, prints binary SHA-256 and uploads the artifact for seven days.
The raw GitHub logs retain their timestamp/tab prefixes on blank lines and
package-manager progress whitespace. They are excluded from documentation
whitespace checks so that the downloaded evidence remains byte-identical.

| Task | Exact tested head | Run | Passed / failed / ignored |
|---|---|---|---|
| Records #707 | `abc5d134ffb50b7cca0c78ab975528a40cee0096` | [38561266393](https://github.com/autonomous-ai/lampOS/actions/runs/38561266393) | 3,814 / 0 / 29 |
| Judge audit #700 | `800ce47a5230d6b99f44dbcb92e56180646c6efc` | [38561284576](https://github.com/autonomous-ai/lampOS/actions/runs/38561284576) | 3,798 / 0 / 29 |
| Memory checkpoint #698 | `fed759ee7557243802b704e296e1cbef11c82071` | [38561301420](https://github.com/autonomous-ai/lampOS/actions/runs/38561301420) | 3,823 / 0 / 30 |
| Mood checkpoint #713 | `9b00b4b7ba23930b7030b4722c244e00ad345cee` | [38570324967](https://github.com/autonomous-ai/lampOS/actions/runs/38570324967) | 3,892 / 0 / 31 |
| Mood ready #713 | `08c90d6b26ea0ba1b39f52f663b70af0f5a29865` | [38578348793](https://github.com/autonomous-ai/lampOS/actions/runs/38578348793) | 3,918 / 0 / 31 |

Counts include standard libtest/doctest result lines and the custom
`turn-end-wire` process harness: 253, 254, 256, 262 and 263 completed
suites respectively. Earlier summaries omitted that custom harness's
12 passing tests; this correction changes no raw log or run outcome.
`test-count-audit.json` records both sets of totals and raw hashes. Ignored manual/device tests were not
executed. All five runs completed with conclusion `success`, including
release build, serial tests, binary hash and artifact-upload steps.

Records' uploaded `lamp-records` SHA-256 is
`df75d8e0848553627897f2f3dd74e9292cbaf5cc9385077c3539587f44525e22`.
Use the corresponding artifact for that task; binary identity is not a lamp
deployment claim.

Completed handoffs: First merged
[Records #721](https://github.com/autonomous-ai/lampOS/pull/721) and
[judge audit #722](https://github.com/autonomous-ai/lampOS/pull/722), both
offered non-draft on the exact heads above. First also merged
[Mood #727](https://github.com/autonomous-ai/lampOS/pull/727) at 23:01 ICT,
using the head and green native run in the last row. Its local Mac/Linux
gates passed, including 1,764 current-base Mac tests. First reports the
disabled check and a one-minute enabled trial on lamp-4ace passed;
[the receipt is on #713](https://github.com/autonomous-ai/lampOS/issues/713#issuecomment-6172610004).
It is not installed as a service yet, and First's metadata day continues
until tomorrow at 18:23 ICT. Those are First's observations, not Fourth's
measurements. Memory's subsequent full Mac
gate passed 1,469 tests, and its rebase including First's installed capture
passes focused Mac checks and strict Linux lint/cross-build. Its final head
failed its final native ARM64 run 38582911230 in a Linux-only capture
fixture's private-socket setup: 3,429 passed, 1 failed, 30 ignored before
Cargo stopped. Memory is not ready. The complete failure and owner-facing
diagnosis are in [the capture setup report](../arm64-capture-connect-20261014/).
Earlier privacy/motor failures remain retained. Green checkpoints do not
certify later heads or explain prior intermittent failures.

Mood's uploaded `lamp-mood` SHA-256 is
`4b5b2b681aab0a33a0de7b14daefb68a21d5f422aca4c85a4ba4ad59aa9fa114`.
That native build differs from the local x86_64-to-ARM64 cross-build; these
are separately identified artifacts, neither installed by Fourth.

## Not shown

No lamp deployment, physical procfs incident reproduction, on-lamp cleanup,
natural speech accuracy, audible response or memory conversation was tested
by these CI runs. First owns merging and the lamp checks. Earlier failed
runs remain in Fourth's adjacent verification folders.
