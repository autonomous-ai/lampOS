# Current phone fixture qualification on 8219e711a

Head e288e8d0c7cdc9534f121934d7553b4b77108f0a contains base 8219e711a; both carried phone patches are preserved. Mac fmt, strict all-target Voice Clippy and fresh workspace-path compilation pass. Eight soft-start integration tests and 123 library controls pass (131 total, zero failed/ignored). The library includes the exact phone case, music authority and framing controls, hearing/census groups and all sixteen additional carry selectors. Filters overlap; selectors are not summed as separate executions.

All 7,025 tracked source hashes match before/after. Linux source-only offline formatting passes on the exact archived head; no Linux build/test runs locally, and the owned container is removed. Both source manifests and bounded receipts are included.

Compiler records observe 25 path identities, including vendor Sonora; 26 package names were invalidated. These are different counts. All observed workspace records are fresh=false; Sonora is cached in both compiler stages (Clippy 41/42 and compile 39/40 records fresh=false). Raw compiler streams remain private; projections bind their original hashes.

Earlier full Mac Voice 4,007/0/21 remains on 65127043e / e10ffd895, not this source. Later hearing/carry runtime changes prevent whole-tree equivalence. Official current native checks are requested separately. Historical local Update FAILED/ok/ok remains failed and held; #3403's separate qualification does not itself waive it. No device, signature-trust or physical acceptance claim.
