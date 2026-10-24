# #3190 native qualification at 414b2df34

Ready for lamp-voice: yes; READ-OK [6381396238](https://github.com/autonomous-ai/lampOS/pull/3190#issuecomment-6381396238), with train integration separate.

| Observation | Run | Actual primary results | Skipped |
| --- | --- | --- | ---: |
| Before, full ordinary ARM64 workspace at 9ed44f88c | 40055587720 | 11,163 PASS + 1 LEAK; zero failed/unrun | 71 |
| After, full ordinary ARM64 workspace at authored 414b2df34 | 40070482584 | 11,163 PASS + 1 LEAK; zero failed/unrun | 71 |
| PR ARM64, newer-base merge preview ad2a2c7f1 | 40070418423 | 7,348 PASS + 1 LEAK; zero failed/unrun | 45 |

The [before/after comparison](before-after.json) independently verifies exactly equal identity/status populations across all 11,164 full-workspace rows. Both use the same `pr` profile, configured `all()` filter and identical nextest configuration; realtime cases are included. Their 200.342/202.546-second test summaries are descriptive, not a speedup claim. The PR selector chooses 12 of 46 packages and its actual checkout merges authored 414b2df34 onto 5074d860c; that subset is not called a full-workspace result.

[Linux Clippy 40070418452](https://github.com/autonomous-ai/lampOS/actions/runs/40070418452) also succeeds at the PR merge checkout. The authored full-workspace dispatch actually checks out 414b2df34, confirmed by its exact `git log -1` output. All actual primary case rows are retained in the two census files; indices are complete and unique, and repeated display rows are accepted only when identical. The sole LEAK retains its actual journal case identity and is not silently recategorized. Separate doctest and own-harness results are not added to primary counts.

The [matched local rule](../tests-qualification-pair-4a84fa5ab-20261024/) remains **7,219/17/34 → 7,234/2/34**; [carried local qualification](../tests-current-3190-414b2df34-20261024/) remains **7,249/3/34**, including all 88 authored passes. All full failures and prescribed standalone results stay visible. Reviewer-two's independent serial Mac Voice gate passed **3,977/0/15**, which does not erase the parallel failures or identify their cause.

Fresh native downloads and byte digests are recorded here separately from earlier baseline-download digests. The earlier unavailable frozen commit object and historical log-byte binding remain explicit P3 provenance limits; no older hash is relabelled. The watcher API timeout was not a test failure. No local ARM build, product timing improvement or lamp/device action is claimed.
