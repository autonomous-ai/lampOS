# IPC full-journal fixture: current-base carry

Authored head `6e177dee0c40557fe7ceedc9b414734c26add031` contains base `0c1f6437ee57eae3416e3281d91b813d1c3bec1d`. The one commit from `7f7dfe42759a7d637e708804e13b78bee7e3c10e` carries unchanged: all seven authored files retain their Git blobs and modes, and the literal [range-diff](range-diff.txt) is equal. The 40 upstream path changes match the new base exactly.

The fresh Mac gate succeeds: workspace formatting, strict all-target IPC Clippy, test compilation, and the complete IPC crate tests. The full log contains **26 ordinary passes, zero failures; one separate doctest pass; and `relay_signals: ok`**. Relay is a custom harness and is not added to either count. [results.json](results.json) retains the literal summaries, exact commands, raw-log hashes and eight compiler-artifact hashes.

The audit checked all **7,197** source-inventory entries against current files and their committed Git blobs/modes. Every gate receipt says the source remained unchanged. All eight actual executable hashes match the retained compiler manifest; the gate also checked them after the full test run. The authored files and build manifests are in [inputs.sha256](inputs.sha256). No binary was executed by this read-only audit.

Earlier native evidence for the original `7f7dfe427` head remains separate, including its 11,328-pass result. New-head ARM64 and Linux Clippy evidence is not included here. No model/provider, lamp, systemd operation or physical latency acceptance is claimed. This packet adds no test execution or repository change.
