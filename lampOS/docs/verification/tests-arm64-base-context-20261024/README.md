# Existing native ARM64 baseline context

These are existing integration-push runs, read without starting a new baseline or building ARM64 locally. They are not after-results for the current unpushed Tests carries.

| Integration head | Native run | Primary outcome | Skipped |
|---|---|---|---:|
| 4a84fa5ab | [40051625749](https://github.com/autonomous-ai/lampOS/actions/runs/40051625749) | 11,121 PASS + 1 LEAK, zero failures | 71 |
| 9ed44f88c | [40055587720](https://github.com/autonomous-ai/lampOS/actions/runs/40055587720) | 11,163 PASS + 1 LEAK, zero failures | 71 |

Every index and case identity before the primary summary is unique and contiguous, with 11,122 and 11,164 executed identities respectively. The same-voice journal LEAK remains separate. Counts for doctests and the separate process harness are not added to this primary census. Full raw logs are bound by SHA-256 in each JSON receipt; checkout hash lines match the recorded integration commits.

Actual `profile.pr` configuration is `default-filter = 'all()'` at both heads. Realtime cases are included, as the retained observed capture-hold rows demonstrate. The workflow header's older statement about skipping realtime is stale; it is not an execution rule. These integration pushes select the workspace. A future PR run may select fewer packages; record its actual filter/census and do not compare a subset as if it were the same workspace population. These runs use nextest's ordinary concurrent profile, not the four-shard serial workflow-dispatch mode.

The 42 additional identities between these integration heads are upstream source changes, not an effect of the Tests candidate. There is no cross-platform timing or causal improvement claim here. Candidate native results remain pending until its latest-source gates permit the own-branch push.
