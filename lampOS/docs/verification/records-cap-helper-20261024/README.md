# Records cap-control preparation

The two-file candidate on `fourth/records-cap-controls-20261024` strengthens the existing oversized-evidence test with direct exact-limit and limit-plus-one calls at 4,096 and 65,536 bytes. No runtime function or cleanup guard changes; all six existing controls remain. Full Records/host/native qualification is pending an exclusively released gate target.

An isolated Mac Rust test harness uses the exact existing `plain_file` and `group_text` source functions. The current helper passes 1/0; a scratch-only variant removing only the over-limit rejection fails 0/1 as expected. This demonstrates that the added raw-reader assertion detects cap removal independently of event parsing. It is not a Linux package, systemd, artifact or physical qualification. Compiled binaries stay local and are not committed. The mutation never changes repository source.
