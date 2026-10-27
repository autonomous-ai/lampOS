# Parent hand controls: original native proof

Original native gate: passed. Exact-head reader and merge disposition remain separate; the train's ARM64 run on its merged tree supplies latest-base verification under First's current rule.

Published head `5b018ca5d0278f5c3655548c00c41a45ab58e9db` passed its original ARM64 run `40600417604` and Linux Clippy run `40600417514`. Both logs independently identify checkout `6b6a27fe2faaf76209231746a1aa5505bfca369c`, tree `cc19e3f51fc08e96b8bc408b51fbd7ef8a7a5faa`, with parents `89823095e69dcbafaa5b3b79733c17ecf9d8e8f7` and the published head. The authored tree is separately recorded; whole-tree equality is not assumed.

The original ARM run recorded **7,858 unique passes**, zero failures, leaks, timeouts, duplicate identities, retries, or selected cases left unrun. **All 23 required controls passed once**, including the Linux-only control. The summary reported 54 skips; identities are not invented from that total. Of the separate 16-case privacy cohort, nine controls were observed once and seven Bus/Msgs identities had no terminal. No full-cohort credit or selected-unrun classification is assigned to those seven.

Separate from nextest: 47 documentation targets produced four passing tests; the relay harness reported one aggregate pass; the process harness reported 13 distinct passes. Export logs reported 48 workspace binaries and two examples. The API and upload ZIP digest agree, but the artifact was not downloaded; no binary headers, pins, installed bytes, or physical behavior are verified here.

The earlier parent run remains **failed: 6,466 passed, three failed, and 1,336 selected cases unrun**. Those three cases passed once in this new original run on identical source files. That observation does not establish the earlier failure's cause or reclassify it. All earlier failed epochs remain preserved.

See [receipt.json](receipt.json), [required23-ledger.json](required23-ledger.json), and [source-ledger.json](source-ledger.json) for source identities, terminal evidence, and limits. No source, index, target, or log worktree was changed by this audit. No rerun, artifact download, deployment, or readiness transition was performed.
