# Source-only boot identity finding on person-scope proof

Reviewed public #3437 head d7c9132e61fe88fd2e7229a24e57a17059d5054b. The receipt binds the exact Git blobs and SHA-256 of the two runtime source files and new test module. No compiler, test observation or device action ran for this read.

Both new positive-proof decisions call not_theirs with inputs still obtained from eyes.speakers and known_with_face. Those inputs are bare passing face IDs; seen_saying requires a nonempty speaker list and matching ID/person but cannot recover a discarded producer boot.

| Comparison in the proposed positive-proof path | Speaker input | Known-face input | Predicate implication |
|---|---|---|---|
| Bare IDs after a restart reuses the same ID | f1 | previous person's f1 | Accepted despite different producer boots |
| Existing boot-qualified evidence | new/f1 | old/f1 | Refused |

This is a logical source evaluation, not an executed reproduction. A still-fresh known name/face file written under the old eyes boot can name a different new-boot face with the same bare ID. The first decision can retain the old person's scope for a direct destructive forget or observation, and the late decision can authorize their keeper. HOLD 6392682413 records this source P1; it is not an execution READ-OK.

The existing speaking_faces test the_known_face_takes_the_boot_it_was_written_under already seals the fresh-old-file/reused-new-ID case and the timestamp boundary. Positive proof should use speakers_with_boot plus the same eyes instance's known_with_boot at both call sites, preserving the switch-off path and one-way narrowed-lease refusal. Regression coverage must traverse the actual call sites.

The new positive fixture currently writes the known-face sidecar before its feeder records any producer boot. known_with_boot correctly refuses that provenance. A valid same-boot positive setup must record its producer frame before writing the known-face pair; a missing-boot/bare-ID fallback would defeat the repair. Follow-up 6392691570 states that fixture requirement. Sixth retains implementation ownership and current readers retain execution/acceptance duties.
