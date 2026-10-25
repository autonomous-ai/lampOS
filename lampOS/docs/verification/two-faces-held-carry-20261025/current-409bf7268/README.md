# Carry onto the landed image correction

Qualified source `5e0910909` combines offered `246a353bd` with release base
`409bf7268`, preserving the original authored commits. The exact source tree
is recorded in identity.json. Its nine changed paths and their complete
binary diff equal the release delta from a6 to 409; every other Git entry
is unchanged.

Rust, Cargo manifests and lockfile, toolchain, compiled installation prose,
and the two lampOS workflow inputs consumed by the CI scanner are unchanged.
The independent full 4,691/0/21 and later 21+4 delta checks remain at their
actual earlier source trees, as recorded in current-a6ccb1aa3. They are not
claimed as reruns. Fresh `cargo +1.96.0 fmt --all --check` passes, and
`bash scripts/imager/tests/test_v2_image_inputs.sh` passes 5/0 on this carry.
The latter uses local stand-ins; no image build, flash or lamp was performed.

The previous offered head's original ARM64 40226248638 and Linux clippy
40226248637 both succeeded. Its actual merge preview equals its offered tree:
11,311 PASS, zero LEAK/FAIL/TIMEOUT, 106 skipped, all three two-face controls
once. The audited receipt is retained here with the raw log hash. This is
not the new carry's native result; fresh native CI and current reader lines
belong to the newly published head.

The feature remains off in product units. Earlier failed full results and
the tracked #3315 timing disposition remain recorded in the preceding proof.
No acoustic, physical two-person, provider or activation result is claimed.
