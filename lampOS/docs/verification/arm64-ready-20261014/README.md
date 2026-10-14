# Fourth's successful native ARM64 runs, 2026-10-14

Complete, unmodified `gh run view <run> --repo autonomous-ai/lampOS --log`
output is retained beside this file. These are GitHub-hosted Ubuntu 22.04
ARM64 ordinary-user runners, Rust 1.96.0, release mode; no lamp was used.
The workflow builds all workspace binaries/examples, runs workspace tests
serially, prints binary SHA-256 and uploads the artifact for seven days.

| Task | Exact tested head | Run | Passed / failed / ignored |
|---|---|---|---|
| Records #707 | `abc5d134ffb50b7cca0c78ab975528a40cee0096` | [38561266393](https://github.com/autonomous-ai/lampOS/actions/runs/38561266393) | 3,802 / 0 / 29 |
| Judge audit #700 | `800ce47a5230d6b99f44dbcb92e56180646c6efc` | [38561284576](https://github.com/autonomous-ai/lampOS/actions/runs/38561284576) | 3,786 / 0 / 29 |
| Memory checkpoint #698 | `fed759ee7557243802b704e296e1cbef11c82071` | [38561301420](https://github.com/autonomous-ai/lampOS/actions/runs/38561301420) | 3,811 / 0 / 30 |
| Mood checkpoint #713 | `9b00b4b7ba23930b7030b4722c244e00ad345cee` | [38570324967](https://github.com/autonomous-ai/lampOS/actions/runs/38570324967) | 3,880 / 0 / 31 |

Counts sum Cargo's final successful test-result lines, including doctests:
252, 253, 255 and 261 suites respectively. Ignored manual/device tests were not
executed. All four runs completed with conclusion `success`, including
release build, serial tests, binary hash and artifact-upload steps.

Records' uploaded `lamp-records` SHA-256 is
`df75d8e0848553627897f2f3dd74e9292cbaf5cc9385077c3539587f44525e22`.
Use the corresponding artifact for that task; binary identity is not a lamp
deployment claim.

Completed handoffs: First merged
[Records #721](https://github.com/autonomous-ai/lampOS/pull/721) and
[judge audit #722](https://github.com/autonomous-ai/lampOS/pull/722), both
offered non-draft on the exact heads above. Memory and Mood are **not ready**:
both have subsequently rebased onto `a9992f272`, including Eyes' lint repair
and First's eye behavior. Their current heads need their own green CI, and
the Mac gates are incomplete. Memory's newest full run failed an inherited
motor fixture; its raw output remains in the adjacent verification folder.
The green checkpoints do not certify later heads or erase prior failures.

Mood's uploaded `lamp-mood` SHA-256 is
`4b5b2b681aab0a33a0de7b14daefb68a21d5f422aca4c85a4ba4ad59aa9fa114`.
That native build differs from the local x86_64-to-ARM64 cross-build; these
are separately identified artifacts, neither installed by Fourth.

## Not shown

No lamp deployment, physical procfs incident reproduction, on-lamp cleanup,
natural speech accuracy, audible response or memory conversation was tested
by these CI runs. First owns merging and the lamp checks. Earlier failed
runs remain in Fourth's adjacent verification folders.
