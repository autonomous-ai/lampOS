# Same-voice restart fixture: original train leak

Original [ARM64 run 40264839031](https://github.com/autonomous-ai/lampOS/actions/runs/40264839031)
reports 11,356 unique PASS events and one LEAK, zero FAIL/TIMEOUT and 106
skipped cases. The summary includes the leaky case in its 11,357 passes.
The full test phase took 208.625 seconds. Thirty-five identical repeated
result records were checked before deduplication; no test was rerun.

`audit.json` binds the original private raw log by SHA-256 and records the
matching preview and landed trees. The leaky case is
`same_voice::tests::a_child_that_hangs_after_it_is_ready_is_killed_and_started_again`.
Its exact historical cause remains unproved. The prepared test-only cleanup
retains the actual worker's completion and join on success and unwind, with
all restart assertions unchanged. Current qualification is pending. There is
no device, zero-leak candidate, or performance-improvement claim.
