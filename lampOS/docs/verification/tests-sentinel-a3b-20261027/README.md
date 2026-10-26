# Sentinel on a3b: Mac qualification

Local Sentinel carry for PR #3543, `5ac039201953f56a73f1210c41cf621a51f7413c`, on pinned
`a3b9c46814bb2b2452f0d6b6cc13bb3a16992c33` passed the complete actual Mac
`session_reader::tests::` module **4 passed, 0 failed, 0 ignored**, once serial.
The supervised observation took **0.594 s**; libtest reported **0.10 s**.
The compiled Voice library inventory contained **3,826** cases, with **3,822**
outside this module filtered out. Inventory is separate from runtime coverage;
no full Voice runtime is claimed.

Formatting, strict all-targets Clippy over the actual twelve-package Voice
reverse closure, workspace build and Voice library compilation passed. Only
`lamp-voice` was cleaned; its invalidated strict artifacts were `fresh:false`.
No transitive freshness claim is made. Workspace build preceded test compilation.
The compiler-bound test executable is **115,889,560 bytes**, SHA-256
`5eaceef3c0bfde1376740fcb6ba6f26081bdd3fc203844a1d67e99b8e61e44d5`,
with test profile, zero debuginfo, debug assertions, overflow checks and no features.

All **10,674** tracked files, modes and index stayed unchanged. The qualification
alias matched the preserved official source except for its branch label.
All **235** inherited sealed copies and the new independent library copy remained
intact: **236** final witnesses. Sealed copies and the actual module executable
retained every bound attribute, including inode. All eleven owned stage commands
and both controller/wrapper exited zero. All owned groups closed, the lease was
released, and no retry, timeout or reserve abort occurred.

This proof qualifies only the recorded `5ac039201`/`a3b9c4681` host source epoch. The later
`0c1d8bc14`/`90ca725c5` official source-only carry was unpublished at preparation and receives
no qualification here. The changed Linux-only case, privileged DAC, current native
CI and device behavior were unrun in this observation. The
[legacy 57 native proof](../tests-sentinel-57-native-20261027/) and
[older host proof](../tests-sentinel-2bf-20261027/) retain their original heads and
scopes. No runtime result is carried from them.

[audit.json](audit.json) records all four outcomes, the compile/profile binding,
actual inventory scope, stage exits, integrity checks and sanitized receipt hashes.
Preparation used existing receipts only; no source/target reads, binary copies,
compiler, runtime or CI operations were performed.
