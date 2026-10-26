# Closed-output descendant negative control

Exact fixed source `220251c703aedf6babb148439d6fabe9602edbda`, reviewed query SHA-256 `5c0d9bcc352f8e90d0c30f8eb2ce7c506215280c47a35df9f56013e507e54866`. A local archive matched the clean original source inventory before mutation. Only the scratch query.rs production prefix was restored from `9784e3e26407b63584930c1303d60b722c3ddb7c`; the repaired source's tests remained unchanged, including the new regression. No deadline was disabled.

Compilation selected Setup --lib and the exact `reset::query::tests::a_failed_query_cleans_its_same_group_child_even_after_output_closes` case, exit 0. One exact runtime invocation failed as expected, Cargo exit 101:

`test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 23 filtered out; finished in 1.55s`

The failure was `failed query left its closed-output descendant running`, after the earlier nonzero exit-status-7 and closed-output-ready assertions passed. The child waits for a bounded release file; Rust writes that release only after observing the returned error/status/ready witness. Therefore its escaped write follows the returned error rather than racing a delayed leader. The fixture ends naturally, and the test never signals a reaped group. Runtime command 2.738s; configured bound 120s plus at most 15s cleanup. No retry or additional test case ran.

The separate target was atomically absent/empty before compilation. Original source/head/status remained unchanged before/after. Closure retained 1 actual driver, 15 actual/preserved local outputs and 139 depfiles. Timeout cleanup, if needed, sends one group SIGKILL only while the controller leader is unreaped, then waits; it never signals after wait and records kill/wait errors. This observed control needed no timeout cleanup. Exact commands, sanitized failure, source/patch and raw-log hashes accompany the packet; no binaries or scripts are included.

This is evidence that the old cleanup implementation fails the new regression, not readiness for the repaired implementation. The repaired full current-source gate and native CI are separate evidence. Historical green 9784 native results do not waive this defect.
