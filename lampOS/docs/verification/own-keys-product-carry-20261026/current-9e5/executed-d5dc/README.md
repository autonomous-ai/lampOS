# ownkeys: current d5dc host gate

Recorded base `d5dc2e87f9a64ae8d21992145d3960035582e6cc`; recorded HEAD `b2c75ba618dd15e97dfaa133c81320b6d8095777`. The source inventory is bound separately in [results.json](results.json). The gate began on the authored carry head.

All recorded stages exited zero with unchanged source, and the original complete marker reports successful source and artifact guards. The full-run literal summaries total **6708 passed, 0 failed, 44 ignored**. Read the individual literal summaries and exact normalized commands in results.json; focused invocations and the IPC relay harness are kept separate. These are host libtest summaries, not a native nextest unique-case census.

[Executable bindings](executables.json) retain the compiler target, source, features and SHA-256. Actual compiler-emitted executable hashes rechecked unchanged; preserved copies also rechecked. Compiler artifacts include non-test executables where Cargo emitted them; compilation alone is not execution. Preserved copies are not claimed as the executables invoked by Cargo. Focused Voice commands explicitly name their actual library test executable.

All source bytes, modes and inventory membership, receipt/log hashes, and actual executable hashes were rechecked before this packet was written. Original raw logs and full inventories stay in the private evidence store, identified by hashes. Paths in published commands use `<source>`, `<target>` and `<gate>` placeholders; the Cargo executable is rendered as `cargo`. Commands run from `<source>/lampOS` with the pinned `+1.96.0`, one build job and incremental compilation disabled.

No older failed procedure, guard, control or native event is erased or waived. This is not ARM64, device, provider, activation, release or physical latency evidence.
