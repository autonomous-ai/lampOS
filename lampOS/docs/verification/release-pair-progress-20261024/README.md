# Release-pair progress, 24 October 2026

Ready for lamp-voice: no. This is retained intermediate host evidence, not a ready PR or lamp qualification.

## Memory

Source tree `db30c396c9ad1ebb88b84714265809cd1c22054f` on base `4a84fa5ab6a9133aeec555ec4bc0636dd953f660` passed both actual 6408↔6449 cases: **2 passed / 0 failed / 0 ignored**. The selected compiled driver `/target/debug/deps/release_pair-3fcc8857fb334971 --ignored --show-output --test-threads=1` executes the published AArch64 binaries through existing kernel binfmt in a rootless Debian 12 AMD64 container, with native `/tmp` for Unix sockets. These Memory payloads are byte-identical, SHA256 `9f595301253679a6c9a1aea232d14b081147fe966d87b6776899ede19ccb8deb`; two release labels are not two implementations.

Ordinary full Memory tests passed **314 / 0 / 8**; the two artifact cases are among the ignored tests in that separate run. Strict Memory all-target Clippy and workspace formatting passed. All 10,311 source files retained bytes, modes and mtimes, semantic index authority was unchanged, and all five owned containers were removed. Each actual direction retained 30 successful RPCs and three confirmed process reaps. The complete evidence export contains 14 regular files; four regenerated executable copies were verified before omission. The archive identity and exact terminal receipts are in `memory-export.json` and `memory-verified.json`; the binary archive/database files are not committed.

Earlier failures remain retained: the first 0/2 occurred before readiness; a later native socket control established chmod(0600) fails with EINVAL on the host evidence mount and succeeds in native tmpfs. The next attempt reached `forget_person` but failed 0/2 because the fixture used generic IDs instead of required known-person IDs, and its evidence export timed out (effective exit 125; complete retention unproved). The correction changes only synthetic IDs and their predicate/rules checks, preserving all original assertions and deadlines. No production behavior or permission guard is relaxed.

RPC timings span immediately before Unix-socket connection through complete JSON response parsing, excluding evidence writes. They are socket/SQLite observations, not service-startup, acoustic or lamp UX results. Remaining Mac/workspace/current-base gates and fresh PR native gates are pending. Signature trust, physical power loss, legacy JSONL-reader rollback and whole-lamp acceptance remain unqualified.

## Common retained full-suite failures

Frozen tree `7b5df82fd031432c9ee0cb32754fd5b9f622f351` on base `1d915d44c97fcdd980a8a5d9c9e4fecc9570d746` remains unpublished. Full Mac Voice was **4,063 / 15 / 15**; full Linux Voice **4,131 / 8 / 18**, both exit101. The embedded child failure is already represented by its failed parent. `common-failures.md` records each actual boundary and source comparison; all failing fixture/immediate inspected source paths match both base and comparison main 4a. This does not establish their cause or waive the failed gates.

All 63 core Common controls passed on both platforms. The separately named carry set has 15 Mac and 16 Linux passes, with Update census not selected by Voice and rotation Linux-only. Linux Update later passed **388 / 0 / 4**, Skills package **23 / 0 / 0**; formatting, strict workspace/all-target Clippy, workspace build and Eyes also passed. Mac Update remains active. No new native PR run or private-memory lamp acceptance is claimed.

## Records and shipping priority

#2809 Records remains separate: actual 6408/6449 Voice sessions and cleanup readers under a real native systemd host, with original root/proc/openat2 checks. Review found startup health can lag event writes and shared metadata uses projected events; the fixture is being corrected against both release sources before compilation. Its dedicated native workflow is being prepared. No successful actual Records pair, lamp action, row increase or release acceptance is claimed.
