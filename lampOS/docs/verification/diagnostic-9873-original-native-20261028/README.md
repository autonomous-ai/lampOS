# Child diagnostic controls: original native proof

Ready: no new readiness transition is claimed.

Child #3577 head `9873c542ff2f3ecef64ec414adcc6f383fa0b85a` passed its original ARM64 run `40603825772` and Linux Clippy run `40603825773`. Both logs independently identify checkout `c07f43c30e5d3511d3ed47cef9ad7aac79d4d3e0`, tree `b123ebd838ce6503429fe6bb9339daac76445f5f`, with parents `c6520a4c02468bdba72b96040e84963de2ef6b1e` and the published child head.

The original ARM run recorded **7,870 unique passes**, zero failures, leaks, timeouts, duplicates, retries, or selected cases left unrun. **All 18 diagnostic controls passed once**, including the sequence control as a subset. The summary reported 54 skips; identities are not invented from that count. Separately, 47 documentation targets produced four passing tests, the relay harness reported one aggregate pass, and the process harness reported 13 distinct passes.

Both child-owned blobs match the published source. Two inherited hand-control files differ through the upstream spoken-sleep addition; its new control passed once. In total, **46 paths differ between the authored tree and actual merge tree**. Neither all-five-file equality nor whole-tree equality is claimed. The 275-test host run remains evidence for its separately bound authored source.

The export log reported 48 binaries and two examples, and the API/upload ZIP digest agrees. No artifact was downloaded; headers, installed bytes, compatible pins, or physical behavior are not verified here. All prior failed runs remain failed. Fresh passes establish no cause or waiver for those failures. No rerun, deployment, source mutation, or readiness transition was performed.

See [receipt.json](receipt.json), [required18-ledger.json](required18-ledger.json), and [source-ledger.json](source-ledger.json) for actual completion, named controls, source differences, and limits.
