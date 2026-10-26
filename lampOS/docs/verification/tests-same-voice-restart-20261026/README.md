# Same-voice restart fixture completion

Part of #1569. Test-only fixture cleanup retains the actual `Prints::timed` worker, closes channels after the original answer-timeout and second-start assertions, waits for completion, and joins. The journal fixture still uses `Prints::journaled`. Production supervision is unchanged; unwinding retains ownership of the join.

On source head `cced429433211e06790f23ed8f51e07be613972d` plus the two reviewed dirty files, formatting, strict Clippy for twelve affected packages, and workspace build passed. The full `same_voice::tests::` module ran once from the original compiled executable: 21 passed, zero failed or ignored in 2.308 seconds. The actual executable inventory and outcomes match, with the changed restart case exactly once. Source, executable copies, and all 126 inherited witnesses remained unchanged.

The historical ARM64 run [40264839031](https://github.com/autonomous-ai/lampOS/actions/runs/40264839031) recorded 11,356 unique PASS, one LEAK, zero FAIL/TIMEOUT and 106 skips. Its cause remains unproved. That broad native selection and the local module run are different scopes; current native qualification is pending. This receipt does not claim full Voice or reverse-dependent runtime coverage.

`mac-gate.json` preserves result counts, case inventory and private-receipt hashes without raw build logs. Its source receipt's inherited `base` field is historical helper metadata, not the current lamp-voice base; the actual checked-out head and terminal base are cced42943. Authored commit `98be6a072e18da73a62368b34d72baaad77fc14f` has parent cced42943. Root verified all 10,652 tracked source bytes/modes against the preserved precommit receipt before committing. Original receipt identity remains unchanged.
