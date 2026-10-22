# Update fixture diagnostics: native qualification, 23 October 2026 (+07)

[PR #3085](https://github.com/autonomous-ai/lampOS/pull/3085) head
`996511111` adds failure diagnostics to the Update test fixture. The Rust
file is byte-identical to the previously read `f5f78c35c`; the replacement
corrects the Mac architecture label and records its current-base gate.

Both required jobs completed successfully for tested merge `bab1994`, head
`996511111` into `b8a9148bd`:

- [ARM64 39824358667](https://github.com/autonomous-ai/lampOS/actions/runs/39824358667/job/119983584749):
  **10,583 passed / 0 failed / 67 skipped**, 179.647 seconds in nextest.
  The Voice journal case is one leaky pass, retained in the receipt.
  The affected interrupted put-back case passed in **1.108 s**, the real
  CLI checker in **0.153 s**, and the seven-cut checker in **3.794 s**.
- [Linux Clippy 39824358767](https://github.com/autonomous-ai/lampOS/actions/runs/39824358767/job/119983584751):
  every crate and target with warnings denied, success. Its checkout also
  names `bab1994`, head `996511111` into `b8a9148bd`.

[native.json](native.json) preserves terminal job and workflow metadata,
checkout identity, exact result lines and complete log hashes. The native
watcher exited zero. No job was rerun for green. The prior failed Mac
consumer run remains unwaived; this diagnostic patch does not establish
its cause or clear it.

[The source PR's Mac receipt](https://github.com/autonomous-ai/lampOS/blob/996511111/lampOS/docs/verification/updater-fixture-exit-20261022/mac-current.json)
records native ARM64 macOS fmt, strict Update Clippy and the full Update
suite: **308 / 0 / 4 ignored**, Round **107 / 0**, no filtering, with 36
bound inputs. Its original controls and corrected post-run architecture
observation remain in the adjacent README and evidence.json.

Integration advanced to `28f796223` during CI. The
[carry preflight](carry-preflight.json) is a successful `git merge-tree`
conflict check only. It neither moves the qualified branch nor qualifies
a combined head. First owns the final current-base train and merge; Tests
has the requested replacement evidence re-read. No downloaded-artifact,
physical unplug, flash durability, device or real-release result is claimed.
