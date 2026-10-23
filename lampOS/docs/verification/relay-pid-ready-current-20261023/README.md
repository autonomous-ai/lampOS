# Relay PID-ready gate on the current base

Head `e228f645212fa32d6e9c1446f6d01621fe2c563d`, parent
`cf462adb04f64d699c226dd3b1781c0ad927068f`. The worktree was clean before
and after both invocations. The two-file patch has an equal range-diff from
the original controlled source, whose receipt remains separate.

The first invocation passed formatting, strict all-target VoiceEval Clippy
and workspace build. Its full test package was launched in the restricted
sandbox by mistake: **509 passed, 114 failed**. Of those failures, 113
explicitly reported denied fixture socket bindings; one failed reading a
missing relay file; the cause of that missing-file observation was not traced.
The denied socket operations invalidate this invocation as full qualification;
the separate missing-file failure remains unclassified.
All output and command metadata are retained in [restricted.json](restricted.json).

The unchanged full package command then ran once with the required local
fixture socket permissions. **623 passed, 0 failed, 0 ignored**: 378 library,
7 binary and 238 integration tests; no doctest cases. All seven relay-hardening
cases passed. The command took 61.757 seconds including Cargo overhead.
[corrected.json](corrected.json) preserves the full streams and exact timings.
No filter, serial fallback, source edit or assertion change accompanied the
environment correction. Formatting, Clippy and build were not repeated.

Both used `x86_64-apple-darwin`, Rust 1.96.0, four compiler jobs, the same
MAIN target, offline locked dependencies, debug information and incremental
compilation disabled, and default libtest parallelism. The corrected invocation
preserved source mtimes and reused the immediately preceding same-worktree
artifacts; no other Cargo job intervened. Physical hardware/translation mode
was not established. No ARM build or device action was performed.

The JSON text fields preserve exact decoded UTF-8 bytes and hashes, including
the invalid run. Native ARM64 and Linux formatting are separate checks.
