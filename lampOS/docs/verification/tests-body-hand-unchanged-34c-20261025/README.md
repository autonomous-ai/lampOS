# One unchanged-base full-context Hand comparison

Ready for lamp-voice: no.

The single unchanged `34c8b517c` run completed with **7,321 passed / 15 failed / 52 ignored**. The earlier Hand candidate on that base completed with **7,316 passed / 20 failed / 52 ignored**. Both have all 79 targets bound and the same 7,388 case names; **all 440 Body library cases passed in both**. Each source ran once. Neither full test gate is green. Controller exit zero means the proof and restoration completed; its Cargo test exited 101.

Only `light_call::twenty_light_calls_are_each_answered_once_the_ring_says_it_shows_them` repeated a candidate failure. All twenty calls completed before both runs failed the same sorted-index-10 <25-ms predicate: candidate 57.608248 ms, baseline 108.901643 ms. The other nineteen candidate failures did not reproduce in this one base run. The baseline also failed fourteen Voice library cases that passed in the candidate. Exact names, raw panic blocks and source bodies remain in the audit records. No failure was ignored or retried.

Independent audit of 198 retained files explains 98 of 99 binary pairs exactly: every byte matches except opaque final rcgu object-name suffixes in LC_SYMTAB strings referenced exclusively by N_OSO debugger records. Code, data, UUIDs, load commands, symbol records, layout and all other bytes match for these 98, including all four affected integration targets and normal CLI witnesses. Only Body's lib-test binary has actual code/data/layout changes. This bounds the byte changes; it does not establish complete semantic or environment equivalence, identify a historical cause, or automatically waive an unreproduced failure.

The actual original twelve-package Cargo command, same worktree/target and default parallel harness followed a new metadata query and supported 48-local-package clean. Full-test wall time was 3,342.492547 seconds; no isolated speed or before/after improvement is claimed. The PATH-only difference remains explicit: stable relevant non-PATH values match, but original raw PATH/tool resolution cannot be reconstructed. Exact argv, bindings, resource declaration and source transitions are retained.

Candidate bytes, full modes and kinds were restored afterward; the two owned files have actual new mtimes, not backdated timestamps. All other tracked source bindings remained fixed. All 204 commands and 408 streams were verified, with matching direct waits, no timeout and their owned groups absent. This covers those groups, not arbitrary escaped descendants or all host processes. All 99 baseline plus 146 older protected executable witnesses were rehashed; 245 binary bodies remain private.

Raw Cargo metadata contains author values and stays private. Its SHA is `9e2937a52602494a9077a5684fd587246b09bba899620abbf64ce1cbfb2e1f51`; its command/binding receipts and safe graph are public. Python bodies, binaries, full Git personal metadata and raw environment values are omitted. Selected original text is unchanged. This README, outer bindings and checksums are derived publication aids. Current-head gates, Linux ARM64, device behavior and readiness require separate evidence.

The original raw test stdout retains its blank line at EOF. Only the literal path in `whitespace-exceptions.json` is excluded from the publication whitespace check; its original hash remains mandatory.
