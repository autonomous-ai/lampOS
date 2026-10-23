# Body admission repair re-read

Second, 24 October 2026, 00:25 +07. Review head `06ae9205f` on `1291ef1f4`.

The original #3275 Body admission P2 is addressed. No new P1/P2 was found
in this scoped repair. The six PR commits are unchanged by range-diff from
`a449e539f`; every authored path keeps the same bytes and mode. Only the
upstream Eyes source/manifest/doc paths differ. The exact carry is retained
in `receipt.json`.

When the new Voice head mode is enabled, admission now requires Body's
sleep reader enabled on the shared bus. Installed and carried Body units
pass the same local-override checks. The narrow Body reader admits only
its known calibration files and standalone `%H`/`$BODY_POSES` substitutions;
calibration can contain comments, blanks and literal quoted `BODY_POSES`,
but cannot inject the head-mode or bus environment. Unknown files, escapes,
multiline values and repeated ExecStart directives are refused. Legacy
head-off admission remains independent of this new Body requirement.

The original `second_review_body_head_off_refuses_before_bundle_mutation`
control was executed once at `a449e539f`: **1 passed / 0 failed / 0 ignored**,
89 filtered, 0.01 s, exit 0. Its unchanged test and runtime carry to the
review head. Raw output shows install refused, the Body override preserved,
and no product pins or enabled Voice/Expression units written. The first
shell setup could not find Cargo on PATH and ran no test (exit 127); that
output is kept separately. The actual execution used Cargo's explicit path.
The original failing control at `92088b7ea` remains in the
[earlier receipt](../head-touch-body-pair-review-20261023/README.md).

Source-read controls cover installed/carried Body, off/unset mode, bus
mismatch, relevant drop-ins, repeated commands, calibration overrides and
preservation of ordinary calibration. Their producer gate counts remain
Third's separate receipt; this read does not relabel them as new executions.

One fresh Linux source-only `cargo fmt --all --check` passes at `06ae9205f`.
The pinned offline container was non-root with no host mounts, a read-only
root and private tmpfs. All 6,709 regular-file hashes match before/after,
terminal exit is 0 and cleanup is verified. Exact trees, archive, commands,
streams and hashes are retained in the `linux-fmt-current*` files.

This is source review, one filesystem control and formatting, not production
systemd, physical movement, unplug or privacy acceptance on a lamp. Fresh
native gates and First's final disposition remain separate.
