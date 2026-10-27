# Update public-output wrapper negative control

Exact repaired source `431b784c4b9e911575f79e7ad3ce879e61c2b8b9`, query SHA-256 `86d35d882ecf3f121c6b2bc35be8361b76ce38a00b28b4bd0c13c8094d9d319d`, tests SHA-256 `5ec4034c2389ef47fbb60436092f1170e9c9991dd2ad0beca9296fda9731d566`. The clean original archive was source-verified, then only scratch world::output_within was restored from b2344a07df6b38d2736e01297999751c26e477a5. The private query helper, all eight new tests and Cargo stayed unchanged. No deadline was disabled.

Matching Setup-independent Update --lib selection compiled with exit 0. Exactly `world::query::tests::public_output_accepts_one_mib_and_refuses_one_byte_more` ran once and failed as expected, Cargo exit 101:

`test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 132 filtered out; finished in 0.15s`

Its failure was the short assertion `public reader accepted output beyond 1 MiB`. Thus a bounded helper left unused by the actual public wrapper cannot satisfy this regression. The exact 1MiB positive assertion preceded the overflow refusal. No 1MiB panic payload is published. Runtime command 1.48s; bound 120s plus at most 15s cleanup. No retry or additional test ran.

The isolated target was atomically absent/empty before compilation. Original source/head/status stayed unchanged before/after. Closure retained 1 actual driver, 50 actual/preserved local outputs and 220 depfiles. Timeout cleanup signals only the owned unreaped group once, then waits; kill/wait errors are durable. This control did not need timeout cleanup. Commands, source/patch identities and raw-log hashes are included, with no binaries or scripts.

The prior partial standard-library-only diagnostic/collection failure remains a separate historical outcome; it is not erased, relabeled or counted as this wrapper control. This expected mutant failure is not readiness for the repaired implementation. Full current Update and native CI remain separate evidence.
