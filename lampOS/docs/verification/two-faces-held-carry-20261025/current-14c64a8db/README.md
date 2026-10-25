# Qualification on train141

Source `a9017ccb8` contains actual release base `14c64a8db`. Train141
landed #3497's Body unplug fixture correction and its evidence. The Body
production prefix before `#[cfg(test)]` is byte-identical to 9e9; all base
changes are listed in the identity record. The authored feature is unchanged.

Fresh native Mac formatting, strict all-target Body/Voice/Eval clippy,
workspace build and full serial Body tests passed **609/0/0 ignored**.
All 7092 tracked lampOS source hashes stayed fixed. The
commands, times, exits, literal summaries and raw-log hashes are retained.
This validates the changed base composition; it is not a rerun or relabelling
of the earlier full Voice/Eval qualification.

Full Body/Memory/Voice/Eval 5,613/0/29 remains on a497afbd1 / e1c69457a; affected 840/0 remains on 7ed73b534 / 9e9e6733a. Original ARM 40218880202 / Linux 40218880226 passed on a5f10fb09 / 9e9e6733a: 11,297 PASS plus one tracked LEAK, 106 skipped, all three controls once. Earlier reader full 4,674/1/21 remains failed under the recorded #3315 disposition; the isolated pass proves neither cause nor repair.

Rust 1.96.0, native x86_64 Mac, one compiler job, serial tests, existing owned
cache, incremental off and zero development/test debug info. Source identity
is not an all-fresh binary attestation. The original native CI on the newly
offered head remains separate. No local ARM64 link, lamp action, product
activation or acoustic acceptance is claimed.
