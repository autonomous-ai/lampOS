# Own-keys current affected-crate gate

Head `ff17b8c4e57dccf6fa3fe16e8f83ab2b68e66474`, base `690dff041e331b5c89160901b4f38f3a9915e60b`. Cargo metadata identifies five direct Voice consumers: Skills and Voice-eval runtime dependencies; Cloud, Memory and Update development dependencies. The six complete crates were compiled and tested together with exactly the same package/feature/target selection. The unchanged Selftest, Setup, Health and Controls test suites were not repeated. The changed factory script and embedded Update test inputs are covered here.

Formatting and strict nine-crate/all-target Clippy pass. The six-crate outer target census is **6175/0/48** (passed/failed/ignored):

| Crate | Passed | Failed | Ignored |
|---|---:|---:|---:|
| lamp-cloud | 196 | 0 | 4 |
| lamp-memory | 330 | 0 | 8 |
| lamp-skills | 522 | 0 | 9 |
| lamp-update | 415 | 0 | 6 |
| lamp-voice | 4089 | 0 | 21 |
| lamp-voice-eval | 623 | 0 | 0 |

Doctests are separate: **0/0/0**. The Memory room fixture reports one nested child pass, explicitly excluded from the outer totals. [census.json](census.json) preserves every outer summary, child distinction, Cargo target and source-log line. The factory stand-in suite passes **94/0**, separately from Rust tests. The first combined `bash -n` invocation parsed only `steps.sh`; an additive standalone `bash -n deploy/factory/steps-test.sh` also passed. Both original and supplementary receipts are retained.

[results.json](results.json) records exact normalized commands, source and raw-log hashes, metadata scope and limitations. All 7,258 source entries and 66 compiler-emitted actual/preserved artifact pairs passed final guards and an independent packet recheck. [executables.json](executables.json) binds the emitted targets and hashes. Compilation alone is not execution; actual executed test drivers are identified in the census. No binaries are included.

Earlier failed artifact guards, install observations and parser reconciliations remain independent and unwaived. Historical 6,708/0/44, Update 230/0/2 and factory 86/0 results retain their earlier heads; they are not added to this gate. No local whole-workspace or ARM64 gate, provider/key use, installation, device, activation or physical latency acceptance is claimed.
