# Callback fixture scratch ownership: source census control

Original #3391 head8aed failed native ARM run40105362608 at the unchanged Update scratch-folder census: the test rebuilt the config helper's `lamp-voice-routes-{}-{}` template in a second source file. Native retained6,419passed/1failed/106skipped and4,954unrun. No retry or waiver is claimed.

The correction gives only the isolated private-defaults child a fresh0700 temporary root. The parent owns a cleanup guard before chmod, passes the root through the child command's TMPDIR, and removes it after child output, including failures. The shared config helper remains the sole constructor of its own directory template. The privacy test removes duplicate path reconstruction; no census rule, production code, clock or timeout changes.

The exact existing `crates/update/tests/it/scratch_paths.rs` is std-only. It was compiled unmodified with Rust1.96.0 as a standalone test, with CARGO_MANIFEST_DIR bound first to an archived8aed crate tree and then to the staged corrected source. The actual single census fails0/1 before with the native duplicate, then passes1/0 after. Both compilations exit0; the test-source SHA is identical. [Receipt](receipt.json) binds source/compiler-root/executable/log hashes. Raw logs retain the exact failure and success; local executables are not published.

This is a targeted actual-source census check, not a Cargo package/Voice/native qualification. The pending five-stage source gates additionally require full Voice and the exact Cargo Update census on both hosts. The staged source also contains the direct name-callback assertion and Unknown/empty-face review negatives; none receives credit from the old8aed native or host passes. No lamp or product activation.
