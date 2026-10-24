# Current f978 native evidence

Ready for lamp-voice: no

These are the original successful runs on the current authored carries, downloaded without a CI rerun. The complete console streams, run metadata and case projections are byte-for-byte hashed in manifest.json. Terminal case projections use only the primary nextest section before Summary, with contiguous unique indices and binary/case identities; echoed or later diagnostic blocks are excluded.

| PR and authored head | Original ARM64 run | PASS | LEAK | Selected terminal | Skipped | Original Linux fmt/strict workspace Clippy |
|---|---:|---:|---:|---:|---:|---:|
| #3403, 9ce73c76a | 40126362263 | 7,409 | 1 | 7,410 | 53 | 40126362254 |
| #3404, ea10e4c7d | 40126363634 | 7,399 | 1 | 7,400 | 53 | 40126363568 |

All four runs completed successfully. Neither ARM run has failed, timed-out or unrun selected cases. For every run, the actual checkout printed in its primary log resolves to a fetched preview commit with parents f9780cb2a and the exact authored head. Its whole tree equals that authored head's tree; complete-tree-proof.json records the full identities. This is actual new native evidence, not an old-green equivalence claim. No ARM64 build ran on the Mac and no device was used.

The newer frozen Mac full runs remain FAILED at Update 7,316/8/52 and NoPause 7,265/49/52, including the changed NoPause case. This successful native evidence does not qualify or waive those failures; bounded producer classification is still in progress. See the separate tests-failed-full-epochs-20261025 record. The PRs remain drafts.
